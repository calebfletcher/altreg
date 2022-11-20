mod config;
pub mod mirror;
pub mod package;

use std::{fs, net::SocketAddr};

use anyhow::{anyhow, Context};
use axum::{
    body::Bytes,
    extract::Path,
    http::StatusCode,
    http::Uri,
    response::IntoResponse,
    routing::{get, put},
    Extension, Json, Router,
};
use config::Config;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};
use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::package::Package;

#[derive(Serialize, Deserialize)]
struct Entry {
    versions: Vec<Package>,
    time_of_last_update: chrono::DateTime<chrono::Utc>,
    is_local: bool,
}

struct InternalError(anyhow::Error);

impl IntoResponse for InternalError {
    fn into_response(self) -> axum::response::Response {
        tracing::warn!("stacktrace: {:?}", self.0);
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "something went wrong",
        )
            .into_response()
    }
}

impl<T: Into<anyhow::Error>> From<T> for InternalError {
    fn from(e: T) -> Self {
        Self(e.into())
    }
}

async fn index_config(Extension(config): Extension<Config>) -> Json<Value> {
    Json(json!({ "dl": config.external_url.clone() + "/crates", "api": config.external_url }))
}

async fn crate_metadata(
    Path(parts): Path<Vec<String>>,
    Extension(db): Extension<sled::Db>,
    Extension(config): Extension<Config>,
) -> Result<(StatusCode, String), InternalError> {
    let crate_name = parts.last().expect("invalid route to crate_metadata");
    info!(crate = crate_name, "pulling crate metadata");

    let cache_entry = db
        .get(crate_name)
        .with_context(|| "could not access cache entry")?;
    if let Some(entry) = cache_entry {
        let entry: Entry = bincode::deserialize(&entry)
            .with_context(|| "could not deserialise metadata in cache entry")?;

        let has_expired =
            chrono::Utc::now() - entry.time_of_last_update > chrono::Duration::minutes(30);
        if config.offline || entry.is_local || !has_expired {
            info!(crate = crate_name, "returning metadata from cache");
            return entry
                .versions
                .into_iter()
                .map(|version| serde_json::to_string(&version))
                .try_fold(String::new(), |mut acc, item| {
                    acc.push_str(
                        &item.with_context(|| "could not convert version metadata to json")?,
                    );
                    acc.push('\n');
                    Ok(acc)
                })
                .map(|metadata| (StatusCode::OK, metadata));
        } else {
            // Expired crate
            info!(crate = crate_name, "crate in cache has expired");

            db.remove(crate_name)
                .with_context(|| "could not remove entry from cache")?;
        }
    };

    if config.offline {
        return Ok((StatusCode::NOT_FOUND, "not found".to_owned()));
    }

    info!(crate = crate_name, "pulling crate metadata from upstream");
    let upstream = mirror::get_package(crate_name)
        .await
        .with_context(|| "could not get package from upstream")?;

    let upstream = match upstream {
        Some(value) => value,
        None => return Ok((StatusCode::NOT_FOUND, "not found".to_owned())),
    };

    // Upstream JSON format to binary representation
    let versions: Vec<Package> = upstream
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .with_context(|| "could not parse upstream json metadata")?;

    let entry = Entry {
        versions,
        time_of_last_update: chrono::Utc::now(),
        is_local: false,
    };

    // Insert binary representation into database
    db.insert(
        crate_name,
        bincode::serialize(&entry).with_context(|| "could not serialise cache entry")?,
    )
    .with_context(|| "could not insert cache entry")?;

    Ok((StatusCode::OK, upstream))
}

async fn crate_download(
    Path((crate_name, version)): Path<(String, String)>,
    Extension(state): Extension<Config>,
) -> Result<(StatusCode, Bytes), InternalError> {
    let cache_path = state
        .data_dir
        .join("crates")
        .join(crate_path(&crate_name, &version));
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

fn create_error(msg: &str) -> Result<(StatusCode, Json<Value>), InternalError> {
    Ok((
        StatusCode::BAD_REQUEST,
        Json(json!({ "errors": [{"detail": msg}]})),
    ))
}

async fn add_crate(
    body: Bytes,
    Extension(db): Extension<sled::Db>,
    Extension(state): Extension<Config>,
) -> Result<(StatusCode, Json<Value>), InternalError> {
    if body.len() < 4 {
        return create_error("body too short");
    }
    let meta_length = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let data_offset = 4 + meta_length;

    if body.len() < data_offset + 4 {
        return create_error("body too short");
    }
    let metadata = body.slice(4..data_offset);

    let data_length =
        u32::from_le_bytes(body[data_offset..data_offset + 4].try_into().unwrap()) as usize;
    if body.len() < data_offset + 4 + data_length {
        return create_error("body too short");
    }
    let data = body.slice(data_offset + 4..data_offset + 4 + data_length);
    let metadata: package::Metadata = serde_json::from_slice(&metadata)?;

    let crate_name = metadata.name.clone();
    let crate_version = metadata.vers.clone();
    let cksum = format!("{:x}", Sha256::digest(&data));

    // Check if crate already exists
    let cache_entry = db
        .get(&metadata.name)
        .with_context(|| "could not access cache entry")?;
    if let Some(entry) = cache_entry {
        // If it already exists, add a new version to the entryinfo!("found crate in cache");
        let mut entry: Entry = bincode::deserialize(&entry)
            .with_context(|| "could not deserialise metadata in cache entry")?;

        // Make sure we don't publish new versions of existing upstream crates
        if !entry.is_local {
            return create_error(
                "attempted to upload crate with the same name as a cached upstream crate",
            );
        }

        // Check that it is valid to upload this version
        let new_version = Version::parse(&metadata.vers)?;
        for version in &entry.versions {
            let existing_version = Version::parse(&version.vers)?;
            if new_version == existing_version {
                return create_error("attempted to upload existing version");
            }
            if new_version < existing_version {
                return create_error("attempted to upload older version");
            }
        }

        // Add this version
        entry.versions.push(metadata.into_package(cksum));
        entry.time_of_last_update = chrono::Utc::now();
        // TODO: transaction aware db update
        db.remove(&crate_name)
            .with_context(|| "could not remove entry from cache")?;
        db.insert(
            &crate_name,
            bincode::serialize(&entry).with_context(|| "could not serialise cache entry")?,
        )
        .with_context(|| "could not insert cache entry")?;
    } else {
        // If it doesn't exist, create a new entry
        let entry = Entry {
            versions: vec![metadata.into_package(cksum)],
            time_of_last_update: chrono::Utc::now(),
            is_local: true,
        };
        db.insert(
            &crate_name,
            bincode::serialize(&entry).with_context(|| "could not serialise cache entry")?,
        )
        .with_context(|| "could not insert cache entry")?;
    }

    // Store crate file
    let cache_path = state
        .data_dir
        .join("crates")
        .join(crate_path(&crate_name, &crate_version));
    let parent = cache_path
        .parent()
        .ok_or_else(|| anyhow!("invalid cache path"))?;
    if !parent.exists() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = File::create(cache_path).await?;
    file.write_all(&data).await?;

    Ok((StatusCode::OK, Json(json!({}))))
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

    // Directory checks
    if !config.data_dir.exists() {
        fs::create_dir(&config.data_dir).with_context(|| "unable to create data dir")?;
    }
    let crates_dir = config.data_dir.join("crates");
    if !crates_dir.exists() {
        fs::create_dir(&crates_dir).with_context(|| "unable to create crate cache dir")?;
    }

    let db = sled::open(config.data_dir.join("db"))
        .with_context(|| format!("unable to open database in {}", config.data_dir.display()))?;

    let listen_addr = SocketAddr::new(config.host, config.port);

    let app = Router::new()
        .route("/index/config.json", get(index_config))
        .route("/index/1/:crate_name", get(crate_metadata))
        .route("/index/2/:crate_name", get(crate_metadata))
        .route("/index/:first/:second/:crate_name", get(crate_metadata))
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
        .await?;

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
