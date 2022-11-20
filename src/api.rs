use anyhow::{anyhow, Context};
use axum::{body::Bytes, routing::put, Extension, Json, Router};
use reqwest::StatusCode;
use semver::Version;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{fs::File, io::AsyncWriteExt};

use crate::{config::Config, crate_path, package, Entry, InternalError};

pub fn router() -> Router {
    Router::new().route("/v1/crates/new", put(add_crate))
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

fn create_error(msg: &str) -> Result<(StatusCode, Json<Value>), InternalError> {
    Ok((
        StatusCode::BAD_REQUEST,
        Json(json!({ "errors": [{"detail": msg}]})),
    ))
}
