use anyhow::Context;
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tracing::info;

use crate::{
    config::Config,
    mirror,
    package::{Package, UploadedPackage},
    AppState, Entry, InternalError,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config.json", get(index_config))
        .route("/1/:crate_name", get(crate_metadata))
        .route("/2/:crate_name", get(crate_metadata))
        .route("/:a/:b/:crate_name", get(crate_metadata))
}

async fn index_config(State(config): State<Config>) -> Json<Value> {
    Json(json!({ "dl": config.external_url.clone() + "/crates", "api": config.external_url }))
}

async fn crate_metadata(
    Path(parts): Path<Vec<String>>,
    State(db): State<crate::Db>,
    State(config): State<Config>,
) -> Result<(StatusCode, String), InternalError> {
    let crate_name = parts.last().expect("invalid route to crate_metadata");
    info!(crate = crate_name, "pulling crate metadata");

    if let Some(entry) = db.get_crate(crate_name)? {
        let has_expired =
            chrono::Utc::now() - entry.time_of_last_update > chrono::Duration::minutes(30);
        if config.offline || entry.is_local || !has_expired {
            info!(crate = crate_name, "returning metadata from cache");
            return entry
                .versions
                .into_iter()
                .map(|version| serde_json::to_string(&version.pkg))
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
            db.remove_crate(crate_name)?;
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
    let versions: Vec<UploadedPackage> = upstream
        .lines()
        // Deserialise each version from upstream
        .map(serde_json::from_str)
        // Add null upload timestamps
        .map(|pkg: Result<Package, _>| {
            pkg.map(|pkg| UploadedPackage {
                pkg,
                upload_meta: None,
                upload_timestamp: None,
            })
        })
        .collect::<Result<_, _>>()
        .with_context(|| "could not parse upstream json metadata")?;

    let entry = Entry {
        versions,
        time_of_last_update: chrono::Utc::now(),
        is_local: false,
    };

    // Insert binary representation into database
    db.insert_crate(crate_name, entry)?;

    Ok((StatusCode::OK, upstream))
}
