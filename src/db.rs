use std::path::Path;

use anyhow::{anyhow, Context};
use tracing::warn;

use crate::Entry;

const DB_VERSION: u32 = 2;
static DB_VERSION_KEY: &str = "version";

#[derive(Debug, Clone)]
pub struct Db {
    crate_tree: sled::Tree,
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

        Ok(Db { crate_tree })
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

    pub fn iter_crates(&self) -> sled::Iter {
        self.crate_tree.iter()
    }
}
