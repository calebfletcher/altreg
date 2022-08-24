use axum::body::Bytes;

use crate::{crate_path, crate_prefix};

static CRATES_IO_INDEX: &str = "https://index.crates.io";
static CRATES_IO_INDEX_DL: &str = "https://crates.io/api/v1/crates";

pub async fn get_package(name: &str) -> Result<String, anyhow::Error> {
    tracing::info!("checking {name} in crates.io index");
    let prefix = crate_prefix(name);
    let url = format!("{}/{}/{}", CRATES_IO_INDEX, prefix, name);

    Ok(reqwest::get(url).await?.text().await?)
}

pub async fn download_crate(name: &str, version: &str) -> Result<Bytes, anyhow::Error> {
    tracing::info!("downloading {name}@{version} from crates.io");
    let url = format!("{}/{}", CRATES_IO_INDEX_DL, crate_path(name, version));
    Ok(reqwest::get(url).await?.bytes().await?)
}
