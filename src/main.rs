mod config;
pub mod mirror;
pub mod package;

use std::{fs, net::SocketAddr};

use anyhow::Context;
use axum::{
    body::Bytes,
    extract::Path,
    http::Uri,
    routing::{get, put},
    Extension, Json, Router,
};
use config::Config;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::package::Package;

async fn index(Extension(state): Extension<Config>) -> Json<Value> {
    let host = format!("http://{}:{}", state.host, state.port);
    Json(json!({ "dl": host.clone() + "/crates", "api": host }))
}

async fn crate_fallback(uri: Uri, Extension(db): Extension<sled::Db>) -> String {
    let mut parts = uri
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let crate_name = parts.pop().unwrap();

    let cache_entry = db.get(crate_name).unwrap();
    if let Some(entry) = cache_entry {
        info!("found crate in cache");

        let versions: Vec<Package> = bincode::deserialize(&entry).unwrap();

        versions
            .into_iter()
            .map(|version| serde_json::to_string(&version).unwrap())
            .fold(String::new(), |mut acc, item| {
                acc.push_str(&item);
                acc.push('\n');
                acc
            })
    } else {
        info!("pulling crate metadata from upstream");
        let upstream = mirror::get_package(crate_name).await.unwrap();

        // Upstream JSON format to binary representation
        let versions: Vec<Package> = upstream
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        // Insert binary representation into database
        db.insert(crate_name, bincode::serialize(&versions).unwrap())
            .unwrap();

        upstream
    }
}

async fn crate_download(
    Path((crate_name, version)): Path<(String, String)>,
    Extension(state): Extension<Config>,
) -> Bytes {
    let cache_path = state
        .data_dir
        .join("crates")
        .join(crate_path(&crate_name, &version));
    if cache_path.exists() {
        tracing::info!("using cached {crate_name}@{version}");
        let mut file = tokio::fs::File::open(cache_path).await.unwrap();
        let mut buf = Vec::with_capacity(file.metadata().await.unwrap().len() as usize);
        file.read_to_end(&mut buf).await.unwrap();

        buf.into()
    } else {
        let bytes = mirror::download_crate(&crate_name, &version).await.unwrap();

        let parent = cache_path.parent().unwrap();
        if !parent.exists() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        let mut file = tokio::fs::File::create(cache_path).await.unwrap();
        file.write_all(&bytes).await.unwrap();

        bytes
    }
}

async fn add_crate(body: Bytes) {
    dbg!(body);
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialise logging system
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "altreg=info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::load().with_context(|| "unable to load config")?;
    let db = sled::open(config.data_dir.join("db"))?;

    // Directory checks
    if !config.data_dir.exists() {
        fs::create_dir(&config.data_dir).with_context(|| "unable to create data dir")?;
    }
    let crates_dir = config.data_dir.join("crates");
    if !crates_dir.exists() {
        fs::create_dir(&crates_dir).with_context(|| "unable to create crate cache dir")?;
    }

    let listen_addr = SocketAddr::new(config.host, config.port);

    let app = Router::new()
        .route("/config.json", get(index))
        .route("/crates/:crate_name/:version/download", get(crate_download))
        .route("/api/v1/crates/new", put(add_crate))
        .fallback(get(crate_fallback))
        .layer(Extension(config))
        .layer(Extension(db))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    axum::Server::bind(&listen_addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

fn crate_prefix(name: &str) -> String {
    match name.len() {
        1 => "1".to_owned(),
        2 => "2".to_owned(),
        3 => format!("3/{}", name.chars().next().unwrap()),
        _ => {
            let chars: Vec<_> = name.chars().take(4).collect();
            format!("{}{}/{}{}", chars[0], chars[1], chars[2], chars[3])
        }
    }
}

fn crate_path(name: &str, version: &str) -> String {
    format!("{}/{}/download", name, version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_prefix_samples() {
        assert_eq!(crate_prefix("a"), "1");
        assert_eq!(crate_prefix("ab"), "2");
        assert_eq!(crate_prefix("abc"), "3/a");
        assert_eq!(crate_prefix("cargo"), "ca/rg");
    }
}
