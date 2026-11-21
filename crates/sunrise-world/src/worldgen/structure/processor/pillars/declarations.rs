/// Direction's serialized names, in Direction.values() order.
const DIRECTION_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

fn direction(name: &str) -> Result<Direction, DecodeError> {
    let i = DIRECTION_NAMES
        .iter()
        .position(|n| *n == name)
        .ok_or(DecodeError::Field("direction"))?;
    Ok(Direction::ALL[i])
}

/// The processor list a pillar runs on each of its blocks: a list holder
/// (a name or the processors), none when absent or `minecraft:empty`.
fn nested(
    v: Option<&Value>,
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
) -> Result<Option<ProcessorList>, DecodeError> {
    match v {
        None => Ok(None),
        Some(Value::String(s)) if s == "minecraft:empty" => Ok(None),
        Some(v) => Ok(Some(ProcessorList::decode(
            &v.to_string(),
            resources,
            bindings,
        )?)),
    }
}

/// `weighted_list_of_replacement_blocks`: blocks by name with their weight,
/// at least one.
fn weighted_blocks(
    v: &Value,
    resources: &StructureResources,
) -> Result<Vec<(BlockStateId, i32)>, DecodeError> {
    let replacements = v["weighted_list_of_replacement_blocks"]
        .as_array()
        .ok_or(DecodeError::Field("weighted_list_of_replacement_blocks"))?
        .iter()
        .map(|e| {
            let name = e["block"].as_str().ok_or(DecodeError::Field("block"))?;
            let name = crate::worldgen::tags::identifier(name)?;
            let state = resources
                .registry
                .default_state(&name)
                .ok_or_else(|| DecodeError::Registry(format!("unknown block {name}")))?;
            let weight = e["weight"].as_i64().ok_or(DecodeError::Field("weight"))?;
            Ok((state, weight as i32))
        })
        .collect::<Result<Vec<_>, DecodeError>>()?;
    if replacements.is_empty() {
        return Err(DecodeError::Field("weighted_list_of_replacement_blocks"));
    }
    Ok(replacements)
}

impl HeightRandomizer {
    fn decode(
        v: &Value,
        state: &impl Fn(&Value) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let entries = v["entries"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|e| {
                let y = |k: &str| e.get(k).and_then(Value::as_i64).map(|y| y as i32);
                let p = e["probability"]
                    .as_f64()
                    .ok_or(DecodeError::Field("randomizer probability"))?;
                Ok((state(&e["block"])?, p as f32, y("min_y"), y("max_y")))
            })
            .collect::<Result<_, DecodeError>>()?;
        Ok(Self {
            entries,
            default: state(&v["default"])?,
        })
    }
}

impl YungRandomizer {
    fn decode(
        v: &Value,
        state: &impl Fn(&Value) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let entries = v["entries"]
            .as_array()
            .ok_or(DecodeError::Field("randomizer entries"))?
            .iter()
            .map(|e| {
                let p = e["probability"]
                    .as_f64()
                    .ok_or(DecodeError::Field("randomizer probability"))?;
                // yungsapi:altitude, the one condition the mods' pillars use.
                let (bottom, top) = match e.get("condition") {
                    None => (None, None),
                    Some(c) if c["type"] == "yungsapi:altitude" => {
                        (c["bottom_cutoff_y"].as_f64(), c["top_cutoff_y"].as_f64())
                    }
                    Some(_) => return Err(DecodeError::Field("randomizer condition")),
                };
                Ok((state(&e["blockState"])?, p as f32, bottom, top))
            })
            .collect::<Result<_, DecodeError>>()?;
        Ok(Self {
            entries,
            default: state(&v["defaultBlockState"])?,
        })
    }
}

impl Pillar {
    pub(super) fn decode(
        v: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
        state: impl Fn(&Value) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let triggers = v["pillar_trigger_and_replacements"]
            .as_array()
            .ok_or(DecodeError::Field("pillar_trigger_and_replacements"))?
            .iter()
            .map(|p| Ok((state(&p["trigger"])?, state(&p["replacement"])?)))
            .collect::<Result<_, DecodeError>>()?;
        Ok(Self {
            triggers,
            list: nested(v.get("pillar_processor_list"), resources, bindings)?,
            direction: direction(v["direction"].as_str().unwrap_or("down"))?,
            original: v.get("original_replaced_block").map(&state).transpose()?,
            length: v["pillar_length"].as_i64().unwrap_or(1000) as i32,
            forced: v["forced_placement"].as_bool().unwrap_or(false),
            randomizer: v
                .get("pillar_state_randomizer")
                .map(|r| HeightRandomizer::decode(r, &state))
                .transpose()?,
            blocks: Blocks::decode(bindings)?,
        })
    }
}

impl BottomPillar {
    pub(super) fn decode(
        v: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        Ok(Self {
            list: nested(v.get("pillar_processor_list"), resources, bindings)?,
            length: v["pillar_length"].as_i64().unwrap_or(1000) as i32,
            forced: v["forced_placement"].as_bool().unwrap_or(false),
            blocks: Blocks::decode(bindings)?,
        })
    }
}

impl CloseOffFluidSources {
    pub(super) fn decode(
        v: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        let replacements = weighted_blocks(v, resources)?;
        Ok(Self {
            replacements,
            ignore_down: v["ignore_down"].as_bool().unwrap_or(false),
            if_air: v["if_air_in_world"].as_bool().unwrap_or(false),
            blocks: Blocks::decode(bindings)?,
        })
    }
}

impl CloseOffAirSources {
    pub(super) fn decode(
        v: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        Ok(Self {
            replacements: weighted_blocks(v, resources)?,
            blocks: Blocks::decode(bindings)?,
        })
    }
}

impl YungPillar {
    pub(super) fn decode(
        v: &Value,
        bindings: &FeatureBlockBindings<'_>,
        state: impl Fn(&Value) -> Result<BlockStateId, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let target = state(&v["target_block"])?;
        Ok(Self {
            target: bindings.registry().state(target).unwrap().name.clone(),
            output: YungRandomizer::decode(&v["target_block_output"], &state)?,
            pillar: YungRandomizer::decode(&v["pillar_states"], &state)?,
            keep_properties: v["keep_properties"].as_bool().unwrap_or(false),
            blocks: Blocks::decode(bindings)?,
        })
    }
}

impl StairPillar {
    pub(super) fn decode(
        v: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        let id = |v: &Value| -> Result<String, DecodeError> {
            let name = v.as_str().ok_or(DecodeError::Field("stair_pillar block"))?;
            Ok(crate::worldgen::tags::identifier(name)?.to_string())
        };
        let blocks_to = v["blocks"]
            .as_object()
            .ok_or(DecodeError::Field("stair_pillar blocks"))?
            .iter()
            .map(|(from, to)| Ok((id(&Value::String(from.clone()))?, id(to)?)))
            .collect::<Result<_, DecodeError>>()?;
        let pillar = id(&v["pillar"])?;
        Ok(Self {
            blocks_to,
            front: id(&v["front"])?,
            pillar: resources
                .registry
                .default_state(&pillar)
                .ok_or_else(|| DecodeError::Registry(format!("unknown block {pillar}")))?,
            blocks: Blocks::decode(bindings)?,
        })
    }
}
