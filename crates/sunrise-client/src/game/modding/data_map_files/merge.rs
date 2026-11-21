//! Application of validated data-map layers in pack order.
use super::{File, Key, Tags};
use std::collections::BTreeMap;

impl Key {
    fn visit(&self, tags: &Tags, mut visit: impl FnMut(&str)) {
        match self {
            Self::Entry(id) => visit(id),
            Self::Tag(tag) => {
                if let Some(members) = tags.get(tag) {
                    for id in members {
                        visit(id);
                    }
                }
            }
        }
    }
}

impl<T: Clone> File<T> {
    pub(super) fn apply(
        self,
        map: &mut BTreeMap<String, T>,
        tags: &Tags,
        known: impl Fn(&str) -> bool,
        context: &str,
    ) {
        if self.replace {
            map.clear();
        }
        for (key, value) in self.values {
            key.visit(tags, |id| {
                if known(id) {
                    map.insert(id.to_owned(), value.clone());
                } else {
                    log::warn!("Sunrise: {context}: unknown target {id}, entry skipped");
                }
            });
        }
        for key in self.remove {
            key.visit(tags, |id| {
                map.remove(id);
            });
        }
    }
}

/// Pack-order loading shared by raw mod maps and typed builtin codecs. Each
/// value decodes before target membership, so even an unknown target cannot
/// conceal a bad value and allow the rest of that file to apply.
pub(super) fn merge_versions<T: Clone>(
    id: &str,
    directory: &str,
    mut map: BTreeMap<String, T>,
    overlay: &sunrise_world::loot::PackOverlay<'_>,
    tags: &Tags,
    prepare: impl Fn(&[u8], &str) -> Result<Option<File<T>>, String>,
    known: impl Fn(&str) -> bool,
) -> (BTreeMap<String, T>, bool) {
    let (namespace, path) = id.split_once(':').expect("validated data-map type ID");
    let wanted = format!("data_maps/{namespace}:{directory}/{path}");
    let mut applied = false;
    if let Some((_, versions)) = overlay.files.iter().find(|(key, _)| *key == wanted) {
        for (layer, bytes) in versions.iter().enumerate() {
            let source = format!(
                "data/{namespace}/data_maps/{directory}/{path}.json (layer {})",
                layer + 1
            );
            match prepare(bytes, &source) {
                Ok(Some(file)) => {
                    file.apply(&mut map, tags, &known, &source);
                    applied = true;
                }
                Ok(None) => {}
                Err(error) => log::error!(
                    "Sunrise: data map {id} on minecraft:{directory}: {source}: file skipped: {error}"
                ),
            }
        }
    }
    (map, applied)
}
