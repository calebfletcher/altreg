use std::path::Path;

use anyhow::{anyhow, Context};
use tracing::warn;

use crate::{auth, token::TokenEntry, Entry};

const DB_VERSION: u32 = 2;
static DB_VERSION_KEY: &str = "version";

#[derive(Debug, Clone)]
pub struct Db {
    crate_tree: sled::Tree,
    user_tree: sled::Tree,
    token_tree: sled::Tree,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let db = sled::open(path).with_context(|| "unable to open database")?;

        match db.get(DB_VERSION_KEY)? {
            Some(version_bytes) => {
                let version: u32 = bincode::deserialize(&version_bytes)
                    .with_context(|| "could not deserialise database version")?;

                if version > DB_VERSION {
                    return Err(anyhow!(
                        "database was created in a newer version of the registry (db version {version})"
                    ));
                }
                if version < DB_VERSION {
                    warn!("database was created in an older version of the registry (db version {version})");
                    db.insert(DB_VERSION_KEY, bincode::serialize(&DB_VERSION)?)
                        .with_context(|| "could not update database version in database")?;
                    // TODO: Database migrations
                }
            }
            None => {
                // Database was empty
                db.insert(DB_VERSION_KEY, bincode::serialize(&DB_VERSION)?)
                    .with_context(|| "could not set database version in database")?;
            }
        }

        let crate_tree = db.open_tree("crates")?;
        let user_tree = db.open_tree("users")?;
        let token_tree = db.open_tree("tokens")?;

        Ok(Db {
            crate_tree,
            user_tree,
            token_tree,
        })
    }

    pub fn get_crate(&self, crate_name: &str) -> Result<Option<Entry>, anyhow::Error> {
        self.crate_tree
            .get(crate_name)
            .with_context(|| "could not access crate entry")?
            .map(|raw| bincode::deserialize(&raw))
            .transpose()
            .with_context(|| "could not deserialise metadata in crate entry")
    }

    pub fn remove_crate(&self, crate_name: &str) -> Result<(), anyhow::Error> {
        self.crate_tree
            .remove(crate_name)
            .with_context(|| "could not remove crate")
            .map(|_| ())
    }

    pub fn insert_crate(&self, crate_name: &str, entry: &Entry) -> Result<(), anyhow::Error> {
        self.crate_tree
            .insert(
                crate_name,
                bincode::serialize(entry).with_context(|| "could not serialise crate entry")?,
            )
            .with_context(|| "could not insert crate")
            .map(|_| ())
    }

    /// Modify a crate atomically.
    ///
    /// This function will call a function `f` (potentially multiple times during contention) with the result that
    /// the change the function makes is applied atomically. If the function returns an error, then the old value is
    /// preserved.
    pub fn modify_crate(
        &self,
        crate_name: &str,
        mut f: impl FnMut(&mut Entry) -> Result<(), anyhow::Error>,
    ) -> Result<(), anyhow::Error> {
        let mut err: Option<anyhow::Error> = None;

        self.crate_tree
            .update_and_fetch(crate_name, |old| match old {
                Some(old) => {
                    // Deserialize the entry
                    let mut entry = bincode::deserialize(old)
                        .expect("existing entries should be deserializable");

                    // Call the user's function
                    if let Err(e) = f(&mut entry) {
                        err = Some(e);
                        return Some(old.to_vec());
                    }

                    // Serialize the entry
                    let entry = match bincode::serialize(&entry) {
                        Ok(entry) => entry,
                        Err(e) => {
                            err = Some(e.into());
                            return Some(old.to_vec());
                        }
                    };
                    Some(entry)
                }
                None => None,
            })?;

        // Return the error if there was one
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub fn iter_crates(&self) -> impl Iterator<Item = (String, Entry)> {
        self.crate_tree
            .iter()
            .filter_map(|elem| elem.ok())
            .map(|(name, entry)| {
                (
                    String::from_utf8_lossy(&name).to_string(),
                    bincode::deserialize(&entry).expect("crate entries be valid to deserialize"),
                )
            })
    }

    pub fn get_user(&self, username: &str) -> Result<Option<auth::User>, anyhow::Error> {
        self.user_tree
            .get(username)
            .with_context(|| "could not access user entry")?
            .map(|raw| bincode::deserialize(&raw))
            .transpose()
            .with_context(|| "could not deserialise user entry")
    }

    pub fn insert_user(&self, username: &str, user: &auth::User) -> Result<(), anyhow::Error> {
        self.user_tree
            .insert(
                username,
                bincode::serialize(user).with_context(|| "could not serialise user entry")?,
            )
            .with_context(|| "could not insert user")
            .map(|_| ())
    }

    pub fn iter_users(&self) -> sled::Iter {
        self.user_tree.iter()
    }

    pub fn get_token_user(
        &self,
        token: &[u8],
    ) -> Result<Option<(TokenEntry, auth::User)>, anyhow::Error> {
        self.token_tree
            .get(token)
            .with_context(|| "could not access token entry")?
            .map(|raw| bincode::deserialize::<TokenEntry>(&raw))
            .transpose()
            .with_context(|| "could not deserialise token entry")
            .and_then(|entry| {
                Ok(entry
                    .map(|entry| {
                        self.get_user(entry.username())
                            .map(|user| user.map(|user| (entry, user)))
                    })
                    .transpose()
                    .with_context(|| "could not get user entry")?
                    .flatten())
            })
    }

    pub fn insert_token(&self, token: &[u8], entry: &TokenEntry) -> Result<(), anyhow::Error> {
        self.token_tree
            .insert(
                token,
                bincode::serialize(entry).with_context(|| "could not serialise token entry")?,
            )
            .with_context(|| "could not insert token")
            .map(|_| ())
    }

    pub fn iter_tokens(&self) -> sled::Iter {
        self.token_tree.iter()
    }

    pub fn delete_token(&self, token: &[u8]) -> Result<(), anyhow::Error> {
        self.token_tree.remove(token)?;
        Ok(())
    }
}
