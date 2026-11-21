//! stable-line: no-content-declarations
//! `[[native.mixin]]`: a source-level edit of a game function, the only form that compiles game code.
//! Each row is one horizon-mixins `[[injection]]`, forwarded to the pinned tool, which owns the
//! vocabulary beyond the keys named here. Native only.
use serde::{Deserialize, Serialize};
use std::fmt;

/// One `[[native.mixin]]` row. `options` holds every other key of the tool's `[[injection]]`
/// (`selector`, `ordinal`, `bindings`, `cancellable`, `require`, `slice`, `variable`...), which
/// the tool validates when the game is built.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Mixin {
    pub id: String,
    /// The game function's path, as the pinned source spells it.
    pub target: String,
    pub kind: Kind,
    /// The helper's source file in this package, compiled inside the target function.
    pub helper: String,
    #[serde(flatten)]
    pub options: toml::Table,
}

/// A horizon-mixins table forwarded whole (`[[native.bridge]]`, `[[native.state]]`); only its
/// id is the package's to check.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Forwarded {
    pub id: String,
    #[serde(flatten)]
    pub fields: toml::Table,
}

/// horizon-mixins' injection kinds, spelled in upper case in the manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Head,
    Return,
    BeforeCall,
    AfterCall,
    AfterAssign,
    ModifyArg,
    ModifyArgs,
    ModifyConstant,
    ModifyVariable,
    ModifyExpressionValue,
    WrapOperation,
    Overwrite,
}

impl Kind {
    pub const ALL: [Self; 12] = [
        Self::Head,
        Self::Return,
        Self::BeforeCall,
        Self::AfterCall,
        Self::AfterAssign,
        Self::ModifyArg,
        Self::ModifyArgs,
        Self::ModifyConstant,
        Self::ModifyVariable,
        Self::ModifyExpressionValue,
        Self::WrapOperation,
        Self::Overwrite,
    ];

    /// The manifest spelling, which is the tool's.
    pub fn name(self) -> &'static str {
        match self {
            Self::Head => "HEAD",
            Self::Return => "RETURN",
            Self::BeforeCall => "BEFORE_CALL",
            Self::AfterCall => "AFTER_CALL",
            Self::AfterAssign => "AFTER_ASSIGN",
            Self::ModifyArg => "MODIFY_ARG",
            Self::ModifyArgs => "MODIFY_ARGS",
            Self::ModifyConstant => "MODIFY_CONSTANT",
            Self::ModifyVariable => "MODIFY_VARIABLE",
            Self::ModifyExpressionValue => "MODIFY_EXPRESSION_VALUE",
            Self::WrapOperation => "WRAP_OPERATION",
            Self::Overwrite => "OVERWRITE",
        }
    }
}

impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = std::borrow::Cow::<str>::deserialize(deserializer)?;
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == name)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "unknown mixin kind `{name}`, expected one of {}",
                    Self::ALL.map(Self::name).join(", ")
                ))
            })
    }
}

/// Why a package's mixins are refused before any tool runs; `id` is `package:mixin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MixinError {
    Invalid {
        id: String,
        field: &'static str,
        fix: &'static str,
    },
}

impl fmt::Display for MixinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { id, field, fix } => {
                write!(f, "E_MIXIN_INVALID: {id}: invalid {field}; {fix}")
            }
        }
    }
}
impl std::error::Error for MixinError {}

impl Mixin {
    pub(super) fn check_all(entries: &[Self], mod_id: &str) -> Result<(), MixinError> {
        for entry in entries {
            let id = format!("{mod_id}:{}", entry.id);
            let invalid = |field, fix| MixinError::Invalid {
                id: id.clone(),
                field,
                fix,
            };
            if entry.target.trim().is_empty() {
                return Err(invalid("target", "name the game function's path"));
            }
            if !super::plain_path(&entry.helper)
                || !entry.helper.starts_with("src/")
                || !entry.helper.ends_with(".rs")
            {
                return Err(invalid(
                    "helper",
                    "name a Rust file under src/ in this package",
                ));
            }
            if entry.options.contains_key("hook") || entry.options.contains_key("hook_version") {
                return Err(invalid(
                    "hook",
                    "a named hook is a mod_api::Hooks method; a mixin names its target",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "mixin_tests.rs"]
mod tests;
