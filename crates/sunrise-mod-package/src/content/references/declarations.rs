// Only check these built-in tag folders. A mod-declared registry's folder is
// `<namespace>/<path>`, and a tag's own id may contain slashes, so the file path alone
// does not say where the registry folder ends.
const CHECKED_TAG_FOLDERS: [&str; 6] = [
    "block",
    "item",
    "fluid",
    "entity_type",
    "worldgen/biome",
    "worldgen/structure",
];

/// A file's text and value as the game's readers take it: without a byte-order mark, and, when
/// that does not parse, without comments.
fn parse(bytes: &[u8]) -> Result<(String, Value), serde_json::Error> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    match serde_json::from_str(text) {
        Ok(value) => Ok((text.to_owned(), value)),
        Err(error) => {
            let plain =
                String::from_utf8_lossy(&super::without_comments(text.as_bytes())).into_owned();
            serde_json::from_str(&plain)
                .map(|value| (plain, value))
                .map_err(|_| error)
        }
    }
}

/// Whether the file's own `neoforge:conditions` may disable it: the tool does not know which mods
/// and features the player has, so a file that carries them is not read.
fn conditional(value: &Value) -> bool {
    value.get("neoforge:conditions").is_some()
}

impl References<'_> {
    fn parsed(&mut self, name: &str) -> Result<Result<(String, Value), serde_json::Error>, String> {
        Ok(parse(&self.reader.read(name)?))
    }

    /// The text and value of a JSON file; an error finding when it is not JSON at all.
    fn json(&mut self, name: &str) -> Result<Option<(String, Value)>, String> {
        match self.parsed(name)? {
            Ok(json) => Ok(Some(json)),
            Err(e) => {
                self.findings.errors.push(format!(
                    "{name}: not valid JSON, so the game drops it: {e}."
                ));
                Ok(None)
            }
        }
    }

    /// A data file the load conditions do not disable (the tool cannot tell, so any file with
    /// `neoforge:conditions` is left alone).
    fn data(&mut self, name: &str) -> Result<Option<Value>, String> {
        Ok(self
            .json(name)?
            .map(|(_, value)| value)
            .filter(|value| !conditional(value)))
    }

    fn blockstate(&mut self, name: &str) -> Result<(), String> {
        let Some((text, _)) = self.json(name)? else {
            return Ok(());
        };
        // A file the decoder refuses, and a definition of a mod's own type, name no model here.
        let Ok(BlockStateFile::Vanilla(definition)) = BlockStateFile::from_json(&text) else {
            return Ok(());
        };
        let variants = definition
            .variants
            .iter()
            .flatten()
            .map(|(key, set)| (format!("variants[{key:?}]"), set));
        let multipart = definition
            .multipart
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, part)| (format!("multipart[{index}].apply"), &part.variant));
        for (field, set) in variants.chain(multipart) {
            match set {
                VariantSet::Single(variant) => {
                    self.missing(
                        name,
                        &format!("{field}.model"),
                        &variant.model,
                        Asset::Model,
                    );
                }
                VariantSet::Weighted(list) => {
                    for (index, (variant, _)) in list.iter().enumerate() {
                        self.missing(
                            name,
                            &format!("{field}[{index}].model"),
                            &variant.model,
                            Asset::Model,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Match the model library: read UTF-8 and let its decoder consume one model, not the whole file.
    fn read_model(&mut self, name: &str, namespace: &str, path: &str) -> Result<(), String> {
        let bytes = self.reader.read(name)?;
        let definition = match std::str::from_utf8(&bytes)
            .map_err(|error| error.to_string())
            .and_then(|text| CuboidModel::from_json(text).map_err(|error| error.to_string()))
        {
            Ok(model) => model,
            Err(error) => {
                self.findings.errors.push(format!(
                    "{name}: the game could not read this model: {error}. Fix the model JSON."
                ));
                return Ok(());
            }
        };
        self.models.insert(
            format!("{namespace}:{path}"),
            Model {
                file: name.to_owned(),
                definition,
            },
        );
        Ok(())
    }

    fn item_definition(&mut self, name: &str) -> Result<(), String> {
        let Some((text, _)) = self.json(name)? else {
            return Ok(());
        };
        let Ok(item) = ClientItem::from_json(&text) else {
            return Ok(());
        };
        let mut models = Vec::new();
        item.model.dependencies(&mut models);
        for model in models {
            self.missing(name, "model", &model, Asset::Model);
        }
        Ok(())
    }

    fn recipe(&mut self, name: &str) -> Result<(), String> {
        let Some(recipe) = self.data(name)? else {
            return Ok(());
        };
        let mut ids = Vec::new();
        recipe_ids(&recipe, &mut ids);
        // Do not try to reproduce the runtime's ordered filters over the merged recipe set.
        let uncertainty = self
            .recipe_filters
            .then_some("a local recipe filter may remove the recipe or replace this reference");
        for (field, id) in ids {
            self.id_in_registry(
                name,
                id,
                "item",
                &format!("the recipe item at {field}"),
                uncertainty,
            );
        }
        Ok(())
    }

    fn loot_table(&mut self, name: &str) -> Result<(), String> {
        let Some(table) = self.data(name)? else {
            return Ok(());
        };
        let mut found = Vec::new();
        loot_table_ids(&table, &mut found);
        for (registry, field, id) in found {
            self.id_in_registry(
                name,
                id,
                registry,
                &format!("the loot reference at {field}"),
                None,
            );
        }
        Ok(())
    }

    fn tag(&mut self, name: &str, tag: &str) -> Result<(), String> {
        let Some(registry) = CHECKED_TAG_FOLDERS.into_iter().find(|&folder| {
            tag.strip_prefix(folder)
                .is_some_and(|rest| rest.starts_with('/'))
        }) else {
            return Ok(());
        };
        let Some(value) = self.data(name)? else {
            return Ok(());
        };
        let Some(Value::Array(values)) = value.get("values") else {
            return Ok(());
        };
        for (index, entry) in values.iter().enumerate() {
            let (id, field) = match entry {
                Value::String(id) => (id, format!("values[{index}]")),
                // `{"id": ..., "required": false}` is allowed to name nothing, and
                // `{"id": ..., "remove": true}` takes an entry out.
                Value::Object(object)
                    if object.get("required").and_then(Value::as_bool) != Some(false)
                        && object.get("remove").and_then(Value::as_bool) != Some(true) =>
                {
                    match object.get("id") {
                        Some(Value::String(id)) => (id, format!("values[{index}].id")),
                        _ => continue,
                    }
                }
                _ => continue,
            };
            if let Some(tag) = id.strip_prefix('#') {
                self.tag_reference(name, &field, id, tag, registry);
            } else {
                self.id_in_registry(
                    name,
                    id,
                    registry,
                    &format!("the tag value at {field}"),
                    None,
                );
            }
        }
        Ok(())
    }

    /// Each `(language key, declaring file)` has a name in its namespace's `en_us.json`.
    fn names(&mut self, wanted: Vec<(String, String)>) -> Result<(), String> {
        // `None`: no file. `Some(None)`: a file the game's lenient reader may take and this
        // one does not; no key is asked of it.
        let mut by_namespace: BTreeMap<String, Option<Option<Value>>> = Default::default();
        for (key, file) in wanted {
            let namespace = file.split('/').nth(1).unwrap_or_default().to_owned();
            let lang = format!("assets/{namespace}/lang/en_us.json");
            if !by_namespace.contains_key(&namespace) {
                let value = if self.has(&lang) {
                    Some(self.parsed(&lang)?.ok().map(|(_, value)| value))
                } else {
                    None
                };
                by_namespace.insert(namespace.clone(), value);
            }
            let why = match &by_namespace[&namespace] {
                Some(Some(lang)) if lang.get(&key).is_some() => continue,
                Some(None) => continue,
                Some(Some(_)) => format!("{lang} has no {key:?}"),
                None => format!("there is no {lang}"),
            };
            if !self.jar {
                self.findings.warnings.push(format!(
                    "{file}: no local name found for {key} ({why}). Add the key to {lang} if another language source does not supply it."
                ));
            }
        }
        Ok(())
    }
}

fn type_of(object: &Value) -> Option<&str> {
    object.get("type").and_then(Value::as_str)
}

/// The item ids of a recipe, read as the recipe kinds the game knows read them: each kind's
/// ingredient fields and its `result`. A recipe of another kind (a mod's own, with fields only it
/// reads) and the data beside the ids (`components`, `group`, `category`) name nothing.
fn recipe_ids<'v>(recipe: &'v Value, out: &mut Vec<(String, &'v str)>) {
    let Some(kind) = type_of(recipe) else {
        return;
    };
    let (ingredients, keyed, result): (&[&str], bool, bool) =
        match kind.strip_prefix("minecraft:").unwrap_or(kind) {
            "crafting_shaped" => (&[], true, true),
            "crafting_shapeless" => (&["ingredients"], false, true),
            "smelting" | "blasting" | "smoking" | "campfire_cooking" | "stonecutting" => {
                (&["ingredient"], false, true)
            }
            "smithing_transform" => (&["template", "base", "addition"], false, true),
            "smithing_trim" => (&["template", "base", "addition"], false, false),
            "crafting_transmute" => (&["input", "material"], false, true),
            _ => return,
        };
    for field in ingredients {
        if let Some(ingredient) = recipe.get(field) {
            ingredient_ids(ingredient, field, out);
        }
    }
    if keyed && let Some(Value::Object(key)) = recipe.get("key") {
        for (symbol, ingredient) in key {
            ingredient_ids(ingredient, &format!("key[{symbol:?}]"), out);
        }
    }
    if result {
        match recipe.get("result") {
            Some(Value::String(id)) => out.push(("result".to_owned(), id)),
            Some(stack) => {
                if let Some(id) = stack.get("id").and_then(Value::as_str) {
                    out.push(("result.id".to_owned(), id));
                }
            }
            None => {}
        }
    }
}

/// The item ids an ingredient names: an id, a list of ingredients, or an ingredient type's own
/// fields (`neoforge:compound`, `difference`, `components`...). A `#tag` and a type only a mod
/// reads name no id.
fn ingredient_ids<'v>(ingredient: &'v Value, path: &str, out: &mut Vec<(String, &'v str)>) {
    match ingredient {
        Value::String(id) if !id.starts_with('#') => out.push((path.to_owned(), id)),
        Value::Array(list) => {
            for (index, entry) in list.iter().enumerate() {
                ingredient_ids(entry, &format!("{path}[{index}]"), out);
            }
        }
        Value::Object(object) => {
            let kind = object
                .get("neoforge:ingredient_type")
                .or_else(|| object.get("type"))
                .and_then(Value::as_str);
            let fields: &[&str] = match kind {
                // WalkedIngredient uses `ingredients` only if `children` is absent.
                Some("neoforge:compound" | "neoforge:intersection")
                    if object.contains_key("children") =>
                {
                    &["children"]
                }
                Some("neoforge:compound" | "neoforge:intersection") => &["ingredients"],
                Some("neoforge:difference") => &["base", "subtracted"],
                Some("neoforge:custom_display") => &["base"],
                Some("neoforge:components") => &["items"],
                // A brewing slot's item predicate wraps its ingredient.
                None => &["item"],
                Some(_) => &[],
            };
            for field in fields {
                if let Some(part) = object.get(*field) {
                    ingredient_ids(part, &format!("{path}.{field}"), out);
                }
            }
        }
        _ => {}
    }
}

type LootReference<'v> = (&'static str, &'static str, &'v str);

/// The `(registry, field, id)` a loot table names: an item entry's `name`, a state condition's
/// `block`, through the table's pools, entries and conditions. Conditional pools and a loot
/// function's payload (`set_components`, `set_custom_data`) are not checked.
fn loot_table_ids<'v>(table: &'v Value, out: &mut Vec<LootReference<'v>>) {
    list(table.get("functions"), |function| {
        conditions(function.get("conditions"), out);
    });
    list(table.get("pools"), |pool| {
        if conditional(pool) {
            return;
        }
        conditions(pool.get("conditions"), out);
        list(pool.get("functions"), |f| {
            conditions(f.get("conditions"), out)
        });
        entries(pool.get("entries"), out);
    });
}

/// `each` of a list, or of the one value a field holds in its place.
fn list<'v>(value: Option<&'v Value>, mut each: impl FnMut(&'v Value)) {
    match value {
        Some(Value::Array(values)) => values.iter().for_each(each),
        Some(value @ Value::Object(_)) => each(value),
        _ => {}
    }
}

fn entries<'v>(value: Option<&'v Value>, out: &mut Vec<LootReference<'v>>) {
    list(value, |entry| {
        conditions(entry.get("conditions"), out);
        list(entry.get("functions"), |f| {
            conditions(f.get("conditions"), out)
        });
        let kind = type_of(entry).map(|t| t.strip_prefix("minecraft:").unwrap_or(t));
        match (kind, entry.get("name").and_then(Value::as_str)) {
            (Some("item"), Some(name)) => out.push(("item", "entry.name", name)),
            (Some("alternatives" | "group" | "sequence"), _) => {
                entries(entry.get("children"), out);
            }
            _ => {}
        }
    });
}

fn conditions<'v>(value: Option<&'v Value>, out: &mut Vec<LootReference<'v>>) {
    list(value, |condition| {
        let kind = type_of(condition).map(|t| t.strip_prefix("minecraft:").unwrap_or(t));
        match kind {
            Some("block_state_property") => {
                if let Some(block) = condition.get("block").and_then(Value::as_str) {
                    out.push(("block", "condition.block", block));
                }
            }
            Some("inverted") => conditions(condition.get("term"), out),
            Some("all_of" | "any_of") => conditions(condition.get("terms"), out),
            _ => {}
        }
    });
}
