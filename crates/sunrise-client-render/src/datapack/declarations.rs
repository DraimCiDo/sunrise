//! stable-line: content-declarations
//! Server pack declaration locations and resource ids.
use super::resource_id;

/// `<kind-dir>/<ns>:<path>` for a pack file the content tables read:
/// `recipe/`, `recipe_filter/`, `loot_table/`, `advancement/`, `trade_set/`,
/// `villager_trade/`, `dialog/`, `damage_type/`, `dimension_type/`, `data_maps/`, enchantments, songs,
/// instruments, painting variants, banner patterns, predicates, item modifiers, `world_clock/`,
/// `worldgen/biome/` or `tags/<sub>/`, where the id keeps the subpath for nested tag kinds.
pub(super) fn content_id(file: &str) -> Option<String> {
    let rest = file.strip_prefix("data/")?;
    let (ns, rest) = rest.split_once('/')?;
    let (kind, rest) = rest.split_once('/')?;
    let kind = match kind {
        "recipe" | "recipe_filter" | "loot_table" | "advancement" | "trade_set"
        | "villager_trade" | "dialog" | "damage_type" | "dimension_type" | "data_maps"
        | "enchantment" | "jukebox_song" | "instrument" | "painting_variant" | "banner_pattern"
        | "predicate" | "item_modifier" | "world_clock" => kind,
        "worldgen" => {
            return resource_id(file, "worldgen/biome", ".json")
                .map(|id| format!("worldgen/biome/{id}"));
        }
        // A tag id is `{ns}:{path-under-tags}` (`tags/worldgen/biome/
        // test:sub/mytag`), the whole path after the namespace, so nested
        // registries keep their directory (`TagLoader`'s id shape).
        "tags" => {
            return Some(format!("tags/{ns}:{}", rest.strip_suffix(".json")?));
        }
        _ => return None,
    };
    Some(format!("{kind}/{ns}:{}", rest.strip_suffix(".json")?))
}
