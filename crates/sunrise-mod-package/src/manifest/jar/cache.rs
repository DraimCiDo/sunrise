//! stable-line: no-content-declarations
//! Look up original files in the launcher-prepared jar cache.

use super::{JarRef, JarRequirement};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

/// One missing original file, shared by launcher preparation and game refusals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingJar {
    pub owners: Vec<String>,
    pub jar: JarRef,
    pub path: PathBuf,
}

impl fmt::Display for MissingJar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} {}: {}",
            self.owners.join(", "),
            self.jar.id(),
            self.jar.version(),
            self.path.display()
        )
    }
}

impl std::error::Error for MissingJar {}

/// An accepted resource root's original-jar inputs could not be checked.
#[derive(Debug)]
#[non_exhaustive]
pub enum JarInputsError {
    /// An exact original is unavailable, with every requiring mod retained.
    Missing(Box<MissingJar>),
    /// The accepted inputs or cache metadata could not be read.
    Io {
        /// The input file or cache directory being checked.
        path: PathBuf,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// The accepted input record failed schema validation.
    Invalid {
        /// The accepted input record.
        path: PathBuf,
        /// The decoding or validation failure.
        source: serde_json::Error,
    },
}

impl fmt::Display for JarInputsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(missing) => missing.fmt(f),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Invalid { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for JarInputsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Missing(missing) => missing.as_ref(),
            Self::Io { source, .. } => source,
            Self::Invalid { source, .. } => source,
        })
    }
}

/// A hash cannot name two different original Java mod identities.
#[derive(Debug)]
pub struct JarConflict {
    pub first: Box<JarRef>,
    pub second: Box<JarRef>,
}

impl fmt::Display for JarConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} {} != {} {}",
            self.first.sha512(),
            self.first.id(),
            self.first.version(),
            self.second.id(),
            self.second.version()
        )
    }
}

impl std::error::Error for JarConflict {}

/// Collect exact files once, preserving all owning mod ids in input load order.
/// The returned hash order is for preparation only; pack layers stay per mod.
pub fn collect_jar_requirements<'a>(
    mods: impl IntoIterator<Item = (&'a str, &'a [JarRef])>,
) -> Result<Vec<JarRequirement>, JarConflict> {
    let mut hashes: BTreeMap<&str, JarRequirement> = BTreeMap::new();
    for (owner, jars) in mods {
        for jar in jars {
            if let Some(first) = hashes.get_mut(jar.sha512()) {
                if first.jar != *jar {
                    return Err(JarConflict {
                        first: Box::new(first.jar.clone()),
                        second: Box::new(jar.clone()),
                    });
                }
                if !first.owners.iter().any(|id| id == owner) {
                    first.owners.push(owner.to_owned());
                }
            } else {
                hashes.insert(
                    jar.sha512(),
                    JarRequirement {
                        owners: vec![owner.to_owned()],
                        jar: jar.clone(),
                    },
                );
            }
        }
    }
    Ok(hashes.into_values().collect())
}

/// Missing exact files in a jar cache, with every requiring mod retained.
/// Byte identity is verified by launcher preparation, not rehashed on a reload.
pub fn missing_jars(
    store: &Path,
    requirements: &[JarRequirement],
) -> std::io::Result<Vec<MissingJar>> {
    let mut missing = Vec::new();
    for requirement in requirements {
        let path = requirement.jar.cache_path(store);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    path.display().to_string(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(MissingJar {
                    owners: requirement.owners.clone(),
                    jar: requirement.jar.clone(),
                    path,
                })
            }
            Err(error) => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("{}: {error}", path.display()),
                ));
            }
        }
    }
    Ok(missing)
}

impl JarRef {
    /// The exact file's path in the supplied jar-cache directory.
    pub fn cache_path(&self, store: &Path) -> PathBuf {
        store.join(format!("{}.jar", self.sha512))
    }
}
