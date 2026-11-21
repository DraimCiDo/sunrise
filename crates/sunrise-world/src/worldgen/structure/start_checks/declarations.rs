//! stable-line: content-declarations
//! Extra start-check declarations for carried structures.

use super::{LiquidCheck, Repurposed, StartChecks};
use serde_json::Value;

impl StartChecks {
    /// `"sunrise:start_checks"` of a structure, or none.
    pub fn decode(value: Option<&Value>) -> Self {
        let Some(v) = value else {
            return Self::default();
        };
        let int = |k: &str| v.get(k).and_then(Value::as_i64).map(|n| n as i32);
        Self {
            biome_radius: int("biome_radius"),
            liquid: match v.get("liquid").and_then(Value::as_str) {
                Some("top") => Some(LiquidCheck::Top),
                Some("lowest") => Some(LiquidCheck::Lowest),
                _ => None,
            },
            terrain_radius: int("terrain_radius"),
            terrain_range: int("terrain_range"),
            min_y: int("min_y"),
            max_y: int("max_y"),
            repurposed: match v.get("repurposed").and_then(Value::as_str) {
                Some("city_nether") => Some(Repurposed::CityNether),
                Some("shipwreck_nether") => Some(Repurposed::ShipwreckNether),
                Some("stronghold_end") => Some(Repurposed::StrongholdEnd),
                _ => None,
            },
        }
    }
}
