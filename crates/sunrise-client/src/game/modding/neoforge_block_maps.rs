//! stable-line: no-content-declarations
//! NeoForge's built-in block data maps (`NeoForgeDataMaps`): `neoforge:flammable`,
//! `neoforge:oxidizables` and `neoforge:waxables`, merged from the world's
//! data packs and mods' files as a mod's own data maps are, and installed on
//! the world's behaviours for fire spread, weathering, scraping and waxing.
use super::Game;
use super::native_data_maps::{DataMapType, DataMaps, MapRegistries, WorldRegistries};
use std::sync::Arc;
use sunrise_world::level::BlockMaps;
#[path = "neoforge_block_maps/declarations.rs"]
mod declarations;
use declarations::MAPS;

#[cfg(test)]
#[path = "neoforge_block_maps/neoforge_block_maps_tests.rs"]
mod tests;

/// The maps of a world's tables' files; none where no pack gives a value. `rank` is a block's
/// registry id, which picks the block an inverse map gives back.
fn load(
    overlay: &sunrise_world::loot::PackOverlay<'_>,
    registries: &dyn MapRegistries,
    rank: impl Fn(&str) -> u32,
) -> Option<Arc<BlockMaps>> {
    let types = MAPS.map(|id| DataMapType {
        id: id.to_owned(),
        registry: "minecraft:block".to_owned(),
    });
    let maps = DataMaps::load(&types, overlay, registries);
    let values = |id| maps.json_values("minecraft:block", id);
    let block_maps =
        BlockMaps::from_values(values(MAPS[0]), values(MAPS[1]), values(MAPS[2]), rank);
    (!block_maps.is_empty()).then(|| Arc::new(block_maps))
}

impl Game<'_> {
    /// Install the open world's tables' block maps, replacing the last
    /// load's: none for a world without tables.
    pub(in crate::game) fn install_block_maps(&self) {
        let maps = self.world_tables.as_ref().and_then(|tables| {
            let overlay = sunrise_world::loot::PackOverlay {
                conditions: &tables.conditions,
                files: &tables.files,
                ..sunrise_world::loot::PackOverlay::default()
            };
            let table = &self.res.behaviours().table;
            let rank = |block: &str| {
                table
                    .block_by_name(block)
                    .map_or(u32::MAX, |b| u32::from(b.0))
            };
            // Tables and the registry generation are installed together before this call.
            // Use the same membership boundary as their staged data-map load.
            let registries = WorldRegistries::new(
                self.ui
                    .registries
                    .by_registry
                    .iter()
                    .map(|(registry, entries)| (registry.as_str(), entries.ids.as_slice())),
                self.mods.datapack_registries.as_deref(),
                &tables.items,
                &self.options.resources,
                &overlay,
            );
            load(&overlay, &registries, rank)
        });
        self.res.behaviours().set_block_maps(maps);
    }
}
