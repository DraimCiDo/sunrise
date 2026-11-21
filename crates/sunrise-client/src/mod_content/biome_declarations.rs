/// The folders a biome modifier sits in, under `data/<ns>`.
const FOLDERS: [&str; 2] = ["/neoforge/biome_modifier/", "/forge/biome_modifier/"];

/// Whether a mod file is a biome modifier.
pub(super) fn is_modifier(file: &str) -> bool {
    file.starts_with("data/") && file.ends_with(".json") && FOLDERS.iter().any(|f| file.contains(f))
}

/// The `attributes` that `sunrise:modify_biome` sets, each replacing the biome's value.
const ATTRIBUTES: [&str; 4] = [
    "minecraft:visual/ambient_particles",
    "minecraft:audio/ambient_sounds",
    "minecraft:audio/background_music",
    "minecraft:audio/music_volume",
];

/// A namespaced id (`minecraft:ash`).
fn id(s: &str) -> bool {
    s.split_once(':')
        .is_some_and(|(ns, path)| !ns.is_empty() && !path.is_empty())
}

/// A whole number of ticks or blocks, as an `i32`.
fn count(v: Option<&Value>) -> Option<u64> {
    v?.as_u64().filter(|n| i32::try_from(*n).is_ok())
}

/// A chance or volume, 0.0..=1.0.
fn unit(v: Option<&Value>) -> bool {
    v.and_then(Value::as_f64)
        .is_some_and(|n| (0.0..=1.0).contains(&n))
}

/// One attribute's value as vanilla's biome JSON writes it, or why it is refused. Ambient
/// particles may come in the `{"argument": [...]}` form; the list is what is kept.
fn attribute(key: &str, v: &Value) -> std::result::Result<Value, String> {
    let sound = |m: &Value| m.get("sound").and_then(Value::as_str).is_some_and(id);
    let bad = |what: &str| format!("attributes.{key}: {what}");
    match key {
        "minecraft:visual/ambient_particles" => {
            let list = v.get("argument").unwrap_or(v);
            let entries = list.as_array().ok_or_else(|| bad("not a list"))?;
            let entry = |e: &Value| {
                let particle = e["particle"]["type"].as_str();
                particle.is_some_and(id) && unit(e.get("probability"))
            };
            match entries.iter().all(entry) {
                true => Ok(list.clone()),
                false => Err(bad("a particle needs a type and a probability 0..=1")),
            }
        }
        "minecraft:audio/ambient_sounds" => {
            let object = v.as_object().ok_or_else(|| bad("not an object"))?;
            let addition = |a: &Value| {
                sound(a)
                    && a.get("tick_chance")
                        .and_then(Value::as_f64)
                        .is_some_and(f64::is_finite)
            };
            for (name, value) in object {
                let ok = match name.as_str() {
                    "loop" => value.as_str().is_some_and(id),
                    "additions" => match value {
                        Value::Array(list) => list.iter().all(addition),
                        one => addition(one),
                    },
                    "mood" => {
                        sound(value)
                            && count(value.get("tick_delay")).is_some()
                            && count(value.get("block_search_extent")).is_some()
                            && value.get("offset").is_some_and(Value::is_number)
                    }
                    _ => false,
                };
                if !ok {
                    return Err(bad(&format!("{name} is not valid")));
                }
            }
            Ok(v.clone())
        }
        "minecraft:audio/background_music" => {
            let object = v.as_object().ok_or_else(|| bad("not an object"))?;
            for (name, music) in object {
                let (min, max) = (count(music.get("min_delay")), count(music.get("max_delay")));
                let ok = matches!(name.as_str(), "default" | "creative" | "underwater")
                    && sound(music)
                    && min.is_some()
                    && max.is_some();
                if !ok {
                    return Err(bad(&format!("{name} needs a sound and nonnegative delays")));
                }
            }
            Ok(v.clone())
        }
        _ => match unit(Some(v)) {
            true => Ok(v.clone()),
            false => Err(bad("not a number 0..=1")),
        },
    }
}

/// The colours of `effects` that `sunrise:modify_biome` sets.
const COLOURS: [&str; 7] = [
    "sky_color",
    "fog_color",
    "water_color",
    "water_fog_color",
    "foliage_color",
    "dry_foliage_color",
    "grass_color",
];

/// The colours that are optional in 26.3's `BiomeSpecialEffects`: a `null` removes them. The
/// others are required or are attributes, so they cannot be removed.
const REMOVABLE: [&str; 3] = ["foliage_color", "dry_foliage_color", "grass_color"];

/// A colour as an integer 0..=0xFFFFFF or a `#rrggbb` string.
fn colour(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .filter(|c| *c <= 0xFF_FFFF)
            .and_then(|c| u32::try_from(c).ok()),
        Value::String(s) => s
            .strip_prefix('#')
            .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|h| u32::from_str_radix(h, 16).ok()),
        _ => None,
    }
}

impl Changes {
    /// The values in a `sunrise:modify_biome` file, vanilla biome JSON's names.
    fn read(doc: &Value) -> std::result::Result<Self, String> {
        let num = |k: &str| match doc.get(k) {
            None => Ok(None),
            Some(Value::Number(n)) => Ok(Some(n.clone())),
            Some(_) => Err(format!("{k}: not a number")),
        };
        let has_precipitation = match doc.get("has_precipitation") {
            None => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(_) => return Err("has_precipitation: not a boolean".into()),
        };
        let temperature_modifier = match doc.get("temperature_modifier") {
            None => None,
            Some(Value::String(s)) if s == "none" || s == "frozen" => Some(s.clone()),
            Some(_) => return Err("temperature_modifier: not none or frozen".into()),
        };
        let mut colours = Vec::new();
        match doc.get("effects") {
            None => {}
            Some(Value::Object(effects)) => {
                for (key, value) in effects {
                    let name = COLOURS
                        .iter()
                        .find(|c| *c == key)
                        .ok_or_else(|| format!("effects: unknown colour {key}"))?;
                    let rgb =
                        match value {
                            Value::Null if REMOVABLE.contains(name) => None,
                            Value::Null => return Err(format!("effects.{key}: cannot be removed")),
                            value => Some(colour(value).ok_or_else(|| {
                                format!("effects.{key}: not a colour 0..=0xFFFFFF")
                            })?),
                        };
                    colours.push((*name, rgb));
                }
            }
            Some(_) => return Err("effects: not an object".into()),
        }
        let mut attributes = Vec::new();
        match doc.get("attributes") {
            None => {}
            Some(Value::Object(map)) => {
                for (key, value) in map {
                    let name = ATTRIBUTES
                        .iter()
                        .find(|a| *a == key)
                        .ok_or_else(|| format!("attributes: unknown attribute {key}"))?;
                    attributes.push((*name, attribute(key, value)?));
                }
            }
            Some(_) => return Err("attributes: not an object".into()),
        }
        Ok(Self {
            temperature: num("temperature")?,
            downfall: num("downfall")?,
            has_precipitation,
            temperature_modifier,
            colours,
            attributes,
        })
    }
}

/// A generation step's index from its name.
fn step(v: &Value) -> std::result::Result<usize, String> {
    let name = v.as_str().ok_or("step: not a name")?;
    STEPS
        .iter()
        .position(|s| *s == name)
        .ok_or_else(|| format!("unknown generation step {name:?}"))
}

/// One NeoForge `SpawnerData` object from `spawners`.
pub(super) fn spawner(v: &Value) -> std::result::Result<Spawner, String> {
    let o = v.as_object().ok_or("spawner: not an object")?;
    let entity = o
        .get("type")
        .and_then(Value::as_str)
        .ok_or("spawner: no type")?;
    let int = |k: &str| {
        o.get(k)
            .and_then(Value::as_i64)
            .filter(|value| i32::try_from(*value).is_ok())
            .ok_or_else(|| format!("spawner: no {k}"))
    };
    let weight = int("weight")?;
    let min_count = int("minCount")?;
    let max_count = int("maxCount")?;
    if weight < 0 {
        return Err(format!("spawner: weight ({weight}) < 0"));
    }
    if min_count <= 0 {
        return Err(format!("spawner: minCount ({min_count}) must be positive"));
    }
    if min_count > max_count {
        return Err(format!(
            "spawner: minCount ({min_count}) > maxCount ({max_count})"
        ));
    }
    Ok(Spawner {
        entity: full(entity),
        weight,
        min_count,
        max_count,
    })
}

impl Modifier {
    /// The modifier in `text`; `None` for `neoforge:none`, which does nothing,
    /// and for a type Sunrise doesn't read yet (a mod's own
    /// serializer), which is skipped with a line in `warnings`: real mods ship
    /// those beside their feature and spawn modifiers, and one must not drop
    /// every content mod.
    pub(super) fn read(
        mod_id: &str,
        file: &str,
        text: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Option<Self>> {
        let name = format!("mod {mod_id}: {file}");
        let at = |e: String| format!("{name}: {e}");
        let doc: Value = serde_json::from_str(text).map_err(|e| at(e.to_string()))?;
        let kind = doc["type"].as_str().ok_or_else(|| at("no type".into()))?;
        let kind = kind
            .strip_prefix("neoforge:")
            .or_else(|| kind.strip_prefix("forge:"))
            .unwrap_or(kind);
        let field = |key: &str| -> std::result::Result<Value, String> {
            doc.get(key).cloned().ok_or_else(|| at(format!("no {key}")))
        };
        let action = match kind {
            "none" => return Ok(None),
            "add_features" => Action::AddFeatures {
                features: field("features")?,
                step: step(&field("step")?).map_err(at)?,
            },
            "remove_features" => Action::RemoveFeatures {
                features: field("features")?,
                steps: match doc.get("steps") {
                    None => (0..STEPS.len()).collect(),
                    Some(Value::Array(steps)) => steps
                        .iter()
                        .map(step)
                        .collect::<std::result::Result<_, _>>()
                        .map_err(at)?,
                    Some(one) => vec![step(one).map_err(at)?],
                },
            },
            "add_spawns" => {
                let raw = field("spawners")?;
                let spawners = match &raw {
                    Value::Array(list) => list
                        .iter()
                        .map(spawner)
                        .collect::<std::result::Result<_, _>>()
                        .map_err(at)?,
                    one @ Value::Object(_) => vec![spawner(one).map_err(at)?],
                    _ => return Err(at("spawners: not a spawner or list".into()).into()),
                };
                Action::AddSpawns(spawners)
            }
            "remove_spawns" => Action::RemoveSpawns(field("entity_types")?),
            "add_spawn_costs" => {
                let entity_types = field("entity_types")?;
                let cost = field("spawn_cost")?;
                let num = |k: &str| {
                    cost.get(k)
                        .and_then(Value::as_number)
                        .cloned()
                        .ok_or_else(|| at(format!("spawn_cost: no {k}")))
                };
                Action::AddSpawnCosts {
                    entity_types,
                    energy_budget: num("energy_budget")?,
                    charge: num("charge")?,
                }
            }
            "remove_spawn_costs" => Action::RemoveSpawnCosts(field("entity_types")?),
            "add_carvers" => Action::AddCarvers {
                carvers: field("carvers")?,
            },
            "remove_carvers" => Action::RemoveCarvers {
                carvers: field("carvers")?,
            },
            "sunrise:modify_biome" => match Changes::read(&doc) {
                Ok(changes) => Action::Modify(changes),
                Err(e) => {
                    warnings.push(at(format!("{e}: skipped")));
                    return Ok(None);
                }
            },
            other => {
                warnings.push(at(format!(
                    "biome modifier type {other:?} skipped: only feature, spawn, carver, \
                     sunrise:modify_biome and none modifiers are read"
                )));
                return Ok(None);
            }
        };
        Ok(Some(Self {
            biomes: Selector::read(field("biomes")?, true).map_err(at)?,
            action,
            name,
            source: [mod_id.to_owned(), file.to_owned(), text.to_owned()],
        }))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Range {
    #[serde(default, deserialize_with = "present")]
    min: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    max: Option<f64>,
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Climate {
    #[serde(rename = "type")]
    _kind: String,
    #[serde(default, deserialize_with = "present")]
    temperature: Option<Range>,
    #[serde(default, deserialize_with = "present")]
    downfall: Option<Range>,
}

impl Selector {
    fn read(mut set: Value, allow_climate: bool) -> std::result::Result<Self, String> {
        let Value::Object(object) = &mut set else {
            return Ok(Self::Generic(set));
        };
        match object.get("type").and_then(Value::as_str) {
            Some("sunrise:climate") if allow_climate => {
                let climate: Climate = serde_json::from_value(set).map_err(|e| e.to_string())?;
                if climate.temperature.is_none() && climate.downfall.is_none() {
                    return Err("climate: temperature or downfall required".into());
                }
                for range in [&climate.temperature, &climate.downfall]
                    .into_iter()
                    .flatten()
                {
                    if range.min.is_none() && range.max.is_none()
                        || matches!((range.min, range.max), (Some(a), Some(b)) if a > b)
                    {
                        return Err("climate: invalid range".into());
                    }
                }
                Ok(Self::Climate(climate))
            }
            Some("neoforge:or" | "or" | "neoforge:and" | "and")
                if object.get("values").is_some_and(Value::is_array) =>
            {
                let Value::Array(values) = object.remove("values").unwrap() else {
                    unreachable!()
                };
                let is_or = matches!(
                    object.get("type").and_then(Value::as_str),
                    Some("or" | "neoforge:or")
                );
                let values = values
                    .into_iter()
                    .map(|v| Self::read(v, allow_climate))
                    .collect::<std::result::Result<_, _>>()?;
                Ok(if is_or {
                    Self::Or(values)
                } else {
                    Self::And(values)
                })
            }
            Some("neoforge:not" | "not") if object.contains_key("value") => {
                let value = object.remove("value").unwrap();
                Ok(Self::Not(Box::new(Self::read(value, allow_climate)?)))
            }
            _ => Ok(Self::Generic(set)),
        }
    }
}
