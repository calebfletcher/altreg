use axum::body::Bytes;

use crate::crate_prefix;

static CRATES_IO_INDEX: &str = "https://index.crates.io";
static CRATES_IO_INDEX_DL: &str = "https://crates.io/api/v1/crates";

pub async fn get_package(name: &str) -> Result<String, anyhow::Error> {
    let prefix = crate_prefix(name);
    let url = format!("{}/{}/{}", CRATES_IO_INDEX, prefix, name);

    Ok(reqwest::get(url).await?.text().await?)
}

pub async fn download_crate(name: &str, version: String) -> Result<Bytes, anyhow::Error> {
    let url = format!("{}/{}/{}/download", CRATES_IO_INDEX_DL, name, version);
    Ok(reqwest::get(url).await?.bytes().await?)
}
