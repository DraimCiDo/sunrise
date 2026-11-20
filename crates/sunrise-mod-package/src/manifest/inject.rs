//! `[[inject]]` declarations and their compiled slots, independent of catalog I/O. An inject
//! never compiles game code: a place with no slot is refused, and [`super::mixin`] is the form
//! that edits source.
use serde::{Deserialize, Serialize};
use std::fmt;

const RESERVED: &str = concat!(
    "_ as async await break const continue crate dyn else enum extern false fn for if ",
    "impl in let loop match mod move mut pub ref return self Self static struct super ",
    "trait true type unsafe use where while abstract become box do final gen macro ",
    "override priv try typeof unsized virtual yield"
);

/// A listener at a compiled slot. Freshness fields are written by the inject tool.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Inject {
    pub id: String,
    pub at: String,
    pub kind: Kind,
    pub with: String,
    #[serde(default)]
    pub priority: i32,
    /// Optional pinning: the function's catalog fingerprint `inject add` writes. Absent, the
    /// build takes it from the pinned game's catalog, which `[native] sunrise` fixes exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// What a listener does at its place, spelled in snake case in the manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Head,
    Return,
    Replace,
}

impl Kind {
    pub const ALL: [Self; 3] = [Self::Head, Self::Return, Self::Replace];

    /// The manifest spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Return => "return",
            Self::Replace => "replace",
        }
    }
}

impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// A removed spelling is refused naming the form that replaced it; it is never read as that form.
/// The source-level kinds name the `[[native.mixin]]` form that compiles them.
impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = std::borrow::Cow::<str>::deserialize(deserializer)?;
        if let Some(kind) = Self::ALL.into_iter().find(|kind| kind.name() == name) {
            return Ok(kind);
        }
        Err(serde::de::Error::custom(match &*name {
            "modify_arg" => {
                "kind `modify_arg` was removed: use `head`, whose listener already gets \
                             every argument to change"
                    .to_owned()
            }
            "modify_call_arg" | "before_call" | "after_call" | "modify_value" | "wrap_call"
            | "wrap_if" => source_level(&name, "`[[native.mixin]]` function replacement"),
            "modify_local" => source_level(&name, "`[[native.mixin]]` kind `MODIFY_VARIABLE`"),
            "expression" => source_level(
                &name,
                "`[[native.mixin]]` kind `MODIFY_CONSTANT` or `MODIFY_EXPRESSION_VALUE`",
            ),
            "field" => source_level(&name, "an attachment for per-object mod data"),
            other => format!(
                "unknown kind `{other}`, expected one of {}",
                Self::ALL.map(Self::name).join(", ")
            ),
        }))
    }
}

fn source_level(kind: &str, form: &str) -> String {
    format!(
        "E_INJECT_NO_SLOT: kind `{kind}` has no compiled slot and `[[inject]]` never compiles \
         game code: use {form} in a native mod"
    )
}

/// The manifest's `[[inject]]` rows. A row's kind is read before its other keys, so a removed
/// kind is refused by name even when keys that went with it follow. The removed `access` key is
/// refused by name too: an injection is always through the API.
pub(super) fn rows<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Inject>, D::Error> {
    Vec::<toml::Table>::deserialize(deserializer)?
        .into_iter()
        .map(row)
        .collect::<Result<_, toml::de::Error>>()
        .map_err(serde::de::Error::custom)
}

/// One `[[inject]]` row: the one place a row is read, so a removed kind or key gets its own
/// refusal and never serde's "unknown field".
pub(super) fn row(row: toml::Table) -> Result<Inject, toml::de::Error> {
    if let Some(kind) = row.get("kind") {
        Kind::deserialize(kind.clone())?;
    }
    if let Some(access) = row.get("access") {
        return Err(serde::de::Error::custom(
            if access.as_str() == Some("internals") {
                "E_INJECT_INVALID: field `access`: `internals` injections were removed: \
                 use `[[native.mixin]]` in a native mod"
            } else {
                "E_INJECT_INVALID: field `access` was removed, an injection is always \
                 through the API: remove this line"
            },
        ));
    }
    row.try_into()
}

impl Inject {
    pub(super) fn check_all(entries: &[Self], mod_id: &str, code: bool) -> Result<(), InjectError> {
        let mut ids = std::collections::HashSet::new();
        for entry in entries {
            let invalid = |field, fix| InjectError::Invalid {
                id: format!("{mod_id}:{}", entry.id),
                field,
                fix,
            };
            if !code {
                return Err(invalid("inject", "declare [native]"));
            }
            if entry.id.trim().is_empty() || !ids.insert(&entry.id) {
                return Err(invalid("id", "use a nonempty, unique inject id"));
            }
            if entry.at.trim().is_empty() {
                return Err(invalid("at", "name the target function's canonical path"));
            }
            if entry
                .hash
                .as_ref()
                .is_some_and(|hash| hash.trim().is_empty())
            {
                return Err(invalid(
                    "hash",
                    "omit it, or run inject add for the pinned game",
                ));
            }
            let name = entry.with.as_bytes();
            if RESERVED.split_whitespace().any(|word| word == entry.with)
                || name.is_empty()
                || name[0].is_ascii_digit()
                || !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                return Err(invalid("with", "name a function in the mod's crate root"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoSlotReason {
    NoApiRecord,
    ReplaceWithoutValue,
}
impl fmt::Display for NoSlotReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoApiRecord => "no API record: a parameter has no API type yet",
            Self::ReplaceWithoutValue => "replace needs a return its record carries",
        })
    }
}

/// Escape-hatch refusals, never part of `mod_api`; `id` is qualified as `package:inject`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InjectError {
    Invalid {
        id: String,
        field: &'static str,
        fix: &'static str,
    },
    Missing {
        id: String,
        at: String,
    },
    Changed {
        id: String,
        at: String,
        old: String,
        new: String,
    },
    NoSlot {
        id: String,
        reason: NoSlotReason,
        declaration: String,
    },
}

impl fmt::Display for InjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::Invalid { .. } => "E_INJECT_INVALID",
            Self::Missing { .. } => "E_INJECT_MISSING",
            Self::Changed { .. } => "E_INJECT_CHANGED",
            Self::NoSlot { .. } => "E_INJECT_NO_SLOT",
        };
        write!(f, "{code}: ")?;
        match self {
            Self::Invalid { id, field, fix } => write!(f, "{id}: invalid {field}; {fix}"),
            Self::Missing { id, at } => write!(f, "{id}: {at} gone; rerun inject add"),
            Self::Changed { id, at, old, new } => {
                write!(f, "{id}: {at} {old} -> {new}; run inject update")
            }
            Self::NoSlot {
                id,
                reason,
                declaration,
            } => write!(
                f,
                "{id}: {reason}; engine API point {declaration}; use an engine function with an API record, or [[native.mixin]] in a native mod"
            ),
        }
    }
}
impl std::error::Error for InjectError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_api_function_kinds_parse() {
        for kind in Kind::ALL {
            let text = format!(
                "id='listen'\nat='game::tick'\nkind='{}'\nwith='listen'",
                kind.name()
            );
            assert_eq!(toml::from_str::<Inject>(&text).unwrap().kind, kind);
        }
        let text = "id='listen'\nat='game::tick'\nkind='head'\nwith='listen'";
        for extra in ["call='f'", "nth=0", "ctx='x'"] {
            assert!(toml::from_str::<Inject>(&format!("{text}\n{extra}")).is_err());
        }
        for kind in [
            "before_call",
            "after_call",
            "modify_value",
            "wrap_call",
            "wrap_if",
        ] {
            assert!(serde_json::from_value::<Kind>(serde_json::json!(kind)).is_err());
        }
    }
}

#[cfg(test)]
mod provenance_tests {
    #[test]
    fn removed_kind_names_mod_row_and_target() {
        refused("kind='modify_arg'", "kind `modify_arg` was removed");
    }
    #[test]
    fn removed_access_names_mod_row_and_target() {
        refused(
            "kind='head'\naccess='internals'",
            "field `access`: `internals` injections were removed: use `[[native.mixin]]`",
        );
    }
    #[test]
    fn the_removed_access_key_is_refused_even_as_api() {
        refused(
            "kind='head'\naccess='api'",
            "field `access` was removed, an injection is always through the API: remove this line",
        );
    }
    fn refused(fields: &str, reason: &str) {
        let text = format!(
            "id='addon'\nname='Addon'\nversion='1.0.0'\napi='0.2'\ncapabilities=[]\n[[inject]]\nid='before'\nat='game::tick'\nwith='before'\n{fields}\n"
        );
        let error = super::super::Manifest::parse(&text).unwrap_err();
        assert!(
            error.contains("addon:before: game::tick:") && error.contains(reason),
            "{error}"
        );
    }
}
