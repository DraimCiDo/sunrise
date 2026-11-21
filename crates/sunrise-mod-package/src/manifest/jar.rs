//! stable-line: content-declarations
mod cache;
mod requirements;

pub use cache::{JarConflict, JarInputsError, MissingJar, collect_jar_requirements, missing_jars};
pub use requirements::JarRequirement;

use super::valid_mod_id;
use serde::{Deserialize, Serialize};

/// An original Java jar used as a passive asset/data layer, in manifest order.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "Fields")]
pub struct JarRef {
    id: String,
    version: String,
    sha512: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    id: String,
    version: String,
    sha512: String,
}

impl TryFrom<Fields> for JarRef {
    type Error = String;

    fn try_from(fields: Fields) -> Result<Self, Self::Error> {
        Self::new(fields.id, fields.version, fields.sha512)
    }
}

impl JarRef {
    /// Validate the supported loader ID syntax and the exact file's SHA-512.
    /// The metadata reader, not this loader-independent manifest, checks the
    /// original loader's more specific ID rule. IDs are never normalized.
    pub fn new(id: String, version: String, sha512: String) -> Result<Self, String> {
        let bytes = id.as_bytes();
        let valid_id = (2..=64).contains(&bytes.len())
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(b))
            && (!bytes.contains(&b'-') || bytes[0].is_ascii_lowercase());
        if !valid_id {
            return Err(format!(
                "jar id {id:?} is not a supported NeoForge or Fabric ID"
            ));
        }
        if version.is_empty() {
            return Err(format!("jar {id}: version is empty"));
        }
        super::hash::sha512_hex(&sha512).map_err(|e| format!("jar {id}: sha512 {e}"))?;
        Ok(Self {
            id,
            version,
            sha512,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn sha512(&self) -> &str {
        &self.sha512
    }
}

#[cfg(test)]
#[path = "jar_tests.rs"]
mod tests;
