use anyhow::anyhow;
use axum::{
    body::Bytes,
    extract::{Path, State},
    routing::get,
    Router,
};
use reqwest::StatusCode;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::{config::Config, crate_path, mirror, AppState, InternalError};

pub fn router() -> Router<AppState> {
    Router::new().route("/crates/:crate_name/:version/download", get(crate_download))
}

async fn crate_download(
    Path((crate_name, version)): Path<(String, String)>,
    State(state): State<Config>,
) -> Result<(StatusCode, Bytes), InternalError> {
    let cache_path = crate_path(state.data_dir, &crate_name, &version);
    if cache_path.exists() {
        tracing::info!("using cached {crate_name}@{version}");
        let mut file = File::open(cache_path).await?;
        let mut buf = Vec::with_capacity(file.metadata().await?.len() as usize);
        file.read_to_end(&mut buf).await?;

        Ok((StatusCode::OK, buf.into()))
    } else {
        if state.offline {
            return Ok((StatusCode::NOT_FOUND, Bytes::new()));
        }

        let bytes = match mirror::download_crate(&crate_name, &version).await? {
            Some(bytes) => bytes,
            None => return Ok((StatusCode::NOT_FOUND, Bytes::new())),
        };

        let parent = cache_path
            .parent()
            .ok_or_else(|| anyhow!("invalid cache path"))?;
        if !parent.exists() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = File::create(cache_path).await?;
        file.write_all(&bytes).await?;

        Ok((StatusCode::OK, bytes))
    }
}
