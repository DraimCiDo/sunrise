//! stable-line: decodes-other-data
//! NeoForge's load conditions (`neoforge:conditions`): a data file that has
//! the key loads only when every condition in it holds. A false condition
//! silently omits the file; a decode or handler error goes to the owning loader.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod declarations;
pub use declarations::is_builtin_condition;

/// Enabled package IDs captured by the engine for one owning data load.
/// Readers share this immutable view; they never recover IDs from namespaces.
/// Resource selection happens before evaluating ordinary JSON files. Stacked
/// data maps evaluate each layer before merge or replace; tags ignore conditions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LoadConditions {
    mods: Vec<String>,
    registered: Option<RegisteredLookup>,
    context: Context,
    diagnostics: Diagnostics,
}

impl LoadConditions {
    /// A vanilla data load has the built-in package only.
    pub const fn vanilla() -> Self {
        Self {
            mods: Vec::new(),
            registered: None,
            context: Context {
                features: None,
                feature_fingerprint: None,
                registries: None,
                tags: None,
            },
            diagnostics: Diagnostics::new(),
        }
    }

    /// Capture IDs visible to the owning load's side. The engine supplies its
    /// enabled-mod inventory's server view for logical-server data.
    pub fn new<'a>(ids: impl IntoIterator<Item = &'a str>) -> Self {
        let mut mods: Vec<_> = ids
            .into_iter()
            .filter(|id| *id != "minecraft")
            .map(str::to_owned)
            .collect();
        mods.sort();
        mods.dedup();
        Self {
            mods,
            registered: None,
            context: Context::default(),
            diagnostics: Diagnostics::default(),
        }
    }

    /// Use the recipe source's inventory while retaining this candidate's
    /// registered handlers, evaluation cache and diagnostics.
    pub fn with_mod_inventory<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Self {
        let mut view = self.clone();
        view.mods = Self::new(ids).mods;
        if view.mods != self.mods {
            view.diagnostics = match &self.registered {
                Some(registered) => registered
                    .0
                    .inventories
                    .lock()
                    .unwrap()
                    .entry(view.mods.clone())
                    .or_default()
                    .clone(),
                // Without handlers there are no cached answers; keep registry
                // diagnostics shared across the candidate.
                None => self.diagnostics.clone(),
            };
        }
        view
    }

    pub fn mod_loaded(&self, id: &str) -> bool {
        id == "minecraft"
            || self
                .mods
                .binary_search_by(|loaded| loaded.as_str().cmp(id))
                .is_ok()
    }

    /// `IContext.enabledFeatures`: the world's enabled feature flag ids, which
    /// `neoforge:feature_flags_enabled` reads. Empty is vanilla's default set.
    pub fn with_features<'a>(mut self, flags: impl IntoIterator<Item = &'a str>) -> Self {
        let mut flags: BTreeSet<String> = flags.into_iter().map(qualified).collect();
        if flags.is_empty() {
            flags.insert("minecraft:vanilla".to_owned());
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        flags.hash(&mut hasher);
        self.context.feature_fingerprint = Some(hasher.finish());
        self.context.features = Some(Arc::new(flags));
        self
    }

    /// `IContext.registries`: every registry's entry ids (built-in and the
    /// packs' datapack registries), which `neoforge:registered` reads.
    pub fn with_registries<'a, I>(
        mut self,
        registries: impl IntoIterator<Item = (&'a str, I)>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.context.registries = Some(Arc::new(Names::new(registries)));
        self
    }

    /// `IContext.getTag`: the tags this load has bound, as each registry's
    /// tags holding at least one entry. Readers that run before the tags load
    /// never get this view, so `neoforge:tag_empty` there omits the file
    /// instead of answering from an older load's tags.
    pub fn with_tags<'a, I>(&self, non_empty: impl IntoIterator<Item = (&'a str, I)>) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut view = self.clone();
        view.context.tags = Some(Arc::new(Names::new(non_empty)));
        view
    }

    /// Attach the engine's immutable registration table to this candidate load.
    pub fn with_lookup<S, F>(mut self, handlers: impl IntoIterator<Item = (S, F)>) -> Self
    where
        S: Into<String>,
        F: Fn(&Value) -> Result<bool, String> + Send + Sync + 'static,
    {
        let handlers: Handlers = handlers
            .into_iter()
            .map(|(id, handler)| (id.into(), Box::new(handler) as Box<ConditionHandler>))
            .collect();
        if handlers.is_empty() {
            return self;
        }
        if self.diagnostics.0.is_none() {
            self.diagnostics = Diagnostics::default();
        }
        self.registered = Some(RegisteredLookup(Arc::new(LookupState {
            handlers,
            inventories: std::sync::Mutex::new(BTreeMap::from([(
                self.mods.clone(),
                self.diagnostics.clone(),
            )])),
        })));
        self
    }

    /// Whether this candidate has any registered condition handlers.
    pub fn has_registered(&self) -> bool {
        self.registered.is_some()
    }

    pub fn allows(&self, file: &Value) -> bool {
        self.allows_file(file, "data file")
    }

    /// Evaluate a resource, constructing its path only for registered handlers or diagnostics.
    pub fn allows_id(&self, file: &Value, kind: &str, id: &str) -> bool {
        self.allows_context(file, || {
            let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
            format!("data/{namespace}/{kind}/{path}.json")
        })
    }

    /// Evaluate a resource whose namespace and path are stored separately.
    pub fn allows_parts(&self, file: &Value, kind: &str, namespace: &str, path: &str) -> bool {
        self.allows_context(file, || format!("data/{namespace}/{kind}/{path}.json"))
    }

    /// Evaluate a registry entry. Its owning load checks accumulated errors
    /// before it publishes the registry.
    pub fn allows_file(&self, file: &Value, path: &str) -> bool {
        self.allows_context(file, || path.to_owned())
    }

    /// Return a file's error without adding it to registry diagnostics. The
    /// caller decides whether to skip the file or refuse its own candidate.
    pub fn allows_file_isolated(&self, file: &Value, path: &str) -> Result<bool, String> {
        self.verdict(file, &|| path.to_owned())
            .map_err(|error| error.message().to_owned())
    }

    fn allows_context(&self, file: &Value, path: impl Fn() -> String) -> bool {
        self.registry_verdict(file, &path).unwrap_or(false)
    }

    /// A false condition omits the entry; a decode or handler error also
    /// invalidates the enclosing registry file (for example a loot table).
    fn registry_verdict(&self, file: &Value, path: &dyn Fn() -> String) -> Option<bool> {
        match self.verdict(file, path) {
            Ok(answer) => Some(answer),
            Err(error) => {
                let diagnostic = format!("{}: {}", path(), error.message());
                // Some early readers do not have all of the built-in contexts.
                // Preserve their existing omission rather than reading stale tags.
                let invalid = matches!(error, ConditionError::Invalid(_));
                if let Some(state) = self.diagnostics.0.as_ref().filter(|_| invalid) {
                    state.lock().unwrap().errors.insert(diagnostic);
                } else {
                    log::error!("{diagnostic}; file left out");
                }
                if invalid { None } else { Some(false) }
            }
        }
    }

    /// Refuse a failed candidate before publishing any of its tables.
    pub fn check_errors(&self) -> Result<(), String> {
        if let Some(state) = self.diagnostics.0.as_ref() {
            let state = state.lock().unwrap();
            if !state.errors.is_empty() {
                return Err(state.errors.iter().cloned().collect::<Vec<_>>().join("; "));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum ConditionError {
    Invalid(String),
    MissingContext(&'static str),
}
impl ConditionError {
    fn message(&self) -> &str {
        match self {
            Self::Invalid(message) => message,
            Self::MissingContext(message) => message,
        }
    }
}

type Verdict = Result<bool, ConditionError>;
type CachedVerdict = Arc<std::sync::OnceLock<Verdict>>;

#[derive(Debug, Default)]
struct CandidateDiagnostics {
    cache: BTreeMap<(String, String, u64), CachedVerdict>,
    errors: BTreeSet<String>,
}
// Diagnostics do not contribute to a resource generation's cache identity.
#[derive(Clone, Debug)]
struct Diagnostics(Option<std::sync::Arc<std::sync::Mutex<CandidateDiagnostics>>>);
impl Diagnostics {
    const fn new() -> Self {
        Self(None)
    }
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self(Some(Default::default()))
    }
}
impl PartialEq for Diagnostics {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Diagnostics {}
impl std::hash::Hash for Diagnostics {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// `ICondition.IContext`: what `feature_flags_enabled`, `registered` and
/// `tag_empty` read. A part the owning load didn't provide leaves its
/// condition unsupported: the file is omitted with one error, never answered stale.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
struct Context {
    features: Option<Arc<BTreeSet<String>>>,
    feature_fingerprint: Option<u64>,
    registries: Option<Arc<Names>>,
    tags: Option<Arc<Names>>,
}
impl Context {
    /// The cache key of answers given under this context.
    fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        (&self.feature_fingerprint, &self.registries, &self.tags).hash(&mut hasher);
        hasher.finish()
    }
}

/// Registry id -> entry ids, compared and hashed by a fingerprint taken once,
/// so cache identity never rehashes every entry.
#[derive(Debug)]
struct Names {
    by_registry: BTreeMap<String, BTreeSet<String>>,
    fingerprint: u64,
}
impl Names {
    fn new<'a, I>(registries: impl IntoIterator<Item = (&'a str, I)>) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        use std::hash::{Hash, Hasher};
        let mut by_registry: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (registry, ids) in registries {
            by_registry
                .entry(qualified(registry))
                .or_default()
                .extend(ids.into_iter().map(qualified));
        }
        let mut hasher = std::hash::DefaultHasher::new();
        by_registry.hash(&mut hasher);
        Self {
            by_registry,
            fingerprint: hasher.finish(),
        }
    }
    fn contains(&self, registry: &str, id: &str) -> bool {
        self.by_registry
            .get(registry)
            .is_some_and(|ids| ids.contains(id))
    }
}
impl PartialEq for Names {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
    }
}
impl Eq for Names {}
impl std::hash::Hash for Names {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fingerprint.hash(state);
    }
}

/// An identifier with vanilla's default namespace (`Identifier.parse`).
fn qualified(id: &str) -> String {
    let id = id.strip_prefix('#').unwrap_or(id);
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

// Pass the raw node without copying; the registration adapter removes `type`
// before passing the remaining fields to the mod.
type ConditionHandler = dyn Fn(&Value) -> Result<bool, String> + Send + Sync;
type Handlers = BTreeMap<String, Box<ConditionHandler>>;
struct LookupState {
    inventories: std::sync::Mutex<BTreeMap<Vec<String>, Diagnostics>>,
    handlers: Handlers,
}
#[derive(Clone)]
struct RegisteredLookup(std::sync::Arc<LookupState>);
impl std::fmt::Debug for RegisteredLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RegisteredLookup")
    }
}
impl PartialEq for RegisteredLookup {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for RegisteredLookup {}
impl std::hash::Hash for RegisteredLookup {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&std::sync::Arc::as_ptr(&self.0), state);
    }
}

enum Condition<'a> {
    ModLoaded(&'a str),
    Literal(bool),
    FeatureFlags(Vec<String>),
    Registered { registry: String, value: String },
    TagEmpty { registry: String, tag: String },
    Not(Box<Self>),
    Combinator { and: bool, children: Vec<Self> },
    Custom(&'a ConditionHandler, &'a Value),
}

impl<'a> Condition<'a> {
    // Only pure tests with available inputs can be skipped after the result
    // is decided. Decode has already checked even the branches we skip.
    fn must_test(&self, context: &Context) -> bool {
        match self {
            Self::ModLoaded(_) | Self::Literal(_) => false,
            Self::FeatureFlags(_) => context.features.is_none(),
            Self::Registered { .. } => context.registries.is_none(),
            Self::TagEmpty { .. } => context.tags.is_none(),
            Self::Not(child) => child.must_test(context),
            Self::Combinator { children, .. } => {
                children.iter().any(|child| child.must_test(context))
            }
            Self::Custom(..) => true,
        }
    }

    fn test(
        &self,
        loaded: &dyn Fn(&str) -> bool,
        context: &Context,
        missing_context: &mut Option<&'static str>,
    ) -> Verdict {
        let mut unavailable = |reason| {
            missing_context.get_or_insert(reason);
            Ok(false)
        };
        Ok(match self {
            Self::ModLoaded(id) => loaded(id),
            Self::Literal(value) => *value,
            Self::FeatureFlags(flags) => {
                let Some(enabled) = &context.features else {
                    return unavailable(
                        "neoforge:feature_flags_enabled: no feature flags in this load",
                    );
                };
                flags.iter().all(|flag| enabled.contains(flag))
            }
            Self::Registered { registry, value } => {
                let Some(registries) = &context.registries else {
                    return unavailable("neoforge:registered: no registries in this load");
                };
                registries.contains(registry, value)
            }
            Self::TagEmpty { registry, tag } => {
                let Some(tags) = &context.tags else {
                    return unavailable("neoforge:tag_empty: tags not loaded yet");
                };
                !tags.contains(registry, tag)
            }
            Self::Not(child) => !child.test(loaded, context, missing_context)?,
            Self::Combinator { and, children } => {
                Self::combine(*and, children, loaded, context, missing_context)?
            }
            Self::Custom(handler, args) => handler(args).map_err(ConditionError::Invalid)?,
        })
    }

    fn combine(
        and: bool,
        children: &[Self],
        loaded: &dyn Fn(&str) -> bool,
        context: &Context,
        missing_context: &mut Option<&'static str>,
    ) -> Verdict {
        let mut answer = and;
        for child in children {
            if answer == and || child.must_test(context) {
                let value = child.test(loaded, context, missing_context)?;
                answer = if and {
                    answer && value
                } else {
                    answer || value
                };
            }
        }
        Ok(answer)
    }
}

#[cfg(test)]
#[path = "conditions_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "condition_registry_tests.rs"]
mod registry_tests;

#[cfg(test)]
#[path = "condition_log_tests.rs"]
mod log_tests;
