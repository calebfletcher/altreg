use std::{fs, net::IpAddr, path::PathBuf};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub data_dir: PathBuf,
    pub external_url: String,
    pub offline: bool,
}

pub fn load() -> Result<Config, anyhow::Error> {
    let config = toml::from_str(
        &fs::read_to_string("config.toml").with_context(|| "unable to read config file")?,
    )
    .with_context(|| "unable to decode config")?;

    Ok(config)
}
