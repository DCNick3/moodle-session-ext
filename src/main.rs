use crate::config::Config;
use crate::db::Database;
use crate::model::Email;
use crate::moodle::Moodle;
use crate::updater::update_loop;
use anyhow::Context;
use anyhow::Result;
use camino::Utf8PathBuf;
use opentelemetry::sdk::Resource;
use opentelemetry::{sdk, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::select;
use tracing::info;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub mod config;
pub mod db;
pub mod model;
pub mod moodle;
pub mod server;
pub mod updater;

fn read_homeycomb_key() -> Option<String> {
    std::env::var("HONEYCOMB_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("HONEYCOMB_API_KEY_FILE")
                .ok()
                .and_then(|f| std::fs::read_to_string(f).ok())
        })
        .map(|s| s.trim().to_string())
}

fn init_tracing(env_filter: String) {
    let fmt = tracing_subscriber::fmt::layer().with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);
    let filter = tracing_subscriber::filter::EnvFilter::builder().parse_lossy(env_filter);

    if let Some(honeycomb_key) = read_homeycomb_key() {
        println!("NOTE: Honeycomb key found");

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_trace_config(sdk::trace::Config::default().with_resource(Resource::new([
                KeyValue::new("service.name", "moodle-session-ext"),
            ])))
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .http()
                    .with_endpoint("https://api.honeycomb.io/v1/traces")
                    .with_headers(HashMap::from([(
                        "x-honeycomb-team".to_string(),
                        honeycomb_key,
                    )])),
            )
            .install_batch(opentelemetry::runtime::Tokio)
            // .install_simple()
            .unwrap();

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt)
            .with(tracing_opentelemetry::OpenTelemetryLayer::new(tracer))
            .init();
    } else {
        println!("NOTE: Honeycomb key NOT found");
        tracing_subscriber::registry().with(filter).with(fmt).init();
    };
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::var("CONFIG")
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|_| Utf8PathBuf::from_str("config.yml").unwrap());

    let config = std::fs::read_to_string(config_path).context("Reading config file")?;
    let config: Config = serde_yaml::from_str(&config).context("Parsing config file")?;

    println!("config = {:#?}", config);

    init_tracing(
        std::env::var("RUST_LOG") // RUST_LOG is top priority
            .ok()
            .or(config.logging.filter) // then we read config
            .unwrap_or_else(|| "".to_string()),
    );

    info!("Starting...");

    let db = Arc::new(Database::new(config.database)?);
    let moodle = Arc::new(Moodle::new(config.moodle)?);

    let update_fut = update_loop(
        db.clone(),
        moodle.clone(),
        db.subscribe_queue_updates()?,
        config.updater,
    );
    let server_fut = server::run(db.clone(), moodle.clone(), config.server);

    select! {
        r = update_fut => {
            info!("Update loop finished");
            r.context("In updater")
        }
        s = server_fut => {
            info!("Server loop finished");
            s.context("In server")
        }
    }?;

    Ok(())
}
