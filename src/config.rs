use camino::Utf8PathBuf;
use reqwest::Url;
use serde::de;
use serde::{Deserialize, Deserializer};
use std::net::SocketAddr;
use std::time::Duration;

fn deserialize_path<'de, D>(de: D) -> Result<Utf8PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = de::Deserialize::deserialize(de)?;
    Ok(Utf8PathBuf::from(s))
}

fn deserialize_url<'de, D>(de: D) -> Result<Url, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &'de str = de::Deserialize::deserialize(de)?;
    Ok(Url::parse(s).map_err(de::Error::custom)?)
}

#[derive(Debug, Deserialize)]
pub struct Database {
    #[serde(deserialize_with = "deserialize_path")]
    pub path: Utf8PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Moodle {
    #[serde(deserialize_with = "deserialize_url")]
    pub base_url: Url,
    pub rpm: u32,
    pub max_burst: u32,
    pub user_agent: String,
}

#[derive(Debug, Deserialize)]
pub struct Updater {
    pub gap: Duration,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub endpoints: Vec<SocketAddr>,
}
