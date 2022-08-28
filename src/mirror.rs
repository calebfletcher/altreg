use anyhow::anyhow;
use axum::body::Bytes;
use reqwest::StatusCode;

use crate::{crate_path, crate_prefix};

static CRATES_IO_INDEX: &str = "https://index.crates.io";
static CRATES_IO_INDEX_DL: &str = "https://crates.io/api/v1/crates";

pub async fn get_package(name: &str) -> Result<Option<String>, anyhow::Error> {
    tracing::info!("checking {name} in crates.io index");
    let prefix = crate_prefix(name);
    let url = format!("{}/{}/{}", CRATES_IO_INDEX, prefix, name);

    let response = reqwest::get(url)
        .await?
        .error_for_status()
        .map(|response| Some(response))
        .or_else(|e| match e.status() {
            Some(StatusCode::NOT_FOUND) => Ok(None),
            Some(_) => Err(e.into()),
            None => Err(anyhow!("unable to decode status code")),
        })?;

    if let Some(response) = response {
        Ok(Some(response.text().await?))
    } else {
        Ok(None)
    }
}

pub async fn download_crate(name: &str, version: &str) -> Result<Option<Bytes>, anyhow::Error> {
    tracing::info!("downloading {name}@{version} from crates.io");
    let url = format!("{}/{}", CRATES_IO_INDEX_DL, crate_path(name, version));

    let response = reqwest::get(url)
        .await?
        .error_for_status()
        .map(|response| Some(response))
        .or_else(|e| match e.status() {
            Some(StatusCode::NOT_FOUND) => Ok(None),
            Some(_) => Err(e.into()),
            None => Err(anyhow!("unable to decode status code")),
        })?;

    if let Some(response) = response {
        Ok(Some(response.bytes().await?))
    } else {
        Ok(None)
    }
}
