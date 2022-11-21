use anyhow::anyhow;
use axum::body::Bytes;
use reqwest::StatusCode;

static CRATES_IO_INDEX: &str = "https://index.crates.io";
static CRATES_IO_INDEX_DL: &str = "https://crates.io/api/v1/crates";

pub async fn get_package(name: &str) -> Result<Option<String>, anyhow::Error> {
    tracing::info!("checking {name} in crates.io index");
    let prefix = crate_prefix(name);
    let url = format!("{}/{}/{}", CRATES_IO_INDEX, prefix, name);

    let response = reqwest::get(url)
        .await?
        .error_for_status()
        .map(Some)
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
    let url = format!("{}/{}/{}/download", CRATES_IO_INDEX_DL, name, version);

    let response = reqwest::get(url)
        .await?
        .error_for_status()
        .map(Some)
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
