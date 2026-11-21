//! Data maps for native server mods (docs/modding/src/how-to/native-mods.md): values a data pack
//! attaches to the entries of a registry, under the types a mod registered
//! (`native_data_map_types`). Vanilla has no data maps; the contract is
//! NeoForge's `DataMapType`: files at
//! `data/<map namespace>/data_maps/<registry folder>/<map path>.json` hold
//! `replace`, `values` (entry ids and `#tags` to values, each value optionally
//! guarded by its own `neoforge:conditions`) and `remove`, and a newer value
//! overrides an older one for the same entry. A registry's folder is its path
//! for a `minecraft` registry and `<namespace>/<path>` otherwise. Any registry
//! the game names takes maps: built in, the packs' and content mods', and the
//! datapack registries mods declare. The files are the world's data packs' and
//! what mods contribute through `native_data_pack_files` (key
//! `data_maps/<ns>:<registry folder>/<map path>`), merged each time the
//! server's tables are built: a world's first open and every `/reload`.
use super::Game;
use super::data_map_json::to_tag;
pub use mod_api::game::DataMapType;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use sunrise_core::nbt::Tag;


/// The registries mods read through a typed key (`DataMapEntry`): always
/// known, even where the command registries list no ids.
const TYPED: [&str; 9] = [
    "minecraft:item",
    "minecraft:block",
    "minecraft:fluid",
    "minecraft:mob_effect",
    "minecraft:entity_type",
    "minecraft:enchantment",
    "minecraft:block_entity_type",
    "minecraft:villager_profession",
    "minecraft:worldgen/biome",
];

/// The registries a data load resolves maps against.
pub(crate) trait MapRegistries {
    /// Whether the game names `registry`: built in, a pack's or a datapack
    /// registry a mod declared.
    fn known(&self, registry: &str) -> bool;
    /// Whether `entry` is in `registry`; true where the registry's ids aren't listed.
    fn contains(&self, registry: &str, entry: &str) -> bool;
    /// The members of `tag` (`minecraft:logs`) in `registry`; none for an unknown tag.
    fn tag(&self, registry: &str, tag: &str) -> Vec<String>;
}

/// A world's registries while its server tables build: every registry's ids
/// as the command registries hold them (built in, the packs' and content
/// mods'), the world's loaded datapack registries (the authority for a
/// mod-declared one the command ids do not list), the loaded item and block
/// tags, and another registry's tags read from the same packs on first use.
pub(crate) struct WorldRegistries<'a> {
    ids: BTreeMap<&'a str, &'a [String]>,
    datapack: Option<&'a mod_api::DatapackRegistrySet>,
    item_tags: &'a HashMap<String, HashSet<String>>,
    block_tags: &'a HashMap<String, HashSet<String>>,
    root: &'a std::path::Path,
    overlay: &'a sunrise_world::loot::PackOverlay<'a>,
    id_sets: RefCell<HashMap<String, HashSet<String>>>,
    datapack_ids: RefCell<HashMap<String, HashSet<String>>>,
    tags: RefCell<HashMap<String, HashMap<String, HashSet<String>>>>,
}
impl<'a> WorldRegistries<'a> {
    pub(crate) fn new(
        ids: impl IntoIterator<Item = (&'a str, &'a [String])>,
        datapack: Option<&'a mod_api::DatapackRegistrySet>,
        items: &'a sunrise_client::items::Items,
        root: &'a std::path::Path,
        overlay: &'a sunrise_world::loot::PackOverlay<'a>,
    ) -> Self {
        Self {
            ids: ids.into_iter().collect(),
            datapack,
            item_tags: &items.item_tags,
            block_tags: &items.block_tags,
            root,
            overlay,
            id_sets: RefCell::default(),
            datapack_ids: RefCell::default(),
            tags: RefCell::default(),
        }
    }
    /// Whether a mod's declaration names `registry`.
    fn declared(&self, registry: &str) -> bool {
        mod_api::__private::datapack_registry_declarations()
            .iter()
            .any(|declaration| declaration.key == registry)
    }
    /// The loaded entries of a mod-declared datapack registry, read once.
    fn datapack_entries(&self, registry: &str) -> Option<HashSet<String>> {
        let mut sets = self.datapack_ids.borrow_mut();
        Some(
            sets.entry(registry.to_owned())
                .or_insert_with(|| {
                    self.datapack
                        .and_then(|set| set.entries(registry).ok())
                        .map(|entries| entries.into_iter().map(String::from).collect())
                        .unwrap_or_default()
                })
                .clone(),
        )
    }
}
impl MapRegistries for WorldRegistries<'_> {
    fn known(&self, registry: &str) -> bool {
        self.ids.contains_key(registry) || TYPED.contains(&registry) || self.declared(registry)
    }
    fn contains(&self, registry: &str, entry: &str) -> bool {
        if let Some(ids) = self.ids.get(registry).filter(|ids| !ids.is_empty()) {
            let mut sets = self.id_sets.borrow_mut();
            let set = sets
                .entry(registry.to_owned())
                .or_insert_with(|| ids.iter().cloned().collect());
            return set.contains(entry);
        }
        // A mod-declared datapack registry the command ids do not list: the
        // world's loaded entries are the authority, and an id not held is
        // refused (#3357's unknown-id rejection).
        if self.declared(registry) {
            return self
                .datapack_entries(registry)
                .is_some_and(|set| set.contains(entry));
        }
        // A typed registry the sync did not list: no source says its entries.
        true
    }
    fn tag(&self, registry: &str, tag: &str) -> Vec<String> {
        let members = |tags: &HashMap<String, HashSet<String>>| -> Vec<String> {
            tags.get(tag)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default()
        };
        match registry {
            "minecraft:item" => members(self.item_tags),
            "minecraft:block" => members(self.block_tags),
            _ => {
                let mut loaded = self.tags.borrow_mut();
                let tags = loaded.entry(registry.to_owned()).or_insert_with(|| {
                    sunrise_client::items::load_tags(self.root, &folder(registry), self.overlay)
                        .unwrap_or_else(|e| {
                            log::warn!("Sunrise: the {registry} tags data maps read: {e}");
                            HashMap::new()
                        })
                });
                members(tags)
            }
        }
    }
}

/// The merged values of every registered type: registry, then type id, to the
/// value of each entry that has one.
pub(crate) enum MapValues {
    Json(BTreeMap<String, Value>),
    Decoded(BTreeMap<String, Tag>),
}
#[derive(Default)]
pub(crate) struct DataMaps {
    pub(super) biome_keys: mod_api::__private::BiomeKeys,
    maps: BTreeMap<String, HashMap<String, MapValues>>,
    pub(super) mob_maps: std::sync::Arc<sunrise_client::neoforge_mob_maps::NeoForgeMobMaps>,
}

/// `minecraft:` for an id written without a namespace.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}



impl DataMaps {
    pub(crate) fn with_biome_keys(mut self, keys: mod_api::__private::BiomeKeys) -> Self {
        self.biome_keys = keys;
        self
    }

    pub(crate) fn with_builtins(mut self, builtin: super::neoforge_mob_maps::Loaded) -> Self {
        for (definition, values) in builtin.values {
            if self
                .maps
                .entry(definition.registry.to_owned())
                .or_default()
                .insert(definition.id.to_owned(), values)
                .is_some()
            {
                log::warn!(
                    "Sunrise: builtin data map {} is reserved: engine definition kept",
                    definition.id
                );
            }
        }
        self.mob_maps = builtin.snapshot;
        self
    }

    /// Every entry map `type_id` of `registry` gives a value, with its merged
    /// JSON; none where the map wasn't loaded.
    pub(crate) fn json_values(
        &self,
        registry: &str,
        type_id: &str,
    ) -> impl Iterator<Item = (&str, &Value)> {
        self.maps
            .get(registry)
            .and_then(|maps| maps.get(type_id))
            .and_then(|values| match values {
                MapValues::Json(values) => Some(values),
                MapValues::Decoded(_) => None,
            })
            .into_iter()
            .flatten()
            .map(|(entry, value)| (entry.as_str(), value))
    }

    /// The registries with a loaded type, in id order: what `data_maps_updated` names.
    pub(crate) fn registries(&self) -> Vec<String> {
        self.maps.keys().cloned().collect()
    }

    /// Map `type_id` of `registry`, refused where no mod registered it there.
    fn map(&self, registry: &str, type_id: &str) -> Result<&MapValues, mod_api::DataMapError> {
        self.maps
            .get(registry)
            .and_then(|maps| maps.get(type_id))
            .ok_or(mod_api::DataMapError::UnknownType)
    }

    /// `entry`'s value in map `type_id` of `registry`, as NBT: none where the
    /// map gives it none, refused where no mod registered the map there.
    fn value(
        &self,
        registry: &str,
        type_id: &str,
        entry: &str,
    ) -> Result<Option<Tag>, mod_api::DataMapError> {
        match self.map(registry, type_id)? {
            MapValues::Json(values) => values.get(entry).map(to_tag).transpose(),
            MapValues::Decoded(values) => Ok(values.get(entry).cloned()),
        }
    }

    /// Every entry map `type_id` of `registry` gives a value, in id order.
    fn entries(
        &self,
        registry: &str,
        type_id: &str,
    ) -> Result<Vec<(String, Tag)>, mod_api::DataMapError> {
        match self.map(registry, type_id)? {
            MapValues::Json(values) => values
                .iter()
                .map(|(entry, value)| Ok((entry.clone(), to_tag(value)?)))
                .collect(),
            MapValues::Decoded(values) => Ok(values
                .iter()
                .map(|(entry, value)| (entry.clone(), value.clone()))
                .collect()),
        }
    }
}

impl Game<'_> {
    /// Tell the mods the data maps of every registry with a type were merged
    /// (`DataMapsUpdatedEvent`), once the server's tables hold them.
    pub(in crate::game) fn announce_data_maps_updated(
        &mut self,
        cause: mod_api::DataMapsUpdateCause,
    ) {
        for registry in self.data_map_registries.clone() {
            self.native_data_maps_updated(&registry, cause);
        }
    }
}

impl Game<'_> {
    /// The server's merged data maps, none unless this is a server whose
    /// tables were built from packs and mods' files.
    pub(super) fn loaded_data_maps(&self) -> Result<&DataMaps, mod_api::DataMapError> {
        if self.serving.is_none() && self.options.dedicated.is_none() {
            return Err(mod_api::DataMapError::Unavailable);
        }
        self.world_tables
            .as_ref()
            .map(|tables| &tables.data_maps)
            .ok_or(mod_api::DataMapError::Unavailable)
    }
    /// Entry `entry` of `registry`: its value in data map `map`, before the map decodes it:
    /// the merged JSON the server's data packs and mods' files gave it, as NBT. `Ok(None)`
    /// where it has none.
    pub(in crate::game) fn data_map_value(
        &self,
        registry: &str,
        entry: &str,
        map: &str,
    ) -> Result<Option<Tag>, mod_api::DataMapError> {
        self.loaded_data_maps()?
            .value(registry, map, &full_id(entry))
    }
    /// Every entry data map `map` of `registry` gives a value, with that value, in id order.
    pub(in crate::game) fn data_map_entries(
        &self,
        registry: &str,
        map: &str,
    ) -> Result<Vec<(String, Tag)>, mod_api::DataMapError> {
        self.loaded_data_maps()?.entries(registry, map)
    }
}
