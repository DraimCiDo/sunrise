impl LoadConditions {
    /// `ConditionalOps.decodeListWithElementConditions` on one list: drop an
    /// element whose own `neoforge:conditions` fail and strip the key from one
    /// whose conditions hold. Only the lists NeoForge decodes that way carry
    /// element conditions (a loot table's pools, data-map values, overlays and
    /// atlas sources, each read by its own reader); anywhere else a nested
    /// `neoforge:conditions` key is ordinary data and is never evaluated.
    fn filter_list(&self, elements: &mut Vec<Value>, path: &dyn Fn() -> String) -> bool {
        let mut valid = true;
        elements.retain_mut(|element| match self.registry_verdict(element, path) {
            Some(true) => {
                if let Some(fields) = element.as_object_mut() {
                    fields.remove("neoforge:conditions");
                }
                true
            }
            Some(false) => false,
            None => {
                valid = false;
                false
            }
        });
        valid
    }

    /// `allows_id` for a loot table, then its `pools` filtered by their own
    /// conditions: NeoForge decodes a table's pool list with element conditions
    /// (a false pool is dropped, a passing one loses the key). A pool's
    /// `entries` and `functions` and an entry's `children` are plain lists.
    pub(crate) fn admit_loot_table(&self, file: &mut Value, id: &str) -> bool {
        if !self.allows_id(file, "loot_table", id) {
            return false;
        }
        let path = || {
            let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
            format!("data/{namespace}/loot_table/{path}.json")
        };
        file.get_mut("pools")
            .and_then(Value::as_array_mut)
            .is_none_or(|pools| self.filter_list(pools, &path))
    }

    fn verdict(&self, file: &Value, path: &dyn Fn() -> String) -> Verdict {
        let Some(payload) = file.get("neoforge:conditions") else {
            return Ok(true);
        };
        let loaded = |id: &str| self.mod_loaded(id);
        let context = &self.context;
        let Some(registered) = &self.registered else {
            return evaluate(payload, &loaded, None, context);
        };
        let path = path();
        let path = if path.starts_with("data/") {
            path.as_str()
        } else {
            path.rfind("/data/")
                .map_or(path.as_str(), |i| &path[i + 1..])
        };
        let state = self
            .diagnostics
            .0
            .as_ref()
            .expect("owning candidate diagnostics");
        // Context-free answers are shared across candidate views. Cache the
        // result, not its loader's recovery policy.
        let encoded = payload.to_string();
        let reads_context = [
            "neoforge:registered",
            "neoforge:tag_empty",
            "neoforge:feature_flags_enabled",
        ]
        .iter()
        .any(|name| encoded.contains(name));
        let cell = state
            .lock()
            .unwrap()
            .cache
            .entry((
                path.to_owned(),
                encoded,
                if reads_context { context.key() } else { 0 },
            ))
            .or_default()
            .clone();
        cell.get_or_init(|| evaluate(payload, &loaded, Some(&registered.0.handlers), context))
            .clone()
    }
}

/// Does the built-in evaluator already implement this condition ID?
pub fn is_builtin_condition(id: &sunrise_core::identifier::Identifier) -> bool {
    BuiltinCondition::from_parts(id.namespace(), id.path()).is_some()
}

#[derive(Clone, Copy)]
enum BuiltinCondition {
    ModLoaded,
    Always,
    Never,
    FeatureFlags,
    Registered,
    TagEmpty,
    Not,
    And,
    Or,
}

impl BuiltinCondition {
    fn from_parts(namespace: &str, path: &str) -> Option<Self> {
        if namespace != "neoforge" {
            return None;
        }
        Some(match path {
            "mod_loaded" => Self::ModLoaded,
            "always" => Self::Always,
            "never" => Self::Never,
            "feature_flags_enabled" => Self::FeatureFlags,
            "registered" => Self::Registered,
            "tag_empty" => Self::TagEmpty,
            "not" => Self::Not,
            "and" => Self::And,
            "or" => Self::Or,
            _ => return None,
        })
    }
}

#[derive(Default)]
struct DecodeErrors<'a> {
    unknown: BTreeSet<&'a str>,
    invalid: BTreeSet<String>,
}

impl<'a> Condition<'a> {
    fn decode(
        node: &'a Value,
        handlers: Option<&'a Handlers>,
        errors: &mut DecodeErrors<'a>,
    ) -> Option<Self> {
        let Some(name) = node["type"].as_str() else {
            errors
                .invalid
                .insert("load condition: missing or invalid type".into());
            return None;
        };
        if let Some(kind) = name
            .split_once(':')
            .and_then(|(ns, path)| BuiltinCondition::from_parts(ns, path))
        {
            return match Self::decode_builtin(kind, node, handlers, errors) {
                Ok(condition) => condition,
                Err(field) => {
                    errors.invalid.insert(format!("{name}: invalid {field}"));
                    None
                }
            };
        }
        let handler = handlers.and_then(|handlers| match name.split_once(':') {
            Some((ns, _)) if !ns.is_empty() => handlers.get(name),
            _ => handlers.get(&format!(
                "minecraft:{}",
                name.strip_prefix(':').unwrap_or(name)
            )),
        });
        if let Some(handler) = handler {
            Some(Self::Custom(handler.as_ref(), node))
        } else {
            errors.unknown.insert(name);
            None
        }
    }

    fn decode_builtin(
        kind: BuiltinCondition,
        node: &'a Value,
        handlers: Option<&'a Handlers>,
        errors: &mut DecodeErrors<'a>,
    ) -> Result<Option<Self>, &'static str> {
        let identifier = |value: &Value, field| {
            value
                .as_str()
                .and_then(|id| sunrise_core::identifier::Identifier::parse(id).ok())
                .map(|id| id.to_string())
                .ok_or(field)
        };
        let registry = || {
            node.get("registry").map_or_else(
                || Ok("minecraft:item".into()),
                |value| identifier(value, "registry"),
            )
        };
        use BuiltinCondition::*;
        Ok(Some(match kind {
            Always => Self::Literal(true),
            Never => Self::Literal(false),
            ModLoaded => Self::ModLoaded(node["modid"].as_str().ok_or("modid")?),
            FeatureFlags => Self::FeatureFlags(
                node["flags"]
                    .as_array()
                    .filter(|flags| !flags.is_empty())
                    .ok_or("flags")?
                    .iter()
                    .map(|value| identifier(value, "flags"))
                    .collect::<Result<_, _>>()?,
            ),
            Registered => Self::Registered {
                registry: registry()?,
                value: identifier(&node["value"], "value")?,
            },
            TagEmpty => Self::TagEmpty {
                registry: registry()?,
                tag: identifier(&node["tag"], "tag")?,
            },
            Not => {
                let child = node.get("value").ok_or("value")?;
                return Ok(
                    Self::decode(child, handlers, errors).map(|child| Self::Not(Box::new(child)))
                );
            }
            kind @ (And | Or) => Self::Combinator {
                and: matches!(kind, And),
                children: node["values"]
                    .as_array()
                    .ok_or("values")?
                    .iter()
                    .filter_map(|child| Self::decode(child, handlers, errors))
                    .collect(),
            },
        }))
    }
}

fn evaluate(
    payload: &Value,
    loaded: &dyn Fn(&str) -> bool,
    handlers: Option<&Handlers>,
    context: &Context,
) -> Verdict {
    // Decode the entire tree before invoking any test, including branches that
    // cannot affect the boolean answer.
    let mut errors = DecodeErrors::default();
    let conditions: Vec<_> = payload
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| Condition::decode(node, handlers, &mut errors))
        .collect();
    if !errors.unknown.is_empty() {
        errors.invalid.insert(format!(
            "unknown load conditions [{}]",
            errors.unknown.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !errors.invalid.is_empty() {
        return Err(ConditionError::Invalid(
            errors.invalid.into_iter().collect::<Vec<_>>().join("; "),
        ));
    }
    let mut missing_context = None;
    let allowed = Condition::combine(true, &conditions, loaded, context, &mut missing_context)?;
    missing_context.map_or(Ok(allowed), |reason| {
        Err(ConditionError::MissingContext(reason))
    })
}
