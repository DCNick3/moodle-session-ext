use crate::db::Database;
use crate::model::Email;
use anyhow::Result;
use camino::Utf8PathBuf;
use tracing::info;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub mod config;
pub mod db;
pub mod model;
pub mod moodle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_span_events(FmtSpan::ENTER | FmtSpan::EXIT))
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    // TODO: load config and stuff...

    let db_config = config::Database {
        path: Utf8PathBuf::from("sessions.db"),
    };

    info!("Starting...");

    let db = Database::new(db_config)?;

    let email = Email("n.strygin@innopolis.university".to_string());

    db.add_token(&email, "MOODLE1".to_string())?;
    db.add_token(&email, "MOODLE2".to_string())?;
    db.add_token(&email, "MOODLE2".to_string())?;
    db.add_token(&email, "MOODLE3".to_string())?;
    db.add_token(&email, "MOODLE4".to_string())?;

    println!("{}", db.dump()?);

    Ok(())
}
