use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenEntry {
    username: String,
    label: String,
}

impl TokenEntry {
    pub fn username(&self) -> &str {
        self.username.as_ref()
    }

    pub fn label(&self) -> &str {
        self.label.as_ref()
    }
}

/// Create a new token for the user.
///
/// Returns the token to be supplied back to the user.
pub fn create_token(
    db: &db::Db,
    username: &str,
    label: &str,
) -> Result<Option<String>, anyhow::Error> {
    // Check if user already has a token with this label
    if get_user_tokens(db, username)?.contains(&TokenEntry {
        username: username.to_owned(),
        label: label.to_owned(),
    }) {
        return Ok(None);
    }

    let mut token = [0u8; 32];
    OsRng.fill_bytes(&mut token);
    let hashed_token = Sha256::digest(token);

    db.insert_token(
        &hashed_token,
        &TokenEntry {
            username: username.to_owned(),
            label: label.to_owned(),
        },
    )?;
    Ok(Some(bs58::encode(token).into_string()))
}

pub fn lookup_token(db: &db::Db, token: &str) -> Result<Option<crate::auth::User>, anyhow::Error> {
    let hashed_token = Sha256::digest(bs58::decode(token).into_vec()?);
    db.get_token_user(&hashed_token)
}

pub fn get_user_tokens(db: &db::Db, username: &str) -> Result<Vec<TokenEntry>, anyhow::Error> {
    db.iter_tokens()
        .filter_map(|elem| elem.ok())
        .map(|(_, value)| bincode::deserialize::<TokenEntry>(&value).map_err(|e| e.into()))
        .filter(|entry| {
            entry
                .as_ref()
                .map_or(false, |entry| entry.username() == username)
        })
        .collect()
}

pub fn delete(db: &db::Db, username: &str, label: &str) -> Result<(), anyhow::Error> {
    let reference = TokenEntry {
        username: username.to_owned(),
        label: label.to_owned(),
    };

    // Find the tokens matching the username & label
    let tokens = db
        .iter_tokens()
        .filter_map(|elem| elem.ok())
        .filter_map(|(token, value)| {
            bincode::deserialize::<TokenEntry>(&value)
                .ok()
                .map(|elem| (token, elem))
        })
        .filter_map(|(token, entry)| (entry == reference).then_some(token))
        .collect::<Vec<_>>();

    // Delete the tokens
    for token in tokens {
        db.delete_token(&token)?;
    }

    Ok(())
}
