use crate::config::Config;
use crate::db::Database;
use crate::model::Email;
use crate::moodle::Moodle;
use crate::updater::update_loop;
use anyhow::Context;
use anyhow::Result;
use camino::Utf8PathBuf;
use opentelemetry::sdk::resource::{EnvResourceDetector, SdkProvidedResourceDetector};
use opentelemetry::sdk::{trace as sdktrace, Resource};
use opentelemetry_otlp::{HasExportConfig, WithExportConfig};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tracing::info;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Registry;

pub mod config;
pub mod db;
pub mod model;
pub mod moodle;
pub mod server;
pub mod updater;

fn init_tracer() -> Result<sdktrace::Tracer> {
    let mut exporter = opentelemetry_otlp::new_exporter().tonic().with_env();

    println!(
        "Using opentelemetry endpoint {}",
        exporter.export_config().endpoint
    );

    // overwrite the service name because k8s service name does not always matches what we want
    std::env::set_var("OTEL_SERVICE_NAME", env!("CARGO_PKG_NAME"));

    let resource = Resource::from_detectors(
        Duration::from_secs(0),
        vec![
            Box::new(EnvResourceDetector::new()),
            Box::new(SdkProvidedResourceDetector),
        ],
    );

    println!("Using opentelemetry resources {:?}", resource);

    Ok(opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(sdktrace::config().with_resource(resource))
        .install_batch(opentelemetry::runtime::Tokio)?)
}

fn init_tracing() -> Result<()> {
    let tracer = init_tracer().context("Setting up the opentelemetry exporter")?;

    let default = "info,moodle_session_ext=trace"
        .parse()
        .expect("hard-coded default directive should be valid");

    Registry::default()
        .with(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(default)
                .from_env_lossy(),
        )
        .with(
            tracing_subscriber::fmt::Layer::new()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .event_format(tracing_subscriber::fmt::format::Format::default().compact()),
        )
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .init();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::var("CONFIG")
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|_| Utf8PathBuf::from_str("config.yml").unwrap());

    let config = std::fs::read_to_string(config_path).context("Reading config file")?;
    let config: Config = serde_yaml::from_str(&config).context("Parsing config file")?;

    println!("config = {:#?}", config);

    init_tracing()?;

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
