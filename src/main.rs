use crate::db::Database;
use crate::model::Email;
use crate::moodle::Moodle;
use anyhow::Result;
use camino::Utf8PathBuf;
use reqwest::Url;
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

    info!("Starting...");

    let db = Database::new(db_config)?;
    let moodle = Moodle::new(moodle_config)?;

    let email = Email("n.strygin@innopolis.university".to_string());

    db.add_token(&email, "REDACTED", "REDACTED")?;
    db.add_token(&email, "MOODLE2", "")?;
    db.add_token(&email, "MOODLE2", "")?;
    db.add_token(&email, "MOODLE3", "")?;
    db.add_token(&email, "MOODLE4", "")?;

    println!("{:?}", db.get_most_urgent_token()?);

    println!("{}", db.dump()?);

    println!("{:?}", moodle.check_session("kekas").await?);
    println!("{:?}", moodle.check_session("REDACTED").await?);

    println!("{:?}", moodle.update_session("REDACTED", "REDACTED").await?);

    Ok(())
}
