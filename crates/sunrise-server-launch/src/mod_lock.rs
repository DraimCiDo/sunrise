//! The on-disk managed mod selection and its validation boundary.
use crate::config::Mods;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use sunrise_mod_package::manifest::{revision, valid_mod_id, version_from_str};

pub const LOCK: &str = "mods.lock";
pub const MAX_LOCK: u64 = 1 << 20;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModLock {
    format: u32,
    pub sunrise: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    pub input: Mods,
    #[serde(default)]
    pub mods: Vec<LockedMod>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedMod {
    pub id: String,
    #[serde(deserialize_with = "version_from_str")]
    pub version: semver::Version,
    pub sha256: String,
    url: url::Url,
    pub file: PathBuf,
    pub reason: Reason,
    #[serde(default)]
    pub required_by: Vec<String>,
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<Native>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Requested,
    Pack,
    Required,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Native {
    Prebuilt(String),
    Built(String),
}

pub fn release_tag(tag: &str) -> bool {
    tag.as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ModLock {
    /// Read and validate the lock in the canonical server folder `root`.
    pub fn read(root: &Path) -> Result<Self, String> {
        let path = local_file(root, Path::new(LOCK))?;
        let text = read_text(&path, MAX_LOCK)?;
        let lock: Self = toml::from_str(&text).map_err(|e| format!("{LOCK}: {e}"))?;
        if lock.format != 1 {
            return Err(format!("{LOCK} format {} is not supported", lock.format));
        }
        if !revision(&lock.sunrise) {
            return Err(format!("{LOCK}: sunrise must be a 40-hex engine build"));
        }
        if lock.release.as_deref().is_some_and(|tag| !release_tag(tag)) {
            return Err(format!("{LOCK}: release must be a Sunrise release tag"));
        }
        lock.input
            .check()
            .map_err(|e| format!("{LOCK} input: {e}"))?;
        let (mut ids, mut paths) = (BTreeSet::new(), BTreeSet::new());
        for row in &lock.mods {
            let invalid = |field| format!("{LOCK}: mod {:?} has an invalid {field}", row.id);
            if !valid_mod_id(&row.id) || !ids.insert(&row.id) {
                return Err(invalid("or duplicate ID"));
            }
            if !sha256(&row.sha256) {
                return Err(invalid("SHA-256"));
            }
            if row.url.scheme() != "https" {
                return Err(invalid("source URL (expected HTTPS)"));
            }
            // Keep the lock portable, including on hosts where `\\` and `:` are not separators.
            if row.file.as_os_str().is_empty()
                || row
                    .file
                    .as_os_str()
                    .to_string_lossy()
                    .contains(['\\', ':', '\0'])
                || !row
                    .file
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                || !paths.insert(&row.file)
            {
                return Err(invalid("or duplicate relative file path"));
            }
            if matches!(row.reason, Reason::Required) == row.required_by.is_empty()
                || row.required_by.iter().any(|id| !valid_mod_id(id))
            {
                return Err(invalid("required_by list for its reason"));
            }
            if let Some(native) = &row.native {
                match native {
                    Native::Prebuilt(hash) if !sha256(hash) => {
                        return Err(invalid("native descriptor SHA-256"));
                    }
                    Native::Built(commit) if !revision(commit) => {
                        return Err(invalid("native SDK commit"));
                    }
                    _ => {}
                }
            }
        }
        Ok(lock)
    }
}

/// Call only with a validated relative path. Resolve links before checking containment, including
/// links in intermediate directories, and don't let a lock direct reads to a device or pipe.
pub fn local_file(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let actual =
        fs::canonicalize(root.join(path)).map_err(|e| format!("{}: {e}", path.display()))?;
    if !actual.starts_with(root) {
        return Err(format!(
            "{} points outside the server folder",
            path.display()
        ));
    }
    if !actual.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok(actual)
}

/// Read a bounded UTF-8 record, refusing a valid prefix of an oversized file.
pub fn read_text(path: &Path, limit: u64) -> Result<String, String> {
    let error = |e| format!("{}: {e}", path.display());
    let mut text = String::new();
    let bytes = fs::File::open(path)
        .map_err(error)?
        .take(limit.saturating_add(1))
        .read_to_string(&mut text)
        .map_err(error)?;
    if bytes as u64 > limit {
        return Err(format!("{} exceeds the {limit}-byte limit", path.display()));
    }
    Ok(text)
}
