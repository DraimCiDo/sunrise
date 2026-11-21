use super::{Burying, Config, Counted, LandSearch, Layout, Library};
use serde_json::Value;
use sunrise_core::{identifier::Identifier, java_hash_map::JavaHashMap};

impl Config {
    pub fn decode(v: &Value) -> Self {
        let int = |k: &str| v[k].as_i64().map(|n| n as i32);
        let mut pieces: Vec<(Identifier, Counted)> = Vec::new();
        for p in v["pieces"].as_array().into_iter().flatten() {
            let Some(id) = p["name"].as_str().and_then(|n| Identifier::parse(n).ok()) else {
                continue;
            };
            let counted = Counted {
                required: p["required"].as_i64().map(|n| n as i32),
                min_depth: p["min_depth"].as_i64().unwrap_or(0) as i32,
                max: p["max"].as_i64().map(|n| n as i32),
            };
            // A later entry for the same piece replaces it, as HashMap.put.
            match pieces.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = counted,
                None => pieces.push((id, counted)),
            }
        }
        // getRequirePieces builds a HashMap of the required ones and the
        // assembler copies it (new HashMap(map)): iterate as that copy does.
        let mut map = JavaHashMap::default();
        for (i, (id, c)) in pieces.iter().enumerate() {
            if c.required.is_some() {
                map.put(id.clone(), i);
            }
        }
        let order: Vec<usize> = map.copied().entries().map(|(_, &i)| i).collect();
        let mut sorted: Vec<(Identifier, Counted)> =
            order.iter().map(|&i| pieces[i].clone()).collect();
        sorted.extend(pieces.iter().filter(|(_, c)| c.required.is_none()).cloned());
        Self {
            library: match v["library"].as_str() {
                Some("repurposed") => Library::Repurposed,
                _ => Library::Moogs,
            },
            min_y: int("min_y"),
            max_y: int("max_y"),
            burying: match v["burying"].as_str() {
                Some("lowest_corner") => Some(Burying::LowestCorner),
                Some("average_land") => Some(Burying::AverageLand),
                Some("lowest_side") => Some(Burying::LowestSide),
                _ => None,
            },
            no_liquid: v["no_liquid"].as_bool().unwrap_or(false),
            ignore_bounds: v["ignore_bounds_pools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some(p.as_str()?.to_owned()))
                .collect(),
            land_search: match v["land_search"].as_str() {
                Some("highest_land") => Some(LandSearch::Highest),
                Some("lowest_land") => Some(LandSearch::Lowest),
                Some("fixed_height") => Some(LandSearch::Fixed),
                _ => None,
            },
            ledge: int("ledge_offset_y").unwrap_or(0),
            layout: match v["layout"].as_str() {
                Some("mineshaft") => Layout::Mineshaft,
                _ => Layout::Generic,
            },
            pieces: sorted,
        }
    }
}
