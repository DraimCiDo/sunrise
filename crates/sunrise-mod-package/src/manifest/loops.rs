//! stable-line: decodes-other-data
//! `[[native.loop]]`: a native mod's type in the slot of a hot loop the game publishes. Like a
//! mixin, its helper is game code: compiled into the launcher's patch crate beside the loop, it
//! names the game's own types, pinned to the exact build. The launcher compiles one copy of the
//! loop with the installed mods' slots and installs it through the loop's entry function.
use serde::{Deserialize, Serialize};
use std::fmt;

/// The priority of a row that names none; lower runs first, ties by mod id.
pub const DEFAULT_PRIORITY: i32 = 1000;

/// One `[[native.loop]]` row.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub id: String,
    /// The published loop, as the game's `loops.json` names it (`block_entities.tick`).
    #[serde(rename = "loop")]
    pub name: String,
    /// The helper's source file in this package: Rust items compiled in the loop's module.
    pub helper: String,
    /// The slot type the helper defines: it implements the loop's slot trait and
    /// `Default + 'static`.
    pub with: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    DEFAULT_PRIORITY
}

/// Why a package's loop rows are refused before anything is built; `id` is `package:row`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoopError {
    /// `E_LOOP_INVALID`: an empty `loop`, a `helper` that is not a Rust file under `src/`, or a
    /// `with` that is not an identifier.
    Invalid {
        id: String,
        field: &'static str,
        fix: &'static str,
    },
}

impl fmt::Display for LoopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { id, field, fix } => {
                write!(f, "E_LOOP_INVALID: {id}: invalid {field}; {fix}")
            }
        }
    }
}
impl std::error::Error for LoopError {}

impl Loop {
    pub(super) fn check_all(entries: &[Self], mod_id: &str) -> Result<(), LoopError> {
        for entry in entries {
            let id = format!("{mod_id}:{}", entry.id);
            if entry.name.trim().is_empty() {
                return Err(LoopError::Invalid {
                    id,
                    field: "loop",
                    fix: "name a published loop of the game, such as block_entities.tick",
                });
            }
            if !super::plain_path(&entry.helper)
                || !entry.helper.starts_with("src/")
                || !entry.helper.ends_with(".rs")
            {
                return Err(LoopError::Invalid {
                    id,
                    field: "helper",
                    fix: "name a Rust file under src/ in this package",
                });
            }
            if !super::rust_ident(&entry.with) {
                return Err(LoopError::Invalid {
                    id,
                    field: "with",
                    fix: "name the slot type the helper defines",
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "loops_tests.rs"]
mod tests;
