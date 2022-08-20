use crate::db::Database;
use crate::model::Email;
use crate::moodle::Moodle;
use crate::updater::update_loop;
use anyhow::Context;
use anyhow::Result;
use camino::Utf8PathBuf;
use reqwest::Url;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_span_events(FmtSpan::NEW | FmtSpan::CLOSE))
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    // TODO: load config and stuff...

    let db_config = config::Database {
        path: Utf8PathBuf::from("sessions.db"),
    };
    let moodle_config = config::Moodle {
        base_url: Url::parse("https://moodle.innopolis.university/").unwrap(),
        rpm: 120,
        max_burst: 12,
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/104.0.0.0 Safari/537.36".to_string()
    };
    let updater_config = config::Updater {
        gap: Duration::from_secs(30 * 60),
    };
    let server_config = config::Server {
        endpoints: vec![SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 8080))],
    };

    info!("Starting...");

    let db = Arc::new(Database::new(db_config)?);
    let moodle = Arc::new(Moodle::new(moodle_config)?);

    println!("{}", db.dump()?);

    let update_fut = update_loop(
        db.clone(),
        moodle.clone(),
        db.subscribe_queue_updates()?,
        updater_config,
    );
    let server_fut = server::run(db.clone(), moodle.clone(), server_config);

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
