use anyhow::anyhow;
use axum::{
    body::Bytes,
    extract::{Path, State},
    routing::{delete, put},
    Json, Router,
};
use reqwest::StatusCode;
use semver::Version;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{fs::File, io::AsyncWriteExt, sync::mpsc::UnboundedSender};
use tracing::info;

use crate::{
    config::Config,
    crate_path,
    package::{self, UploadedPackage},
    token::ApiAuth,
    AppState, Entry, InternalError,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/crates/new", put(add_crate))
        .route("/v1/crates/:crate_name/:version/yank", delete(yank_crate))
        .route("/v1/crates/:crate_name/:version/unyank", put(unyank_crate))
}

fn create_error(msg: &str) -> Result<(StatusCode, Json<Value>), InternalError> {
    Ok((
        StatusCode::BAD_REQUEST,
        Json(json!({ "errors": [{"detail": msg}]})),
    ))
}

async fn add_crate(
    ApiAuth(token, user): ApiAuth,
    State(db): State<crate::Db>,
    State(state): State<Config>,
    State(docs_queue_tx): State<UnboundedSender<(String, String)>>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), InternalError> {
    info!(
        "user {} attempting to upload crate using token {}",
        user.username,
        token.label()
    );
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
    match db.get_crate(&crate_name)? {
        Some(mut entry) => {
            // If it already exists, add a new version to the entry

            // Make sure we don't publish new versions of existing upstream crates
            if !entry.is_local {
                return create_error(
                    "attempted to upload crate with the same name as a cached upstream crate",
                );
            }

            // Check that it is valid to upload this version
            let new_version = Version::parse(&metadata.vers)?;
            let mut is_older_than_latest = false;
            for version in &entry.versions {
                let existing_version = Version::parse(&version.pkg.vers)?;
                if new_version == existing_version {
                    return create_error("attempted to upload existing version");
                }
                if new_version < existing_version {
                    is_older_than_latest = true;
                }
            }

            // Add this version
            entry.versions.push(UploadedPackage {
                pkg: metadata.to_package(cksum),
                upload_meta: Some(metadata),
                upload_timestamp: Some(chrono::Utc::now()),
            });
            if is_older_than_latest {
                entry.versions.sort_unstable_by_key(|version| {
                    Version::parse(&version.pkg.vers)
                        .expect("all existing versions have valid identifiers")
                })
            }
            entry.time_of_last_update = chrono::Utc::now();
            db.insert_crate(&crate_name, &entry)?;
        }
        None => {
            // If it doesn't exist, create a new entry
            let entry = Entry {
                versions: vec![UploadedPackage {
                    pkg: metadata.to_package(cksum),
                    upload_meta: Some(metadata),
                    upload_timestamp: Some(chrono::Utc::now()),
                }],
                time_of_last_update: chrono::Utc::now(),
                is_local: true,
            };
            db.insert_crate(&crate_name, &entry)?;
        }
    }

    // Store crate file
    let cache_path = crate_path(state.data_dir, &crate_name, &crate_version);
    let parent = cache_path
        .parent()
        .ok_or_else(|| anyhow!("invalid cache path"))?;
    if !parent.exists() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = File::create(cache_path).await?;
    file.write_all(&data).await?;

    // Notify the background thread to build the docs for this crate
    docs_queue_tx.send((crate_name, crate_version))?;

    Ok((StatusCode::OK, Json(json!({}))))
}

async fn yank_crate(
    ApiAuth(token, user): ApiAuth,
    State(db): State<crate::Db>,
    Path((crate_name, version)): Path<(String, String)>,
) -> Result<(StatusCode, Json<Value>), InternalError> {
    info!(
        "user {} attempting to yank crate {}@{} using token {}",
        user.username,
        crate_name,
        version,
        token.label()
    );

    // Check the user supplied a valid semver version
    let Ok(yank_version) = Version::parse(&version) else {
        return create_error("invalid crate version supplied");
    };

    // Get the crate
    let Some(mut entry) = db.get_crate(&crate_name)? else {
        return create_error("crate does not exist in index");
    };

    // Find the package to yank
    let Some(package) = entry.versions.iter_mut().find(|version| {
        Version::parse(&version.pkg.vers).expect("all existing versions have valid identifiers")
            == yank_version
    }) else {
        return create_error("crate does not have the specified version published");
    };

    if package.pkg.yanked {
        return create_error("version has already been yanked");
    }

    package.pkg.yanked = true;

    // Reinsert the crate into the database
    db.insert_crate(&crate_name, &entry)?;

    Ok((StatusCode::OK, Json(json!({"ok": true}))))
}

async fn unyank_crate(
    ApiAuth(token, user): ApiAuth,
    State(db): State<crate::Db>,
    Path((crate_name, version)): Path<(String, String)>,
) -> Result<(StatusCode, Json<Value>), InternalError> {
    info!(
        "user {} attempting to unyank crate {}@{} using token {}",
        user.username,
        crate_name,
        version,
        token.label()
    );

    // Check the user supplied a valid semver version
    let Ok(yank_version) = Version::parse(&version) else {
        return create_error("invalid crate version supplied");
    };

    // Get the crate
    let Some(mut entry) = db.get_crate(&crate_name)? else {
        return create_error("crate does not exist in index");
    };

    // Find the package to unyank
    let Some(package) = entry.versions.iter_mut().find(|version| {
        Version::parse(&version.pkg.vers).expect("all existing versions have valid identifiers")
            == yank_version
    }) else {
        return create_error("crate does not have the specified version published");
    };

    if !package.pkg.yanked {
        return create_error("version has not been yanked");
    }

    package.pkg.yanked = false;

    // Reinsert the crate into the database
    db.insert_crate(&crate_name, &entry)?;

    Ok((StatusCode::OK, Json(json!({"ok": true}))))
}
