use super::{Limit, Pool, PoolElement, Pools, Projection, Rules};
use crate::worldgen::{
    DecodeError,
    block_feature::FeatureBlockBindings,
    structure::{StructureResources, enhanced_terrain, processor::ProcessorList},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, OnceLock},
};

fn projection(value: &Value) -> Result<Projection, DecodeError> {
    match value["projection"].as_str() {
        Some("rigid") => Ok(Projection::Rigid),
        Some("terrain_matching") => Ok(Projection::TerrainMatching),
        _ => Err(DecodeError::Field("pool element projection")),
    }
}

impl PoolElement {
    pub(super) fn decode(
        value: &Value,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
        inherited: Option<Projection>,
    ) -> Result<Self, DecodeError> {
        let kind = value["element_type"].as_str().unwrap_or_default();
        let projection = || match inherited {
            Some(p) => Ok(p),
            None => projection(value),
        };
        Ok(match kind {
            "minecraft:empty_pool_element" => Self::Empty,
            // Lithostitched's elements place what their `delegate` places;
            // their limits are the pool entry's (`Pool::limits`).
            "lithostitched:limited" | "lithostitched:guaranteed" | "lithostitched:delegating" => {
                Self::decode(&value["delegate"], resources, bindings, inherited)?
            }
            // YUNG's API's element places what a single element places; its
            // own fields are the pool entry's rules (`Pool::rules`).
            "minecraft:single_pool_element"
            | "minecraft:legacy_single_pool_element"
            | "yungsapi:yung_single_element" => {
                let location = value["location"]
                    .as_str()
                    .ok_or(DecodeError::Field("location"))?;
                Self::Single {
                    template: resources.lazy_template(location),
                    processors: ProcessorList::decode(
                        &value["processors"].to_string(),
                        resources,
                        bindings,
                    )?,
                    processors_source: value["processors"].clone(),
                    legacy: kind == "minecraft:legacy_single_pool_element",
                    projection: projection()?,
                    waterlogging: match value
                        .get("override_liquid_settings")
                        .and_then(Value::as_str)
                    {
                        Some("apply_waterlogging") => Some(true),
                        Some("ignore_waterlogging") => Some(false),
                        _ => None,
                    },
                    enhanced_terrain: value
                        .get("sunrise:enhanced_terrain_adaptation")
                        .map(|a| enhanced_terrain::Adaptation::decode(a).map(Arc::new))
                        .transpose()?,
                    ground_level: match value.get("sunrise:ground_level") {
                        None => 1,
                        Some(v) => v
                            .as_u64()
                            .and_then(|g| i32::try_from(g).ok())
                            .ok_or(DecodeError::Field("sunrise:ground_level"))?,
                    },
                }
            }
            "minecraft:feature_pool_element" => Self::Feature {
                feature: value["feature"].as_str().unwrap_or_default().to_owned(),
                projection: projection()?,
            },
            "minecraft:list_pool_element" => {
                let p = projection()?;
                Self::List {
                    elements: value["elements"]
                        .as_array()
                        .ok_or(DecodeError::Field("list elements"))?
                        .iter()
                        .map(|e| Self::decode(e, resources, bindings, Some(p)))
                        .collect::<Result<_, _>>()?,
                    projection: p,
                }
            }
            _ => return Err(DecodeError::Field("pool element type")),
        })
    }
}

impl Rules {
    pub(super) fn of(element: &Value) -> Self {
        if element["element_type"] != "yungsapi:yung_single_element" {
            return Self::default();
        }
        Self {
            limit: element["name"]
                .as_str()
                .zip(element["max_count"].as_i64())
                .map(|(name, max)| (name.into(), max as i32)),
            priority: element["is_priority"].as_bool().unwrap_or(false),
            min_depth: element["min_required_depth"].as_i64().unwrap_or(0) as i32,
            ignore_bounds: element["ignore_bounds"].as_bool().unwrap_or(false),
        }
    }
}

impl Limit {
    pub(super) fn of(element: &Value) -> Option<Self> {
        let int = |key: &str| {
            element[key]
                .as_i64()
                .map(|v| v.clamp(0, i32::MAX.into()) as i32)
        };
        let (max, guaranteed) = match element["element_type"].as_str()? {
            "lithostitched:limited" => (int("limit")?, false),
            "lithostitched:guaranteed" => (int("count").unwrap_or(1), true),
            _ => return None,
        };
        Some(Self {
            key: element.to_string().into(),
            max,
            min_depth: int("min_depth").unwrap_or(0),
            guaranteed,
        })
    }
}

impl Pools {
    pub fn get(
        &self,
        name: &str,
        resources: &StructureResources,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Option<Arc<Pool>> {
        if let Some(found) = self.decoded.read().unwrap().get(name) {
            return found.clone();
        }
        let pool = self.raw.get(name).and_then(|json| {
            let value: Value = serde_json::from_str(json).ok()?;
            let (mut templates, mut entries, mut raw, mut rules, mut limits) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for entry in value["elements"].as_array()? {
                let element =
                    match PoolElement::decode(&entry["element"], resources, bindings, None) {
                        Ok(element) => Arc::new(element),
                        Err(error) => {
                            // Vanilla refuses the data pack; here every structure using the
                            // pool stops generating, so the reason is the only clue a mod author gets.
                            // Once per pool: each worker decodes its own copy.
                            static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
                            if WARNED.lock().unwrap().insert(name.to_owned()) {
                                log::warn!("Sunrise: template pool {name} is left out: {error}");
                            }
                            return None;
                        }
                    };
                let weight = entry["weight"].as_i64().unwrap_or(1);
                for _ in 0..weight {
                    templates.push(element.clone());
                    entries.push(raw.len());
                }
                raw.push((element, weight as i32));
                rules.push(Rules::of(&entry["element"]));
                limits.push(Limit::of(&entry["element"]));
            }
            let guaranteed = (limits.iter().enumerate())
                .filter(|(_, l)| l.as_ref().is_some_and(|l| l.guaranteed))
                .map(|(i, _)| i)
                .collect();
            Some(Arc::new(Pool {
                templates,
                entries,
                raw,
                rules,
                limits,
                guaranteed,
                fallback: value["fallback"]
                    .as_str()
                    .unwrap_or("minecraft:empty")
                    .to_owned(),
                max_size: OnceLock::new(),
            }))
        });
        self.decoded
            .write()
            .unwrap()
            .insert(name.to_owned(), pool.clone());
        pool
    }
}
