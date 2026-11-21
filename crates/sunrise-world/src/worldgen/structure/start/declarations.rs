use super::{
    Avoid, AvoidBiomes, JigsawDef, ModType, Placement, Reduction, RingSections, STEPS,
    StructureDef, StructureKind, StructureRegistry, StructureSet,
};
use crate::{
    heightmap::HeightmapType,
    worldgen::{
        DecodeError,
        biome::BiomeId,
        height_provider::HeightProvider,
        structure::{
            enhanced_terrain, formations,
            jigsaw::{Draws, Pools},
            mod_types::ModStructureTypes,
            piece_limited, ruined_portal, start_checks,
        },
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use sunrise_core::{density::TerrainAdjustment, position::ChunkPos};

impl RingSections {
    pub(super) fn decode(v: &Value) -> Self {
        let int = |k: &str| v[k].as_i64().map(|n| n as i32);
        Self {
            first: int("distance_to_first_ring").unwrap_or(0),
            thickness: int("thickness").unwrap_or(1).max(1),
            max: int("max_section"),
        }
    }
}

impl AvoidBiomes {
    fn decode(
        v: &Value,
        biome_set: &dyn Fn(&Value) -> Result<BTreeSet<BiomeId>, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let mut biomes = BTreeSet::new();
        match &v["biomes"] {
            Value::Array(list) => {
                for entry in list {
                    biomes.extend(biome_set(entry)?);
                }
            }
            other => biomes = biome_set(other)?,
        }
        let int = |k: &str, default: i32| v[k].as_i64().map_or(default, |n| n as i32);
        Ok(Self {
            biomes,
            radius: int("radius", 0),
            step: int("step", 1).max(1),
            y: int("y", 0),
        })
    }
}

fn heightmap(name: &str) -> Option<HeightmapType> {
    HeightmapType::ALL.into_iter().find(|k| k.name() == name)
}

fn terrain(name: Option<&str>) -> TerrainAdjustment {
    match name {
        Some("bury") => TerrainAdjustment::Bury,
        Some("beard_thin") => TerrainAdjustment::BeardThin,
        Some("beard_box") => TerrainAdjustment::BeardBox,
        Some("encapsulate") => TerrainAdjustment::Encapsulate,
        _ => TerrainAdjustment::None,
    }
}

/// A jigsaw field given as one number or as an object of two. In the object a
/// missing `a` is 0 and a missing `b` is `missing_b` (`JigsawStructure.MaxDistance`
/// and `DimensionPadding` default their second value, not copy the first).
fn pair(value: Option<&Value>, a: &str, b: &str, default: i32, missing_b: i32) -> [i32; 2] {
    match value {
        Some(Value::Number(n)) => [n.as_i64().unwrap_or(0) as i32; 2],
        Some(Value::Object(o)) => {
            let get = |k: &str| o.get(k).and_then(Value::as_i64).map(|v| v as i32);
            [get(a).unwrap_or(0), get(b).unwrap_or(missing_b)]
        }
        _ => [default; 2],
    }
}

/// `JigsawStructure`'s `size` (its depth) and `max_distance_from_center`.
fn jigsaw_depth_and_reach(v: &Value) -> (i32, [i32; 2]) {
    (
        v["size"].as_i64().unwrap_or(0) as i32,
        pair(
            v.get("max_distance_from_center"),
            "horizontal",
            "vertical",
            80,
            // DimensionType.Y_SIZE.
            4064,
        ),
    )
}

/// The bounds `JigsawStructure`'s codec puts on those two: `size` is
/// `Codec.intRange(0, 20)` and `MaxDistance` is `horizontal`
/// `Codec.intRange(1, 128)` beside `vertical`
/// `ExtraCodecs.intRange(1, DimensionType.Y_SIZE)`.
fn jigsaw_reach_in_range(depth: i32, [horizontal, vertical]: [i32; 2]) -> bool {
    (0..=20).contains(&depth) && (1..=128).contains(&horizontal) && (1..=4064).contains(&vertical)
}

impl StructureRegistry {
    pub fn new(
        structures: &BTreeMap<String, String>,
        sets: &BTreeMap<String, String>,
        pools: BTreeMap<String, String>,
        biome_set: &dyn Fn(&Value) -> Result<BTreeSet<BiomeId>, DecodeError>,
        // The structure types the executable's mods registered, looked up
        // before `Unsupported` (`"type": "modid:tower"`).
        mod_types: &ModStructureTypes,
    ) -> Result<Self, DecodeError> {
        let mut defs = Vec::new();
        for (name, json) in structures {
            let v: Value = serde_json::from_str(json)?;
            let draws = match v["type"].as_str() {
                Some("yungsapi:yung_jigsaw") => Draws::Yung,
                _ => Draws::Vanilla,
            };
            let kind = match v["type"].as_str().unwrap_or_default() {
                "minecraft:jigsaw" | "yungsapi:yung_jigsaw" => {
                    let (depth, max_distance) = jigsaw_depth_and_reach(&v);
                    let def = JigsawDef {
                        draws,
                        start_pool: v["start_pool"].as_str().unwrap_or_default().to_owned(),
                        start_jigsaw: v["start_jigsaw_name"].as_str().map(str::to_owned),
                        max_depth: depth,
                        start_height: HeightProvider::from_json(&v["start_height"].to_string())?,
                        use_expansion_hack: v["use_expansion_hack"].as_bool().unwrap_or(false),
                        project_to: v["project_start_to_heightmap"].as_str().and_then(heightmap),
                        max_distance,
                        aliases: v["pool_aliases"].as_array().cloned().unwrap_or_default(),
                        padding: pair(v.get("dimension_padding"), "bottom", "top", 0, 0),
                        waterlogging: v["liquid_settings"].as_str() != Some("ignore_waterlogging"),
                        checks: start_checks::StartChecks::decode(v.get("sunrise:start_checks")),
                        piece_limited: v
                            .get("sunrise:piece_limited")
                            .map(piece_limited::Config::decode),
                    };
                    // JigsawStructure's codec bounds `size` and
                    // `max_distance_from_center`: vanilla registers no
                    // structure carrying others, so one that does places
                    // nothing here either rather than reaching past what the
                    // generator allows.
                    if jigsaw_reach_in_range(depth, max_distance) {
                        StructureKind::Jigsaw(def)
                    } else {
                        StructureKind::Unsupported("minecraft:jigsaw".into())
                    }
                }
                "minecraft:buried_treasure" => StructureKind::BuriedTreasure,
                "minecraft:igloo" => StructureKind::Igloo,
                "minecraft:swamp_hut" => StructureKind::SwampHut,
                "minecraft:desert_pyramid" => StructureKind::DesertPyramid,
                "minecraft:jungle_temple" => StructureKind::JungleTemple,
                "minecraft:woodland_mansion" => StructureKind::Mansion,
                "minecraft:ocean_monument" => StructureKind::Monument(biome_set(
                    &serde_json::json!("#minecraft:required_ocean_monument_surrounding"),
                )?),
                "minecraft:ruined_portal" => StructureKind::RuinedPortal(
                    v["setups"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(ruined_portal::Setup::decode)
                        .collect(),
                ),
                "minecraft:shipwreck" => StructureKind::Shipwreck {
                    beached: v["is_beached"].as_bool().unwrap_or(false),
                },
                "minecraft:ocean_ruin" => StructureKind::OceanRuin {
                    warm: v["biome_temp"].as_str() == Some("warm"),
                    large: v["large_probability"].as_f64().unwrap_or(0.) as f32,
                    cluster: v["cluster_probability"].as_f64().unwrap_or(0.) as f32,
                },
                "minecraft:mineshaft" => StructureKind::Mineshaft {
                    mesa: v["mineshaft_type"].as_str() == Some("mesa"),
                    blocking: Arc::new(biome_set(&Value::String(
                        "#minecraft:mineshaft_blocking".into(),
                    ))?),
                },
                "minecraft:stronghold" => StructureKind::Stronghold,
                "minecraft:fortress" => StructureKind::Fortress,
                "minecraft:end_city" => StructureKind::EndCity,
                "minecraft:nether_fossil" => StructureKind::NetherFossil(
                    HeightProvider::from_json(&v["height"].to_string())?,
                ),
                "sunrise:formations" => match formations::Def::decode(&v) {
                    Ok(def) => StructureKind::Formations(def),
                    Err(_) => StructureKind::Unsupported("sunrise:formations".into()),
                },
                other => match mod_types.get(other) {
                    Some(ty) => StructureKind::Mod(ModType {
                        ty: ty.clone(),
                        config: ty.prepare_config(&v).map_err(|error| {
                            DecodeError::Registry(format!("structure {name}: {error}"))
                        })?,
                    }),
                    None => StructureKind::Unsupported(other.to_owned()),
                },
            };
            defs.push(StructureDef {
                name: name.clone(),
                kind,
                biomes: biome_set(&v["biomes"])?,
                step: STEPS
                    .iter()
                    .position(|s| Some(*s) == v["step"].as_str())
                    .unwrap_or(4),
                terrain: terrain(v["terrain_adaptation"].as_str()),
                enhanced_terrain: v
                    .get("sunrise:enhanced_terrain_adaptation")
                    .map(enhanced_terrain::StructureAdaptation::decode)
                    .transpose()?,
            });
        }
        let index = |name: &str| defs.iter().position(|d| d.name == name);
        let mut out = Vec::new();
        for (name, json) in sets {
            let v: Value = serde_json::from_str(json)?;
            let p = &v["placement"];
            // AbstractSpreadingStructurePlacement's codec: `frequency` is a
            // floatRange(0.0F, 1.0F) and the salt any int.
            let frequency = p["frequency"].as_f64().unwrap_or(1.);
            if !(0. ..=1.).contains(&frequency) {
                return Err(DecodeError::Field("structure set frequency"));
            }
            let placement = match p["type"].as_str().unwrap_or_default() {
                "minecraft:random_spread" => {
                    // The codec's intRange(0, 4096) and
                    // RandomSpreadStructurePlacement.validate: a spread whose
                    // separation reaches its spacing is refused, not clamped
                    // to the grid corners.
                    let spacing = p["spacing"].as_i64().unwrap_or(1);
                    let separation = p["separation"].as_i64().unwrap_or(0);
                    if !(0..=4096).contains(&spacing) || !(0..=4096).contains(&separation) {
                        return Err(DecodeError::Field("random spread spacing range"));
                    }
                    if spacing <= separation {
                        return Err(DecodeError::Field(
                            "random spread spacing not larger than separation",
                        ));
                    }
                    Placement::RandomSpread {
                        spacing: spacing as i32,
                        separation: separation as i32,
                        triangular: p["spread_type"].as_str() == Some("triangular"),
                    }
                }
                "minecraft:dimension_origin" => Placement::DimensionOrigin,
                "minecraft:concentric_rings" => Placement::ConcentricRings {
                    distance: p["distance"].as_i64().unwrap_or(0) as i32,
                    spread: p["spread"].as_i64().unwrap_or(0) as i32,
                    count: p["count"].as_i64().unwrap_or(0) as i32,
                    preferred: biome_set(&p["preferred_biomes"])?,
                },
                _ => Placement::Unsupported,
            };
            let mut structures = Vec::new();
            for entry in v["structures"].as_array().into_iter().flatten() {
                // StructureSelectionEntry.CODEC uses ExtraCodecs.POSITIVE_INT.
                let weight = entry["weight"]
                    .as_i64()
                    .filter(|w| (1..=i64::from(i32::MAX)).contains(w))
                    .ok_or(DecodeError::Field("structure set positive int weight"))?;
                if let Some(definition) = entry["structure"].as_str().and_then(index) {
                    structures.push((definition, weight as i32));
                }
            }
            out.push(StructureSet {
                name: name.clone(),
                structures,
                placement,
                salt: p["salt"].as_i64().unwrap_or(0) as i32,
                frequency: frequency as f32,
                locate_offset: {
                    let at = |i: usize| p["locate_offset"][i].as_i64().unwrap_or(0) as i32;
                    [at(0), at(1), at(2)]
                },
                reduction: match p["frequency_reduction_method"].as_str() {
                    Some("legacy_type_1") => Reduction::Legacy1,
                    Some("legacy_type_2") => Reduction::Legacy2,
                    Some("legacy_type_3") => Reduction::Legacy3,
                    _ => Reduction::Default,
                },
                exclusion: p.get("exclusion_zone").map(|z| {
                    (
                        z["other_set"].as_str().unwrap_or_default().to_owned(),
                        z["chunk_count"].as_i64().unwrap_or(0) as i32,
                    )
                }),
                min_distance: p
                    .get("sunrise:min_distance_from_world_origin")
                    .and_then(Value::as_i64),
                avoid: p.get("sunrise:exclusion_zone").map(|z| Avoid {
                    sets: z["other_sets"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| Some(s.as_str()?.to_owned()))
                        .collect(),
                    chunk_count: z["chunk_count"].as_i64().unwrap_or(0) as i32,
                    their_zones: z["their_zones"].as_bool().unwrap_or(true),
                }),
                rings: p.get("sunrise:ring_sections").map(RingSections::decode),
                avoid_biomes: p
                    .get("sunrise:avoid_biomes")
                    .map(|a| AvoidBiomes::decode(a, biome_set))
                    .transpose()?,
            });
        }
        Ok(Self {
            structures: defs,
            sets: out,
            pools: Pools::new(pools),
            origin: ChunkPos::new(0, 0),
        })
    }
}
