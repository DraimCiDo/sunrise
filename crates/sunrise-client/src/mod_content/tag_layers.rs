//! Tag layers collected during content-root assembly.
use super::J;

/// The mods' layers of one tag, in load order: each layer's `values` and,
/// after them, its `remove` entries (`TagLoader`: a file adds, then removes).
pub(super) struct TagLayers<'a> {
    pub(super) values: &'a [J],
    pub(super) remove: &'a [J],
}

/// What the mods do to one tag file, in load order (a data pack's
/// TagLoader): each adds its values, and one that replaces the tag first
/// drops vanilla's entries and the mods' before it.
#[derive(Default)]
pub(super) struct TagMerge {
    /// A mod replaced the tag: vanilla's entries are left out.
    pub(super) replaced: bool,
    /// The mods whose values are in `values`.
    pub(super) mods: Vec<String>,
    /// Each mod file's additions and, after them, its removals, in load
    /// order (`TagLoader`: one file adds its values, then its `remove`
    /// entries apply). A replace drops the layers below it.
    pub(super) layers: Vec<(Vec<J>, Vec<J>)>,
}

impl TagMerge {
    /// Adds one mod's tag file. A replace that drops entries (vanilla's in a
    /// `shared` tag, one not in the mod's namespaces, or other mods') warns.
    pub(super) fn add(
        &mut self,
        id: &str,
        file: &str,
        replace: bool,
        shared: bool,
        values: &[J],
    ) -> Option<String> {
        let mut warning = None;
        if replace {
            let mut dropped: Vec<String> = Vec::new();
            if shared && !self.replaced {
                dropped.push("vanilla's entries".into());
            }
            if !self.mods.is_empty() {
                dropped.push(format!("those of {}", self.mods.join(", ")));
            }
            if !dropped.is_empty() {
                warning = Some(format!(
                    "mod {id} replaces {file}: {} are left out, as a data pack's replace \
                     does; the mods after it in load order still add to it",
                    dropped.join(" and ")
                ));
            }
            self.replaced = true;
            self.mods.clear();
            self.layers.clear();
        }
        if !self.mods.iter().any(|m| m == id) {
            self.mods.push(id.to_owned());
        }
        self.layers.push((values.to_vec(), Vec::new()));
        warning
    }

    /// The layers' entries for [`super::merge_tag`].
    pub(super) fn layers(&self) -> Vec<TagLayers<'_>> {
        self.layers
            .iter()
            .map(|(values, remove)| TagLayers { values, remove })
            .collect()
    }
}
