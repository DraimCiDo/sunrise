//! stable-line: content-declarations
//! A package's runtime data files: the `runtime/` folder and the `[runtime]` table that lists
//! each file with its SHA-512.
//!
//! Runtime files are the bytes a mod's code opens beside itself (a compiled script bundle, a baked
//! lookup table). They are not pack data: no datapack or resource pack can replace them and no
//! reload touches them.
mod data;

use super::hash::sha512_hex;
pub use data::{MAX_RUNTIME_FILE, MAX_RUNTIME_TOTAL, RUNTIME_DIR, RuntimeData, sha512_of};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The most files a package's `[runtime]` may list.
pub const MAX_RUNTIME_FILES: usize = 256;

/// A `[runtime]` table: each file's name under `runtime/` and its SHA-512.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "BTreeMap<String, String>")]
pub struct RuntimeTable(BTreeMap<String, String>);

impl TryFrom<BTreeMap<String, String>> for RuntimeTable {
    type Error = String;

    fn try_from(files: BTreeMap<String, String>) -> Result<Self, Self::Error> {
        if files.is_empty() {
            return Err("[runtime] lists no file".into());
        }
        if files.len() > MAX_RUNTIME_FILES {
            return Err(format!(
                "[runtime] lists {} files, over the {MAX_RUNTIME_FILES}-file runtime cap",
                files.len()
            ));
        }
        for (name, hash) in &files {
            if !super::plain_path(name) {
                return Err(format!("[runtime] {name}: not a plain relative path"));
            }
            sha512_hex(hash).map_err(|e| format!("[runtime] {name} {e}"))?;
        }
        Ok(Self(files))
    }
}

impl RuntimeTable {
    /// Each listed file's name under `runtime/` and its SHA-512, in name order.
    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, hash)| (name.as_str(), hash.as_str()))
    }

    /// Whether the table lists `name`.
    pub fn lists(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
}
