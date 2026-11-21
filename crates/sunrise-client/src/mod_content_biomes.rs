//! stable-line: content-declarations
//! Biome modifiers: a content mod's placed features and mob spawns in biomes
//! it does not own (vanilla's), as NeoForge's `add_features`,
//! `remove_features`, `add_spawns`, `remove_spawns`, `add_spawn_costs` and
//! `remove_spawn_costs` biome modifiers do. Each is a file in
//! `data/<ns>/neoforge/biome_modifier/` (older Forge mods:
//! `data/<ns>/forge/biome_modifier/`, type `forge:...`).
//!
//! They rewrite the biomes' `features` lists and
//! `minecraft:gameplay/natural_mob_spawns` attributes when the content root
//! builds: an added feature goes at the end of its step's list, and the generator orders every biome's lists as vanilla's
//! `FeatureSorter` does; an added spawner goes at the end of its entity type's
//! spawn category with `minCount` and `maxCount` converted to 26.3's `count`.
//! Biomes no modifier names keep their text byte for byte, and a root with no
//! modifiers keeps vanilla's bundle, so vanilla features keep their indices
//! and their random seeds.
use super::{J, Result};
use serde::Deserialize;
use serde_json::{Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use sunrise_world::worldgen::structure::start::STEPS;



/// One mob spawner entry in an `add_spawns` modifier.
#[derive(Debug, PartialEq)]
pub(super) struct Spawner {
    pub entity: String,
    weight: i64,
    min_count: i64,
    max_count: i64,
}

impl Spawner {
    /// The canonical 26.3 weighted spawn entry, shared by biome and structure modifiers.
    pub(super) fn entry(&self) -> J {
        let count = if self.min_count == self.max_count {
            J::Num(self.min_count.into())
        } else {
            J::Obj(vec![
                ("type".into(), J::Str("minecraft:uniform".into())),
                ("min_inclusive".into(), J::Num(self.min_count.into())),
                ("max_inclusive".into(), J::Num(self.max_count.into())),
            ])
        };
        J::Obj(vec![
            ("type".into(), J::Str(self.entity.clone())),
            ("count".into(), count),
            ("weight".into(), J::Num(self.weight.into())),
        ])
    }
}

/// What a modifier does to its biomes.
#[derive(Debug, PartialEq)]
enum Action {
    /// Appends features to one generation step.
    AddFeatures { features: Value, step: usize },
    /// Takes features out of these steps (every step when the file names none).
    RemoveFeatures { features: Value, steps: Vec<usize> },
    /// Appends spawners to their entity types' spawn categories.
    AddSpawns(Vec<Spawner>),
    /// Takes these entity types out of every spawn category.
    RemoveSpawns(Value),
    /// Sets `spawn_costs` for these entity types.
    AddSpawnCosts {
        entity_types: Value,
        energy_budget: Number,
        charge: Number,
    },
    /// Takes these entity types out of `spawn_costs`.
    RemoveSpawnCosts(Value),
    /// Appends configured carvers to the biome's `carvers` (`add_carvers`).
    AddCarvers { carvers: Value },
    /// Takes configured carvers out of the biome's `carvers` (`remove_carvers`).
    RemoveCarvers { carvers: Value },
    /// Sets single values of the biome, in the MODIFY phase (`sunrise:modify_biome`).
    Modify(Changes),
}

/// The single values a `sunrise:modify_biome` modifier sets; what it leaves out stays.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Changes {
    temperature: Option<Number>,
    downfall: Option<Number>,
    has_precipitation: Option<bool>,
    temperature_modifier: Option<String>,
    /// `effects` colours by name, as 24-bit RGB; `None` removes the override.
    colours: Vec<(&'static str, Option<u32>)>,
    /// Whole `attributes` values (ambient particles, ambient sounds, music), checked.
    attributes: Vec<(&'static str, Value)>,
}










impl Action {
    fn is_modify(&self) -> bool {
        matches!(self, Self::Modify(_))
    }

    fn is_add(&self) -> bool {
        matches!(
            self,
            Self::AddFeatures { .. }
                | Self::AddSpawns(_)
                | Self::AddSpawnCosts { .. }
                | Self::AddCarvers { .. }
        )
    }
}

/// One biome modifier file.
#[derive(Debug)]
pub(super) struct Modifier {
    /// `<mod>: <file>`, for errors.
    name: String,
    biomes: Selector,
    action: Action,
    /// The mod, the file and its text, kept in the content root so the
    /// modifier is applied again over a world's data packs (`over_packs`).
    pub source: [String; 3],
}




/// `ns:path` for an id that may leave out `minecraft:`.
fn full(id: &str) -> String {
    match id.contains(':') {
        true => id.to_owned(),
        false => format!("minecraft:{id}"),
    }
}

/// The registries the modifiers name entries of.
pub(super) struct Registries<'a> {
    pub biome: Registry<'a>,
    pub feature: Registry<'a>,
    pub entity: Registry<'a>,
    pub carver: Registry<'a>,
}

impl<'a> Registries<'a> {
    /// The registries a content build or a world open names entries of.
    pub(super) fn of(
        biome_ids: &'a BTreeSet<String>,
        biome_tags: &'a BTreeMap<String, String>,
        placed: &'a BTreeSet<String>,
        feature_tags: &'a BTreeMap<String, String>,
        entities: (&'a BTreeSet<String>, &'a BTreeMap<String, String>),
        carvers: (&'a BTreeSet<String>, &'a BTreeMap<String, String>),
    ) -> Self {
        let registry = |what, ids, tags| Registry { what, ids, tags };
        Self {
            biome: registry("biome", biome_ids, biome_tags),
            feature: registry("placed feature", placed, feature_tags),
            entity: registry("entity type", entities.0, entities.1),
            carver: registry("carver", carvers.0, carvers.1),
        }
    }
}

/// The ids of the configured carvers in the bundle `path` (none when it is absent).
pub(super) fn carver_ids(path: &Path) -> Result<BTreeSet<String>> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    Ok(match super::read_json(path)? {
        J::Obj(entries) => entries.into_iter().map(|(id, _)| id).collect(),
        _ => BTreeSet::new(),
    })
}

/// A registry the modifiers name entries of: its ids and its tags (tag id →
/// the tag file's text), as the worldgen loader's bundles hold them.
pub(super) struct Registry<'a> {
    pub what: &'static str,
    pub ids: &'a BTreeSet<String>,
    pub tags: &'a BTreeMap<String, String>,
}

impl Registry<'_> {
    /// The ids a holder set names, in its order without repeats: an id,
    /// `#tag`, a list of ids, or one of NeoForge's `any`, `or`, `and` and
    /// `not` objects. Unknown ids and tags are errors; inside a tag, entries
    /// nobody registers are left out, as the worldgen loader leaves them out.
    pub(super) fn resolve(&self, set: &Value) -> std::result::Result<Vec<String>, String> {
        Selector::read(set.clone(), false)?.resolve(self, None)
    }

    /// A set with no climate in it, through the shared holder-set decoder.
    fn resolve_leaf(&self, set: &Value) -> std::result::Result<Vec<String>, String> {
        sunrise_world::holder_set::resolve(set, self)
    }


}

impl sunrise_world::holder_set::HolderLookup for Registry<'_> {
    type Id = String;
    fn what(&self) -> &str {
        self.what
    }
    fn entry(&self, id: &str) -> std::result::Result<Vec<String>, String> {
        let id = full(id);
        match self.ids.contains(&id) {
            true => Ok(vec![id]),
            false => Err(format!("unknown {} {id}", self.what)),
        }
    }
    fn tag(&self, tag: &str) -> std::result::Result<Vec<String>, String> {
        let mut out = Vec::new();
        self.expand(&full(tag), &mut out, &mut BTreeSet::new())?;
        Ok(out)
    }
    fn all(&self) -> std::result::Result<Vec<String>, String> {
        Ok(self.ids.iter().cloned().collect())
    }
}

/// Appends the ids `out` lacks.
fn extend(out: &mut Vec<String>, ids: impl IntoIterator<Item = String>) {
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
}




#[derive(Debug)]
enum Selector {
    Generic(Value),
    Climate(Climate),
    Or(Vec<Selector>),
    And(Vec<Selector>),
    Not(Box<Selector>),
}

impl Selector {

    fn resolve(
        &self,
        registry: &Registry<'_>,
        biomes: Option<&J>,
    ) -> std::result::Result<Vec<String>, String> {
        match self {
            Self::Generic(set) => registry.resolve_leaf(set),
            Self::Climate(climate) => {
                let Some(J::Obj(entries)) = biomes else {
                    return Err("biome bundle is not an object".into());
                };
                let mut selected = Vec::new();
                for (id, text) in entries {
                    let J::Str(text) = text else { continue };
                    let biome: Value =
                        serde_json::from_str(text).map_err(|e| format!("{id}: {e}"))?;
                    let mut matches = true;
                    for (field, range) in [
                        ("temperature", &climate.temperature),
                        ("downfall", &climate.downfall),
                    ] {
                        if let Some(range) = range {
                            let value = biome[field]
                                .as_f64()
                                .ok_or_else(|| format!("{id}: no {field}"))?;
                            matches &= range.min.is_none_or(|min| value >= min)
                                && range.max.is_none_or(|max| value <= max);
                        }
                    }
                    if matches {
                        selected.push(id.clone());
                    }
                }
                Ok(selected)
            }
            Self::Or(values) => {
                let mut out = Vec::new();
                for value in values {
                    extend(&mut out, value.resolve(registry, biomes)?);
                }
                Ok(out)
            }
            Self::And(values) => {
                let mut values = values.iter();
                let mut out = match values.next() {
                    Some(value) => value.resolve(registry, biomes)?,
                    None => Vec::new(),
                };
                for value in values {
                    let ids = value.resolve(registry, biomes)?;
                    out.retain(|id| ids.contains(id));
                }
                Ok(out)
            }
            Self::Not(value) => {
                let excluded = value.resolve(registry, biomes)?;
                Ok(registry
                    .ids
                    .iter()
                    .filter(|id| !excluded.contains(id))
                    .cloned()
                    .collect())
            }
        }
    }
}

/// Applies `modifiers` to the biome bundle `biomes` (id → the biome's text):
/// every addition first, then every removal, then every `sunrise:modify_biome`, each in mod and file order. A modifier naming a
/// biome, feature, entity type or tag the root doesn't have (another mod's, a
/// `#c:` convention tag) is skipped with a line in `warnings`, instead of dropping every content mod.
pub(super) fn apply(
    biomes: &mut J,
    modifiers: &[Modifier],
    registries: &Registries<'_>,
    entity_categories: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> Result<Vec<(String, String)>> {
    let Registries {
        biome: biome_reg,
        feature: feature_reg,
        entity: entity_reg,
        carver: carver_reg,
    } = registries;
    let adds = modifiers.iter().filter(|m| m.action.is_add());
    let removes = modifiers
        .iter()
        .filter(|m| !m.action.is_add() && !m.action.is_modify());
    // NeoForge's MODIFY phase follows ADD and REMOVE.
    let modifies = modifiers.iter().filter(|m| m.action.is_modify());
    // Each biome changed, parsed once, in the bundle's order.
    let mut changed: BTreeMap<String, J> = BTreeMap::new();
    for m in adds.chain(removes).chain(modifies) {
        let at = |e: String| format!("{}: {e}", m.name);
        let resolved: std::result::Result<(Vec<String>, Vec<String>), String> = m
            .biomes
            .resolve(biome_reg, Some(biomes))
            .and_then(|targets| {
                let ids = match &m.action {
                    Action::AddFeatures { features, .. }
                    | Action::RemoveFeatures { features, .. } => feature_reg.resolve(features)?,
                    Action::AddCarvers { carvers } | Action::RemoveCarvers { carvers } => {
                        carver_reg.resolve(carvers)?
                    }
                    Action::Modify(_) => Vec::new(),
                    Action::AddSpawns(spawners) => {
                        if let Some(s) = spawners
                            .iter()
                            .find(|s| !entity_reg.ids.contains(&s.entity))
                        {
                            return Err(format!("unknown {} {}", entity_reg.what, s.entity));
                        }
                        Vec::new()
                    }
                    Action::RemoveSpawns(entity_types)
                    | Action::AddSpawnCosts { entity_types, .. }
                    | Action::RemoveSpawnCosts(entity_types) => entity_reg.resolve(entity_types)?,
                };
                Ok((targets, ids))
            });
        let (targets, ids) = match resolved {
            Ok(both) => both,
            Err(e) => {
                warnings.push(at(format!("{e}: skipped")));
                continue;
            }
        };
        if matches!(&m.action, Action::AddSpawns(s) if s.is_empty()) {
            continue;
        }
        for biome in targets {
            let doc = match changed.entry(biome.clone()) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => {
                    let Some(J::Str(text)) = biomes.get(&biome) else {
                        continue;
                    };
                    e.insert(serde_json::from_str(text).map_err(|e| at(format!("{biome}: {e}")))?)
                }
            };
            if matches!(
                m.action,
                Action::AddCarvers { .. } | Action::RemoveCarvers { .. }
            ) {
                super::carver_registry::expand(doc, carver_reg)?;
            }
            edit(doc, &m.action, &ids, entity_categories)
                .map_err(|e| at(format!("{biome}: {e}")))?;
        }
    }
    let mut out = Vec::with_capacity(changed.len());
    for (id, doc) in changed {
        let text = serde_json::to_string(&doc)?;
        biomes.set(&id, J::Str(text.clone()));
        out.push((id, text));
    }
    Ok(out)
}

/// One modifier's change to one biome.
fn edit(
    biome: &mut J,
    action: &Action,
    ids: &[String],
    entity_categories: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    match action {
        Action::AddFeatures { step, .. } => {
            edit_features(biome, std::slice::from_ref(step), true, ids)
        }
        Action::RemoveFeatures { steps, .. } => edit_features(biome, steps, false, ids),
        Action::AddSpawns(spawners) => add_spawns(biome, spawners, entity_categories),
        Action::RemoveSpawns(_) => remove_spawns(biome, ids),
        Action::AddSpawnCosts {
            energy_budget,
            charge,
            ..
        } => add_spawn_costs(biome, ids, energy_budget, charge),
        Action::RemoveSpawnCosts(_) => remove_spawn_costs(biome, ids),
        Action::AddCarvers { .. } => edit_carvers(biome, true, ids),
        Action::RemoveCarvers { .. } => edit_carvers(biome, false, ids),
        Action::Modify(changes) => {
            modify(biome, changes);
            Ok(())
        }
    }
}

/// One `sunrise:modify_biome` modifier's change to one biome. In 26.3 the sky, fog and water
/// fog colours are the biome's `minecraft:visual/*` attributes; the others stay in `effects`.
fn modify(biome: &mut J, changes: &Changes) {
    if let Some(n) = &changes.temperature {
        biome.set("temperature", J::Num(n.clone()));
    }
    if let Some(n) = &changes.downfall {
        biome.set("downfall", J::Num(n.clone()));
    }
    if let Some(b) = changes.has_precipitation {
        biome.set("has_precipitation", J::Bool(b));
    }
    if let Some(m) = &changes.temperature_modifier {
        biome.set("temperature_modifier", J::Str(m.clone()));
    }
    for (name, rgb) in &changes.colours {
        let (section, key) = match *name {
            "sky_color" | "fog_color" | "water_fog_color" => {
                ("attributes", format!("minecraft:visual/{name}"))
            }
            _ => ("effects", (*name).to_owned()),
        };
        let Some(rgb) = rgb else {
            if let Some(map) = biome.get_mut(section) {
                map.remove(&key);
            }
            continue;
        };
        if biome.get(section).is_none() {
            biome.set(section, J::Obj(Vec::new()));
        }
        if let Some(map) = biome.get_mut(section) {
            map.set(&key, J::Str(format!("#{rgb:06x}")));
        }
    }
    for (key, value) in &changes.attributes {
        if biome.get("attributes").is_none() {
            biome.set("attributes", J::Obj(Vec::new()));
        }
        // A checked value always converts; the whole value replaces the biome's.
        if let (Some(map), Ok(value)) = (
            biome.get_mut("attributes"),
            serde_json::from_value::<J>(value.clone()),
        ) {
            map.set(key, value);
        }
    }
}

/// One carver modifier's change to one biome's `carvers` (a list of configured carver ids).
fn edit_carvers(biome: &mut J, add: bool, carvers: &[String]) -> std::result::Result<(), String> {
    if biome.get("carvers").is_none() {
        if !add {
            return Ok(());
        }
        biome.set("carvers", J::Arr(Vec::new()));
    }
    let Some(list) = biome.get_mut("carvers") else {
        return Ok(());
    };
    // The list may be one id; a tag cannot take more carvers.
    if let J::Str(one) = list {
        if one.starts_with('#') {
            return Err(format!("carvers is the carver tag {one}"));
        }
        *list = J::Arr(vec![J::Str(one.clone())]);
    }
    let J::Arr(list) = list else {
        return Err("carvers: not a list".into());
    };
    let held = |v: &J| match v {
        J::Str(s) => Some(full(s)),
        _ => None,
    };
    if add {
        for carver in carvers {
            if !list.iter().any(|v| held(v).as_deref() == Some(carver)) {
                list.push(J::Str(carver.clone()));
            }
        }
    } else {
        list.retain(|v| !held(v).is_some_and(|h| carvers.contains(&h)));
    }
    Ok(())
}

/// One feature modifier's change to one biome's `features` (a list per step).
fn edit_features(
    biome: &mut J,
    wanted: &[usize],
    add: bool,
    features: &[String],
) -> std::result::Result<(), String> {
    if biome.get("features").is_none() {
        biome.set("features", J::Arr(Vec::new()));
    }
    let Some(J::Arr(steps)) = biome.get_mut("features") else {
        return Err("features: not a list of steps".into());
    };
    if add && let Some(&step) = wanted.first() {
        while steps.len() <= step {
            steps.push(J::Arr(Vec::new()));
        }
    }
    for &i in wanted {
        let Some(list) = steps.get_mut(i) else {
            continue;
        };
        // A step may be one id; a tag cannot take more features.
        if let J::Str(one) = list {
            if one.starts_with('#') {
                return Err(format!("step {} is the feature tag {one}", STEPS[i]));
            }
            *list = J::Arr(vec![J::Str(one.clone())]);
        }
        let J::Arr(list) = list else {
            return Err(format!("step {}: not a list", STEPS[i]));
        };
        let held = |v: &J| match v {
            J::Str(s) => Some(full(s)),
            _ => None,
        };
        if add {
            for f in features {
                // Twice in one step would be a cycle for the sorter.
                if !list.iter().any(|v| held(v).as_ref() == Some(f)) {
                    list.push(J::Str(f.clone()));
                }
            }
        } else {
            list.retain(|v| held(v).is_none_or(|id| !features.contains(&id)));
        }
    }
    Ok(())
}

/// The `minecraft:gameplay/natural_mob_spawns` argument object on `biome`,
/// creating it when the biome does not yet have one.
fn natural_spawns(biome: &mut J) -> std::result::Result<&mut J, String> {
    if biome.get("attributes").is_none() {
        biome.set("attributes", J::Obj(Vec::new()));
    }
    let Some(attrs @ J::Obj(_)) = biome.get_mut("attributes") else {
        return Err("attributes: not an object".into());
    };
    const KEY: &str = "minecraft:gameplay/natural_mob_spawns";
    if attrs.get(KEY).is_none() {
        attrs.set(
            KEY,
            J::Obj(vec![
                (
                    "argument".into(),
                    J::Obj(vec![
                        ("spawn_costs".into(), J::Obj(Vec::new())),
                        ("spawns_by_category".into(), J::Obj(Vec::new())),
                    ]),
                ),
                ("modifier".into(), J::Str("overlay".into())),
            ]),
        );
    }
    let Some(nat) = attrs.get_mut(KEY) else {
        unreachable!()
    };
    if nat.get("argument").is_some() {
        match nat.get_mut("argument") {
            Some(arg @ J::Obj(_)) => Ok(arg),
            _ => Err(format!("{KEY}.argument: not an object")),
        }
    } else {
        match nat {
            J::Obj(_) => Ok(nat),
            _ => Err(format!("{KEY}: not an object")),
        }
    }
}

fn add_spawns(
    biome: &mut J,
    spawners: &[Spawner],
    entity_categories: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    let nat = natural_spawns(biome)?;
    if nat.get("spawns_by_category").is_none() {
        nat.set("spawns_by_category", J::Obj(Vec::new()));
    }
    let Some(by_cat @ J::Obj(_)) = nat.get_mut("spawns_by_category") else {
        return Err("spawns_by_category: not an object".into());
    };
    for s in spawners {
        let cat = entity_categories
            .get(&s.entity)
            .map_or("misc", String::as_str);
        if by_cat.get(cat).is_none() {
            by_cat.set(cat, J::Arr(Vec::new()));
        }
        let Some(J::Arr(list)) = by_cat.get_mut(cat) else {
            return Err(format!("spawns_by_category.{cat}: not a list"));
        };
        list.push(s.entry());
    }
    Ok(())
}

/// Remove entity ids from a canonical weighted spawn list.
pub(super) fn remove_entities(entries: &mut Vec<J>, entities: &[String]) {
    entries.retain(|entry| {
        entry
            .get("type")
            .and_then(J::as_str)
            .is_none_or(|t| !entities.contains(&full(t)))
    });
}

fn remove_spawns(biome: &mut J, entities: &[String]) -> std::result::Result<(), String> {
    let Some(nat) = biome
        .get_mut("attributes")
        .and_then(|a| a.get_mut("minecraft:gameplay/natural_mob_spawns"))
    else {
        return Ok(());
    };
    let nat = if nat.get("argument").is_some() {
        nat.get_mut("argument").unwrap()
    } else {
        nat
    };
    let Some(by_cat) = nat.get_mut("spawns_by_category") else {
        return Ok(());
    };
    let J::Obj(cats) = by_cat else {
        return Err("spawns_by_category: not an object".into());
    };
    for (cat, list) in cats {
        let J::Arr(entries) = list else {
            return Err(format!("spawns_by_category.{cat}: not a list"));
        };
        remove_entities(entries, entities);
    }
    Ok(())
}

fn add_spawn_costs(
    biome: &mut J,
    entities: &[String],
    energy_budget: &Number,
    charge: &Number,
) -> std::result::Result<(), String> {
    let nat = natural_spawns(biome)?;
    if nat.get("spawn_costs").is_none() {
        nat.set("spawn_costs", J::Obj(Vec::new()));
    }
    let Some(costs @ J::Obj(_)) = nat.get_mut("spawn_costs") else {
        return Err("spawn_costs: not an object".into());
    };
    for entity in entities {
        if let J::Obj(entries) = costs {
            entries.retain(|(k, _)| k == entity || full(k) != *entity);
        }
        costs.set(
            entity,
            J::Obj(vec![
                ("energy_budget".into(), J::Num(energy_budget.clone())),
                ("charge".into(), J::Num(charge.clone())),
            ]),
        );
    }
    Ok(())
}

fn remove_spawn_costs(biome: &mut J, entities: &[String]) -> std::result::Result<(), String> {
    let Some(nat) = biome
        .get_mut("attributes")
        .and_then(|a| a.get_mut("minecraft:gameplay/natural_mob_spawns"))
    else {
        return Ok(());
    };
    let nat = if nat.get("argument").is_some() {
        nat.get_mut("argument").unwrap()
    } else {
        nat
    };
    let Some(costs) = nat.get_mut("spawn_costs") else {
        return Ok(());
    };
    let J::Obj(entries) = costs else {
        return Err("spawn_costs: not an object".into());
    };
    entries.retain(|(k, _)| !entities.contains(&full(k)));
    Ok(())
}

/// Writes the state a content build saves for `biome_modifiers_over_packs` (the modifiers and
/// the registries packs leave alone) and, when a file modifier changed the biomes, the biome
/// bundle. Without file modifiers the bundle stands byte for byte; the saved state is what
/// modifiers declared in code are applied with at world open.
pub(super) fn write_state(
    overlay: &super::Overlay<'_>,
    saved: &super::SavedModifiers,
    doc: &J,
    modifiers: &[Modifier],
) -> Result<()> {
    overlay.write(super::MODIFIERS, &serde_json::to_vec(saved)?)?;
    if modifiers.is_empty() {
        return Ok(());
    }
    overlay.write(super::BIOMES, &serde_json::to_vec(doc)?)
}

/// The saved modifier state of the content root `root` (`write_state`), unless a world's packs
/// and the modifiers mods declared in code leave the root's biomes as they are: nothing
/// declared, and no pack brings a biome or a biome tag, or the root saved none.
pub(super) fn saved_bytes(
    root: &std::path::Path,
    packs: &sunrise_world::worldgen::overrides::WorldgenOverrides,
    declared: &[[String; 3]],
) -> Option<Vec<u8>> {
    if declared.is_empty()
        && packs.entries("biomes").next().is_none()
        && packs.tags("biome_tags").next().is_none()
        && packs.entries("carvers").next().is_none()
        && packs.tags("carver_tags").next().is_none()
    {
        return None;
    }
    std::fs::read(root.join(super::MODIFIERS)).ok()
}

/// The ids of the biomes the biome modifiers of the content root `root` changed
/// (`write_state`'s `unmodified`): a client has none of their changes in its own files.
pub fn modified(root: &Path) -> BTreeSet<String> {
    let saved = std::fs::read(root.join(super::MODIFIERS))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    saved
        .as_ref()
        .and_then(|saved| saved.get("unmodified")?.as_object())
        .map(|ids| ids.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "mod_content_biomes_tests.rs"]
mod removal_cycles;
