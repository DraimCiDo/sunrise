//! NeoForge removal entries while resolving biome modifier tags.
use super::{Registry, extend, full};
use serde_json::Value;
use std::collections::BTreeSet;

impl Registry<'_> {
    // Determine the edge to drop from the graph, not from traversal order.
    fn removal_reaches(&self, start: &str, goal: &str) -> bool {
        let mut pending = vec![start.to_owned()];
        let mut visited = BTreeSet::new();
        while let Some(tag) = pending.pop() {
            if tag == goal {
                return true;
            }
            if !visited.insert(tag.clone()) {
                continue;
            }
            let Some(doc) = self
                .tags
                .get(&tag)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
            else {
                continue;
            };
            for value in doc["remove"].as_array().into_iter().flatten().chain(
                doc["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| v["remove"].as_bool() == Some(true)),
            ) {
                if let Some(inner) =
                    sunrise_world::tag_layers::entry_id(value).and_then(|id| id.strip_prefix('#'))
                {
                    pending.push(full(inner));
                }
            }
        }
        false
    }

    pub(super) fn expand(
        &self,
        tag: &str,
        out: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) -> std::result::Result<(), String> {
        if !seen.insert(tag.to_owned()) {
            return Ok(());
        }
        let text = self
            .tags
            .get(tag)
            .ok_or_else(|| format!("unknown {} tag #{tag}", self.what))?;
        let doc: Value = serde_json::from_str(text).map_err(|e| format!("#{tag}: {e}"))?;
        let mut own = Vec::new();
        let operations = doc["values"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| (v, v["remove"].as_bool() == Some(true)))
            .chain(
                doc["remove"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| (v, true)),
            );
        for (value, removes) in operations {
            let Some(id) = sunrise_world::tag_layers::entry_id(value) else {
                continue;
            };
            let required = !removes && value["required"].as_bool() != Some(false);
            let mut members = Vec::new();
            if let Some(inner) = id.strip_prefix('#') {
                let target = full(inner);
                if removes && target.as_str() > tag && self.removal_reaches(&target, tag) {
                    log::warn!(
                        "Sunrise: tags {tag} and {target} form a removal cycle; only removal edge {tag} -> {target} is ignored"
                    );
                    continue;
                }
                if let Err(error) = self.expand(&target, &mut members, seen) {
                    if required {
                        return Err(error);
                    }
                    continue;
                }
            } else if self.ids.contains(&full(id)) {
                members.push(full(id));
            }
            if removes {
                own.retain(|id| !members.contains(id));
            } else {
                extend(&mut own, members);
            }
        }
        seen.remove(tag);
        extend(out, own);
        Ok(())
    }
}
