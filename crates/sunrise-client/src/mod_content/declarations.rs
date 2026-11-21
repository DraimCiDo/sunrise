//! Worldgen files that declare additions to other registry entries.
use super::{Addition, J, Result};

/// The key of a mod's structure set, template pool or processor list file
/// that adds its entries to other sets, pools or lists (vanilla's, another
/// mod's) instead of being one of its own. Vanilla data never has it.
const ADD_TO: &str = "sunrise:add_to";

/// An addition's place among the others (lower first; 1000 by default), as
/// Lithostitched's modifiers' `priority`.
const PRIORITY: &str = "sunrise:priority";

/// The bundles a mod may add to, and the list each entry goes into.
const ADDABLE: [(&str, &str); 3] = [
    ("worldgen/structure_sets.json", "structures"),
    ("worldgen/template_pools.json", "elements"),
    ("worldgen/processor_lists.json", "processors"),
];

impl Addition {
    /// What a mod file of `bundle` adds, if it is an addition file.
    pub(super) fn read(bundle: &str, text: &str) -> Result<Option<Self>> {
        let Some((_, list)) = ADDABLE.iter().find(|(b, _)| *b == bundle) else {
            return Ok(None);
        };
        if !text.contains(ADD_TO) {
            return Ok(None);
        }
        let doc: J = serde_json::from_str(text)?;
        let Some(targets) = doc.get(ADD_TO) else {
            return Ok(None);
        };
        let targets = match targets {
            J::Str(s) => vec![s.clone()],
            J::Arr(a) => a
                .iter()
                .map(|v| match v {
                    J::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .collect::<Option<_>>()
                .ok_or(format!("{ADD_TO}: not a list of ids"))?,
            _ => return Err(format!("{ADD_TO}: not an id or a list of ids").into()),
        };
        let Some(J::Arr(entries)) = doc.get(list) else {
            return Err(format!("{ADD_TO} without a {list} list").into());
        };
        let priority = match doc.get(PRIORITY) {
            None => 1000,
            Some(J::Num(n)) => n.as_i64().ok_or(format!("{PRIORITY}: not an integer"))?,
            Some(_) => return Err(format!("{PRIORITY}: not an integer").into()),
        };
        Ok(Some(Self {
            targets,
            list,
            entries: entries.clone(),
            priority,
        }))
    }
}
