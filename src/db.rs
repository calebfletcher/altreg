use std::path::Path;

use anyhow::Context;

use crate::Entry;

#[derive(Debug, Clone)]
pub struct Db {
    inner: sled::Db,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let db = sled::open(path).with_context(|| "unable to open database")?;

        Ok(Db { inner: db })
    }

    pub fn get_crate(&self, crate_name: &str) -> Result<Option<Entry>, anyhow::Error> {
        self.inner
            .get(crate_name)
            .with_context(|| "could not access cache entry")?
            .map(|raw| bincode::deserialize(&raw))
            .transpose()
            .with_context(|| "could not deserialise metadata in cache entry")
    }

    pub fn remove_crate(&self, crate_name: &str) -> Result<(), anyhow::Error> {
        self.inner
            .remove(crate_name)
            .with_context(|| "could not remove entry from cache")
            .map(|_| ())
    }

    pub fn insert_crate(&self, crate_name: &str, entry: Entry) -> Result<(), anyhow::Error> {
        self.inner
            .insert(
                crate_name,
                bincode::serialize(&entry).with_context(|| "could not serialise cache entry")?,
            )
            .with_context(|| "could not insert cache entry")
            .map(|_| ())
    }

    pub fn iter_crates(&self) -> sled::Iter {
        self.inner.iter()
    }
}
