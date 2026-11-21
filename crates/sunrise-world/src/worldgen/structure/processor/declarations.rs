/// The spawner fields `sunrise:spawner_mobs` writes: its JSON key, the NBT
/// key and the default (BaseSpawner's).
const SPAWNER_FIELDS: [(&str, &str, i16); 7] = [
    ("delay", "Delay", 20),
    ("min_spawn_delay", "MinSpawnDelay", 200),
    ("max_spawn_delay", "MaxSpawnDelay", 800),
    ("spawn_count", "SpawnCount", 4),
    ("max_nearby_entities", "MaxNearbyEntities", 6),
    ("required_player_range", "RequiredPlayerRange", 16),
    ("spawn_range", "SpawnRange", 4),
];

impl PosRule {
    /// A rule's `position_predicate` (always true when missing). Types are
    /// identifiers: `"always_true"` is `minecraft:always_true`, as vanilla
    /// reads it.
    fn decode(value: Option<&Value>) -> Result<Self, DecodeError> {
        let Some(p) = value else {
            return Ok(Self::Always);
        };
        let kind = p["predicate_type"]
            .as_str()
            .unwrap_or("minecraft:always_true");
        Ok(match super::super::tags::identifier(kind)?.as_str() {
            "minecraft:always_true" => Self::Always,
            kind @ ("minecraft:linear_pos" | "minecraft:axis_aligned_linear_pos") => Self::Linear {
                min_chance: p.get("min_chance").and_then(Value::as_f64).unwrap_or(0.) as f32,
                max_chance: p.get("max_chance").and_then(Value::as_f64).unwrap_or(0.) as f32,
                min_dist: p.get("min_dist").and_then(Value::as_i64).unwrap_or(0) as i32,
                max_dist: p.get("max_dist").and_then(Value::as_i64).unwrap_or(0) as i32,
                axis: (kind == "minecraft:axis_aligned_linear_pos").then(|| {
                    match p.get("axis").and_then(Value::as_str) {
                        Some("x") => 0,
                        Some("z") => 2,
                        _ => 1,
                    }
                }),
            },
            "sunrise:piece_origin_axis_aligned_linear_pos" => {
                let int = |key| p.get(key).and_then(Value::as_i64).unwrap_or(0) as i32;
                let (min_dist, max_dist) = (int("min_dist"), int("max_dist"));
                if min_dist >= max_dist {
                    return Err(DecodeError::Field("position predicate distance range"));
                }
                Self::PieceOrigin {
                    min_chance: p.get("min_chance").and_then(Value::as_f64).unwrap_or(0.) as f32,
                    max_chance: p.get("max_chance").and_then(Value::as_f64).unwrap_or(0.) as f32,
                    min_dist,
                    max_dist,
                    axis: match p.get("axis").map(|a| a.as_str()) {
                        Some(Some("x")) => 0,
                        None | Some(Some("y")) => 1,
                        Some(Some("z")) => 2,
                        _ => return Err(DecodeError::Field("position predicate axis")),
                    },
                }
            }
            "sunrise:y_value" => {
                let int = |key| {
                    p.get(key)
                        .and_then(Value::as_i64)
                        .map(|v| v as i32)
                        .ok_or(DecodeError::Field("position predicate y value"))
                };
                let (min, max) = (int("min_y_value")?, int("max_y_value")?);
                if min > max {
                    return Err(DecodeError::Field("position predicate y range"));
                }
                Self::YValue { min, max }
            }
            _ => return Err(DecodeError::Field("position predicate")),
        })
    }
}

impl Modifier {
    /// A rule's `block_entity_modifier` (passthrough when missing).
    fn decode(value: Option<&Value>) -> Result<Self, DecodeError> {
        let Some(m) = value else {
            return Ok(Self::Passthrough);
        };
        Ok(
            match super::super::tags::identifier(m["type"].as_str().unwrap_or(""))?.as_str() {
                "minecraft:passthrough" => Self::Passthrough,
                "minecraft:clear" => Self::Clear,
                "minecraft:append_loot" => {
                    Self::AppendLoot(m["loot_table"].as_str().unwrap_or_default().to_owned())
                }
                "minecraft:append_static" => match json_to_nbt(&m["data"]) {
                    Tag::Compound(c) => Self::AppendStatic(c),
                    _ => Self::AppendStatic(Compound::new()),
                },
                "sunrise:edit" => Self::Edit {
                    remove: m["remove"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|k| k.as_str().map(str::to_owned))
                        .collect::<Option<_>>()
                        .ok_or(DecodeError::Field("block entity modifier keys"))?,
                    data: match json_to_nbt(&m["data"]) {
                        Tag::Compound(c) => c,
                        _ => Compound::new(),
                    },
                },
                _ => return Err(DecodeError::Field("block entity modifier")),
            },
        )
    }
}

fn below(v: &Value) -> Result<f32, DecodeError> {
    v["below"]
        .as_f64()
        .map(|b| b as f32)
        .ok_or(DecodeError::Field("pick bound"))
}

impl Pick {
    fn decode(
        value: &Value,
        state: impl Fn(&Value) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let outcome = |v: &Value| -> Result<Outcome, DecodeError> {
            let states = match v.get("states") {
                Some(list) => list
                    .as_array()
                    .ok_or(DecodeError::Field("pick states"))?
                    .iter()
                    .map(&state)
                    .collect::<Result<Vec<_>, _>>()?,
                None => vec![state(&v["state"])?],
            };
            if states.is_empty() {
                return Err(DecodeError::Field("pick states"));
            }
            let nbt = match v.get("nbt").map(json_to_nbt) {
                Some(Tag::Compound(c)) => Some(c),
                Some(_) => return Err(DecodeError::Field("pick nbt")),
                None => None,
            };
            let text = |v: &Value| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or(DecodeError::Field("pick list"))
            };
            let lists = v["lists"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| {
                    Ok(PickedList {
                        key: text(&l["key"])?,
                        count: l["count"].as_u64().ok_or(DecodeError::Field("pick list"))? as usize,
                        values: l["values"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|e| Ok((below(e)?, text(&e["value"])?)))
                            .collect::<Result<_, DecodeError>>()?,
                        default: text(&l["default"])?,
                    })
                })
                .collect::<Result<_, DecodeError>>()?;
            Ok(Outcome { states, nbt, lists })
        };
        Ok(Self {
            block: value["block"]
                .as_str()
                .map(|b| super::super::tags::identifier(b).map(|i| i.to_string()))
                .ok_or(DecodeError::Field("pick block"))??,
            picks: value["picks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| Ok((below(p)?, outcome(p)?)))
                .collect::<Result<_, DecodeError>>()?,
            default: value.get("default").map(outcome).transpose()?,
            redraw: value["redraw"].as_bool().unwrap_or(false),
            keep_properties: value["keep_properties"].as_bool().unwrap_or(false),
        })
    }
}

impl CountCap {
    fn decode(
        value: &Value,
        state: impl Fn(&Value) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let count = |key: &'static str| {
            value[key]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(DecodeError::Field(key))
        };
        Ok(Self {
            blocks: value["blocks"]
                .as_array()
                .ok_or(DecodeError::Field("blocks"))?
                .iter()
                .map(|b| {
                    let b = b.as_str().ok_or(DecodeError::Field("blocks"))?;
                    Ok(super::super::tags::identifier(b)?.to_string())
                })
                .collect::<Result<_, DecodeError>>()?,
            min: count("min")?,
            max: count("max")?,
            chance: float(value, "chance")?,
            replacement: state(&value["replacement"])?,
        })
    }
}

impl SpawnerMobs {
    fn decode(
        value: &Value,
        replacement: impl FnOnce(Option<&Value>) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let mobs = value["mobs"]
            .as_array()
            .ok_or(DecodeError::Field("spawner_mobs mobs"))?
            .iter()
            .map(
                |m| match (json_to_nbt(&m["entity"]), m["weight"].as_f64()) {
                    (Tag::Compound(entity), Some(weight))
                        if entity.contains_key(&JavaString::from("id")) =>
                    {
                        Ok((entity, weight))
                    }
                    _ => Err(DecodeError::Field("spawner_mobs mob")),
                },
            )
            .collect::<Result<_, _>>()?;
        let mut fields = [0; 7];
        for (field, (key, _, default)) in fields.iter_mut().zip(SPAWNER_FIELDS) {
            // The mods' codecs read ints; the spawner saves them as shorts.
            *field = value
                .get(key)
                .map_or(Some(default), |v| v.as_i64().map(|v| v as i16))
                .ok_or(DecodeError::Field("spawner_mobs field"))?;
        }
        let rules = match value.get("custom_spawn_rules").map(json_to_nbt) {
            None => None,
            Some(Tag::Compound(c)) => Some(c),
            Some(_) => return Err(DecodeError::Field("spawner_mobs custom_spawn_rules")),
        };
        Ok(Self {
            mobs,
            fields,
            rules,
            replacement: replacement(value.get("replacement"))?,
        })
    }
}

/// A block holder set in any form `crate::holder_set` reads; a tag no pack
/// defines holds no blocks.
fn holder_set(
    value: &Value,
    bindings: &FeatureBlockBindings<'_>,
) -> Result<Arc<BTreeSet<BlockStateId>>, DecodeError> {
    let lookup = super::super::block_feature::BlockSetLookup {
        bindings,
        unknown_tags_empty: true,
    };
    let ids = match value {
        Value::Array(list) => {
            let mut ids = Vec::new();
            for value in list.iter().filter(|value| value.is_string()) {
                ids.extend(
                    crate::holder_set::resolve(value, &lookup).map_err(DecodeError::Registry)?,
                );
            }
            ids
        }
        _ => crate::holder_set::resolve(value, &lookup).map_err(DecodeError::Registry)?,
    };
    Ok(Arc::new(ids.into_iter().collect()))
}

fn float(value: &Value, key: &'static str) -> Result<f32, DecodeError> {
    value[key]
        .as_f64()
        .map(|v| v as f32)
        .ok_or(DecodeError::Field(key))
}

/// A required float field vanilla reads through `Codec.floatRange`: a value
/// outside the range, or a NaN, is refused at decode rather than clamped.
fn float_range(value: &Value, key: &'static str, min: f32, max: f32) -> Result<f32, DecodeError> {
    let v = float(value, key)?;
    if !(min..=max).contains(&v) {
        return Err(DecodeError::Field(key));
    }
    Ok(v)
}

impl Processor {
    fn decode(
        value: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        let kind = value["processor_type"]
            .as_str()
            .ok_or(DecodeError::Field("processor_type"))?;
        let state = |v: &Value| -> Result<BlockStateId, DecodeError> {
            bindings
                .codec
                .decode(&v.to_string(), false)
                .map_err(|e| DecodeError::Registry(e.to_string()))
        };
        Ok(match super::super::tags::identifier(kind)?.as_str() {
            "minecraft:nop" => Self::Nop,
            "minecraft:block_ignore" => Self::BlockIgnore(
                value["blocks"]
                    .as_array()
                    .ok_or(DecodeError::Field("block_ignore blocks"))?
                    .iter()
                    .map(|b| state(b).map(|id| resources.registry.state(id).unwrap().name.clone()))
                    .collect::<Result<_, _>>()?,
            ),
            "minecraft:block_rot" => Self::BlockRot {
                rottable: match value.get("rottable_blocks") {
                    Some(v) => Some(holder_set(v, bindings)?),
                    None => None,
                },
                integrity: float_range(value, "integrity", 0., 1.)?,
            },
            "minecraft:gravity" => Self::Gravity {
                heightmap: match value.get("heightmap").and_then(Value::as_str) {
                    Some(name) => HeightmapType::ALL
                        .into_iter()
                        .find(|k| k.name() == name)
                        .ok_or(DecodeError::Field("gravity heightmap"))?,
                    None => HeightmapType::WorldSurfaceWg,
                },
                offset: value.get("offset").and_then(Value::as_i64).unwrap_or(0) as i32,
            },
            "minecraft:protected_blocks" => Self::Protected(holder_set(&value["value"], bindings)?),
            "minecraft:jigsaw_replacement" => Self::JigsawReplacement,
            "minecraft:block_age" => Self::BlockAge(float(value, "mossiness")?),
            "minecraft:lava_submerged_block" => Self::LavaSubmerged,
            "minecraft:blackstone_replace" => Self::BlackstoneReplace,
            "minecraft:capped" => {
                // CappedProcessor's limit is IntProviders.POSITIVE_CODEC: a
                // provider that can draw 0 or less is refused at decode.
                let limit = IntProvider::from_json(&value["limit"].to_string())?;
                if limit.bounds()[0] < 1 {
                    return Err(DecodeError::Field("capped limit"));
                }
                Self::Capped {
                    delegate: Box::new(Self::decode(&value["delegate"], resources, bindings)?),
                    limit,
                }
            }
            "minecraft:rule" => Self::Rule(
                value["rules"]
                    .as_array()
                    .ok_or(DecodeError::Field("rules"))?
                    .iter()
                    .map(|rule| {
                        let test = |key| RuleTest::from_json(&rule[key].to_string(), bindings);
                        let position = PosRule::decode(rule.get("position_predicate"))?;
                        let modifier = Modifier::decode(rule.get("block_entity_modifier"))?;
                        Ok(Rule {
                            input: test("input_predicate")?,
                            location: test("location_predicate")?,
                            position,
                            output: state(&rule["output_state"])?,
                            modifier,
                        })
                    })
                    .collect::<Result<_, DecodeError>>()?,
            ),
            "sunrise:spawner_mobs" => {
                Self::SpawnerMobs(Box::new(SpawnerMobs::decode(value, |v| match v {
                    Some(v) => state(v),
                    None => Ok(resources.registry.default_state("minecraft:air").unwrap()),
                })?))
            }
            "sunrise:pick" => Self::Pick(Box::new(Pick::decode(value, state)?)),
            "lithostitched:block_swap" => Self::BlockSwap(
                value["blocks"]
                    .as_object()
                    .ok_or(DecodeError::Field("blocks"))?
                    .iter()
                    .map(|(from, to)| {
                        let to = to.as_str().ok_or(DecodeError::Field("blocks"))?;
                        Ok((
                            super::super::tags::identifier(from)?.to_string(),
                            super::super::tags::identifier(to)?.to_string(),
                        ))
                    })
                    .collect::<Result<_, DecodeError>>()?,
            ),
            "sunrise:count_cap" => Self::CountCap(Box::new(CountCap::decode(value, state)?)),
            "lithostitched:apply_random" => Self::ApplyRandom(Box::new(
                litho::ApplyRandom::decode(value, resources, bindings)?,
            )),
            "lithostitched:set_block" => {
                Self::SetBlock(Box::new(litho::SetBlock::decode(value, state)?))
            }
            "lithostitched:reference" => {
                Self::Reference(litho::reference(value, resources, bindings)?)
            }
            "lithostitched:discard_input" => Self::DiscardInput,
            "sunrise:pillar" => Self::Pillar(Box::new(pillars::Pillar::decode(
                value, resources, bindings, state,
            )?)),
            "sunrise:bottom_pillar" => Self::BottomPillar(Box::new(pillars::BottomPillar::decode(
                value, resources, bindings,
            )?)),
            "sunrise:close_off_fluid_sources" => Self::CloseOffFluidSources(Box::new(
                pillars::CloseOffFluidSources::decode(value, resources, bindings)?,
            )),
            "sunrise:close_off_air_sources" => Self::CloseOffAirSources(Box::new(
                pillars::CloseOffAirSources::decode(value, resources, bindings)?,
            )),
            "sunrise:yung_pillar" => Self::YungPillar(Box::new(pillars::YungPillar::decode(
                value, bindings, state,
            )?)),
            "sunrise:stair_pillar" => Self::StairPillar(Box::new(pillars::StairPillar::decode(
                value, resources, bindings,
            )?)),
            "sunrise:flood_air" => {
                Self::FloodAir(Box::new(sea_level::FloodAir::decode(resources.registry)?))
            }
            "sunrise:waterlog" => Self::Waterlog,
            "sunrise:waterlog_in_world_water" => Self::WaterlogInWorldWater,
            "sunrise:mineshaft_sky_view" => Self::MineshaftSkyView,
            "sunrise:keep_world_block" => {
                Self::KeepWorldBlock(Box::new(sea_level::KeepWorldBlock::decode(value)?))
            }
            "lithostitched:condition" => Self::Condition(Box::new(litho::Conditional::decode(
                value, resources, bindings,
            )?)),
            _ => return Err(DecodeError::Field("structure processor type")),
        })
    }
}

impl ProcessorList {
    /// A processor list holder: a registry name, `{"processors": [...]}` or a list.
    pub fn decode(
        json: &str,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        // Lists name lists (a pillar's, Lithostitched's): a cycle of names
        // is refused rather than followed.
        thread_local!(static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
        let depth = DEPTH.get();
        if depth >= 16 {
            return Err(DecodeError::Field("processor lists nest too deep"));
        }
        DEPTH.set(depth + 1);
        let decoded = Self::decode_at(json, resources, bindings);
        DEPTH.set(depth);
        decoded
    }
    fn decode_at(
        json: &str,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        let value: Value = serde_json::from_str(json)?;
        let value = match &value {
            Value::String(name) => {
                let name = super::super::tags::identifier(name)?;
                let raw = resources.processor_lists.get(&name).ok_or_else(|| {
                    DecodeError::Registry(format!("unknown processor list {name}"))
                })?;
                serde_json::from_str(raw)?
            }
            other => other.clone(),
        };
        let list = match &value {
            Value::Array(a) => a.clone(),
            Value::Object(_) => value["processors"].as_array().cloned().unwrap_or_default(),
            _ => return Err(DecodeError::Field("processor list")),
        };
        Ok(Self(
            list.iter()
                .map(|p| Processor::decode(p, resources, bindings))
                .collect::<Result<_, _>>()?,
        ))
    }
}
