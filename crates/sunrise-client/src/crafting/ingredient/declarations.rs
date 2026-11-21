/// The kind an ingredient object names: `neoforge:ingredient_type` in
/// current NeoForge, `type` in older packs.
fn ingredient_kind(o: &serde_json::Map<String, Value>) -> Option<&str> {
    o.get("neoforge:ingredient_type")
        .or_else(|| o.get("type"))?
        .as_str()
}

/// A mod's own kind: a namespaced type that is not minecraft's.
fn is_custom(kind: &str) -> bool {
    kind.contains(':') && !kind.starts_with("minecraft:")
}

impl Ingredient {
    /// Asks the `ingredient` hook once for each mod kind in `v`, nested ones
    /// included, and leaves each answer with its occurrence for `parse`: the
    /// one place the hook is called, so every table that reads the recipes
    /// shares the same answers. A file's own answer is dropped first, so data
    /// cannot forge one. `true`: `v` changed.
    // hidden: the load's half of the ingredient hook, not part of the mod surface
    #[doc(hidden)]
    pub fn resolve(v: &mut Value, context: &LoadContext) -> bool {
        match v {
            Value::Array(entries) => entries
                .iter_mut()
                .fold(false, |changed, e| Self::resolve(e, context) | changed),
            Value::Object(o) => {
                let mut changed = o.remove(RESOLVED_ITEMS).is_some();
                let kind = ingredient_kind(o).map(str::to_owned);
                match kind.as_deref() {
                    Some("neoforge:compound" | "neoforge:intersection") => {
                        for key in ["children", "ingredients"] {
                            if let Some(children) = o.get_mut(key) {
                                changed |= Self::resolve(children, context);
                            }
                        }
                    }
                    Some("neoforge:difference") => {
                        for key in ["base", "subtracted"] {
                            if let Some(part) = o.get_mut(key) {
                                changed |= Self::resolve(part, context);
                            }
                        }
                    }
                    Some("neoforge:custom_display") => {
                        if let Some(base) = o.get_mut("base") {
                            changed |= Self::resolve(base, context);
                        }
                    }
                    Some("neoforge:components" | "neoforge:block_tag") => {}
                    Some(k) if is_custom(k) => {
                        let mut list = Vec::new();
                        if mod_api::__private::listened(mod_api::HookId::Ingredient)
                            && let Ok(data) = crate::json_nbt::json_to_compound(o)
                        {
                            native_ingredient(context, k, &data, &mut list);
                        }
                        dedup_keep_order(&mut list);
                        o.insert(RESOLVED_ITEMS.to_owned(), list.into());
                        changed = true;
                    }
                    Some(_) => {}
                    // A brewing slot's item predicate wraps its ingredient.
                    None => {
                        if let Some(item) = o.get_mut("item") {
                            changed |= Self::resolve(item, context);
                        }
                    }
                }
                changed
            }
            _ => false,
        }
    }

    /// Every mod kind named in `v`, nested ones included, with whether
    /// `resolve` already left an empty answer for it. Walks the same shapes
    /// `resolve` does.
    pub fn custom_kinds(v: &Value) -> Vec<(String, bool)> {
        let mut found = Vec::new();
        Self::collect_custom_kinds(v, &mut found);
        found
    }

    fn collect_custom_kinds(v: &Value, found: &mut Vec<(String, bool)>) {
        match v {
            Value::Array(entries) => entries
                .iter()
                .for_each(|e| Self::collect_custom_kinds(e, found)),
            Value::Object(o) => {
                let parts: &[&str] = match ingredient_kind(o) {
                    Some("neoforge:compound" | "neoforge:intersection") => {
                        &["children", "ingredients"]
                    }
                    Some("neoforge:difference") => &["base", "subtracted"],
                    Some("neoforge:custom_display") => &["base"],
                    Some("neoforge:components" | "neoforge:block_tag") => &[],
                    Some(k) if is_custom(k) => {
                        let empty = o
                            .get(RESOLVED_ITEMS)
                            .and_then(Value::as_array)
                            .is_some_and(Vec::is_empty);
                        found.push((k.to_owned(), empty));
                        &[]
                    }
                    Some(_) => &[],
                    None => &["item"],
                };
                for key in parts {
                    if let Some(part) = o.get(*key) {
                        Self::collect_custom_kinds(part, found);
                    }
                }
            }
            _ => {}
        }
    }
}

/// JSON optional record fields treat null as absent before the strict NBT boundary.
pub(super) fn json_patch(data: &serde_json::Map<String, Value>) -> Option<Compound> {
    const OPTIONAL_FIELDS: &[(&str, &[&str])] = &[
        (
            "weapon",
            &["item_damage_per_attack", "disable_blocking_for_seconds"],
        ),
        ("food", &["can_always_eat"]),
        (
            "custom_model_data",
            &["floats", "flags", "strings", "colors"],
        ),
        ("use_cooldown", &["cooldown_group"]),
        ("lodestone_tracker", &["target", "tracked"]),
        ("container_loot", &["seed"]),
        ("tooltip_display", &["hide_tooltip", "hidden_components"]),
        ("attack_animation", &["type", "duration"]),
        (
            "use_effects",
            &["can_sprint", "interact_vibrations", "speed_multiplier"],
        ),
        (
            "attack_range",
            &[
                "min_reach",
                "max_reach",
                "min_creative_reach",
                "max_creative_reach",
                "hitbox_margin",
                "mob_factor",
            ],
        ),
    ];
    let mut normalized = None;
    for (id, value) in data {
        let component = id.strip_prefix("minecraft:").unwrap_or(id);
        let optional = OPTIONAL_FIELDS
            .iter()
            .find(|(name, _)| *name == component)
            .map_or(&[][..], |(_, fields)| *fields);
        if let Some(record) = value.as_object() {
            // Integral JSON-to-NBT conversion would discard weapon's forbidden sign.
            if component == "weapon"
                && record
                    .get("disable_blocking_for_seconds")
                    .and_then(Value::as_f64)
                    .is_some_and(|seconds| seconds == 0.0 && seconds.is_sign_negative())
            {
                return None;
            }
            let mut normalized_record = None;
            for field in optional {
                if record.get(*field).is_some_and(Value::is_null) {
                    normalized_record
                        .get_or_insert_with(|| record.clone())
                        .remove(*field);
                }
            }
            if let Some(record) = normalized_record {
                normalized
                    .get_or_insert_with(|| data.clone())
                    .insert(id.clone(), Value::Object(record));
            }
        }
    }
    crate::json_nbt::json_to_compound(normalized.as_ref().unwrap_or(data)).ok()
}

// Keep conversion failures alongside codec errors at the recipe publication boundary.
pub(super) fn validate_recipe_components(value: &Value, items: &Items) -> Result<(), String> {
    let object = value.as_object().ok_or("malformed components ingredient")?;
    let keys = object
        .get("components")
        .and_then(Value::as_object)
        .map(|patch| {
            patch
                .keys()
                .map(|id| id.strip_prefix('!').unwrap_or(id).to_owned())
                .collect()
        })
        .unwrap_or_default();
    let ids = IngredientRule::Components {
        patch: Default::default(),
        removed: keys,
        strict: false,
    };
    validate(&ids, items)?;
    let conversion_error = std::cell::RefCell::new(None);
    let parsed = WalkedIngredient::walk(
        value,
        &items.item_tag_order,
        &|_| None,
        &resolved_items,
        &|data| {
            let mut converted = sunrise_core::nbt::Compound::new();
            for (id, value) in data {
                let single = serde_json::Map::from_iter([(id.clone(), value.clone())]);
                match json_patch(&single) {
                    Some(field) => converted.extend(field),
                    None => {
                        *conversion_error.borrow_mut() =
                            Some(format!("component {id}: malformed JSON payload"));
                        return None;
                    }
                }
            }
            Some(converted)
        },
    );
    if let Some(error) = conversion_error.into_inner() {
        return Err(error);
    }
    let rule = parsed
        .and_then(|parsed| parsed.rule)
        .ok_or_else(|| "malformed components ingredient".to_owned())?;
    validate(&rule, items)
}

pub(super) fn validate_recipe(value: &Value, items: &Items) -> Result<(), String> {
    match value {
        Value::Object(object) => match ingredient_kind(object) {
            Some("neoforge:components") => validate_recipe_components(value, items)?,
            Some("neoforge:compound" | "neoforge:intersection") => {
                for value in ["children", "ingredients"]
                    .iter()
                    .filter_map(|key| object.get(*key))
                {
                    validate_recipe(value, items)?;
                }
            }
            Some("neoforge:difference") => {
                for value in ["base", "subtracted"]
                    .iter()
                    .filter_map(|key| object.get(*key))
                {
                    validate_recipe(value, items)?;
                }
            }
            Some("neoforge:custom_display") => {
                if let Some(value) = object.get("base") {
                    validate_recipe(value, items)?;
                }
            }
            _ => {
                for value in crate::crafting::INGREDIENT_KEYS
                    .iter()
                    .filter_map(|key| object.get(*key))
                    .chain(
                        crate::crafting::INGREDIENT_MAPS
                            .iter()
                            .filter_map(|key| object.get(*key).and_then(Value::as_object))
                            .flat_map(|map| map.values()),
                    )
                {
                    validate_recipe(value, items)?;
                }
            }
        },
        Value::Array(values) => {
            for value in values {
                validate_recipe(value, items)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Validate only recipe/ingredient codecs; a native ingredient's own JSON is opaque.
pub(super) fn persistent_recipe(value: &Value) -> Result<(), String> {
    if !value["type"]
        .as_str()
        .is_some_and(|kind| kind.starts_with("minecraft:"))
    {
        return Ok(());
    }
    sunrise_world::item_defaults::validate_stack(&value["result"])?;
    for key in crate::crafting::INGREDIENT_KEYS {
        persistent_ingredient(&value[key])?;
    }
    for key in crate::crafting::INGREDIENT_MAPS {
        for ingredient in value[key]
            .as_object()
            .into_iter()
            .flat_map(|map| map.values())
        {
            persistent_ingredient(ingredient)?;
        }
    }
    Ok(())
}

fn persistent_ingredient(value: &Value) -> Result<(), String> {
    if let Some(values) = value.as_array() {
        for value in values {
            persistent_ingredient(value)?;
        }
    } else if let Some(object) = value.as_object() {
        match ingredient_kind(object) {
            Some("neoforge:components") => {
                sunrise_world::item_defaults::validate_component_map(&value["components"])?
            }
            Some("neoforge:compound" | "neoforge:intersection") => {
                for key in ["children", "ingredients"] {
                    persistent_ingredient(&value[key])?;
                }
            }
            Some("neoforge:difference") => {
                persistent_ingredient(&value["base"])?;
                persistent_ingredient(&value["subtracted"])?;
            }
            Some("neoforge:custom_display") => {
                persistent_ingredient(&value["base"])?;
                persistent_display(&value["display"])?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn persistent_display(value: &Value) -> Result<(), String> {
    match value["type"].as_str().unwrap_or("") {
        "minecraft:item_stack" => sunrise_world::item_defaults::validate_stack(&value["item"])?,
        "minecraft:composite" => {
            for display in value["contents"].as_array().into_iter().flatten() {
                persistent_display(display)?;
            }
        }
        "minecraft:with_any_potion" | "minecraft:only_with_component" => {
            persistent_display(&value["contents"])?
        }
        "minecraft:dyed" => persistent_display(&value["target"])?,
        "minecraft:smithing_trim" => {
            for key in ["base", "material", "pattern"] {
                persistent_display(&value[key])?;
            }
        }
        "minecraft:with_remainder" => {
            persistent_display(&value["input"])?;
            persistent_display(&value["remainder"])?;
        }
        _ => {}
    }
    Ok(())
}
