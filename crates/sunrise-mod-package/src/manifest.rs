//! stable-line: content-declarations
//! `sunrise-mod.toml`, a mod's manifest, and the `.smod` file around it.
//!
//! Native mixin schema adapted from horizon-mixins (Copyright (c) 2026 levzzz),
//! MIT licensed; see `HORIZON_MIXINS_LICENSE` and `THIRD_PARTY_NOTICES.md`.
mod hash;
mod jar;
pub mod loops;
pub mod mixin;
mod runtime_files;
pub use hash::sha512_of;
pub use jar::{
    JarConflict, JarInputsError, JarRef, JarRequirement, MissingJar, collect_jar_requirements,
    missing_jars,
};
pub use runtime_files::{
    MAX_RUNTIME_FILE, MAX_RUNTIME_FILES, MAX_RUNTIME_TOTAL, RUNTIME_DIR, RuntimeData, RuntimeTable,
};

use mod_surface::uses::USES_FILE;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::Read,
    path::{Path, PathBuf},
};

/// The manifest's file name in a `.smod` or a mod folder.
pub const MANIFEST: &str = "sunrise-mod.toml";
/// The content manifest API this build loads.
pub const API: &str = "0.2";

fn api() -> String {
    API.to_owned()
}
/// Namespaces no mod may claim: vanilla's, the convention tags' (mods add
/// to those, docs/modding/src/how-to/content-mods.md) and the game's own.
const RESERVED_NAMESPACES: [&str; 3] = ["minecraft", "c", "sunrise"];
/// Ids no mod or pack may take: the mod site's own pages under `/mods/` (a project's page is
/// `/mods/<id>`), and the names of the pages below an id. The site's routes are checked against
/// this list on its side.
pub const RESERVED_IDS: [&str; 7] = [
    "account",
    "edit",
    "index",
    "mine",
    "policy",
    "registry-page",
    "submit",
];

/// A mod's version as semver, padding an omitted minor and patch (`1` is `1.0.0`, `1.2` is
/// `1.2.0`). Pre-release and build metadata are allowed.
pub fn parse_version(version: &str) -> Result<semver::Version, String> {
    // Only the `major.minor.patch` core is padded: the dots of a pre-release (`1-rc.1`) or build part
    // after a `-` or `+` belong to it.
    let (core, suffix) = version.split_at(version.find(['-', '+']).unwrap_or(version.len()));
    let padded = match core.matches('.').count() {
        0 => format!("{core}.0.0{suffix}"),
        1 => format!("{core}.0{suffix}"),
        _ => version.to_owned(),
    };
    semver::Version::parse(&padded).map_err(|_| {
        format!(
            "version {version:?} is not a semver version (MAJOR.MINOR.PATCH, e.g. 3.6.30+refabricated)"
        )
    })
}

/// The first build-metadata identifier of the placeholder a port gives a jar version it could not
/// read as semver (`0.0.0+unresolved.<the jar's text>`).
pub const UNRESOLVED_BUILD: &str = "unresolved";

/// Whether `version` is such a placeholder. Dependency resolution never lets one satisfy a
/// requirement, since semver matching ignores build metadata and `*` or `=0.0.0` would accept it;
/// it stays refused until someone sets the real version by hand. Any other `0.0.0` is a real version.
pub fn is_unresolved(version: &semver::Version) -> bool {
    version.major == 0
        && version.minor == 0
        && version.patch == 0
        && version.pre.is_empty()
        && version.build.split('.').next() == Some(UNRESOLVED_BUILD)
}

fn version_from_str<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<semver::Version, D::Error> {
    let version = String::deserialize(deserializer)?;
    parse_version(&version).map_err(serde::de::Error::custom)
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[a-z0-9_]`, 1 to 64 characters: the mod's folder, log and settings key.
    pub id: String,
    pub name: String,
    /// Semver, read once here: omitted minor and patch are padded ([`parse_version`]).
    #[serde(deserialize_with = "version_from_str")]
    pub version: semver::Version,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// The Mods screen's `License:` line (a name such as `MIT`); empty shows it as not given.
    #[serde(default)]
    pub license: String,
    /// Where the license text is: the screen makes its name a link.
    #[serde(default)]
    pub license_url: Option<String>,
    /// The Mods screen's `Credits:` line.
    #[serde(default)]
    pub credits: String,
    /// The mod's home page, the Mods screen's Homepage button.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Where to report problems, the Mods screen's Issues button.
    #[serde(default)]
    pub issues: Option<String>,
    /// The content API line; the tool writes it into the packed package. Absent, it is [`API`].
    #[serde(default = "api")]
    pub api: String,
    #[serde(default)]
    pub side: Side,
    /// What the mod adds, as the Mods screen and the registry list it; the tool writes it into the
    /// packed package. Absent, it is [`Capability::derived`].
    #[serde(default = "Capability::derived")]
    pub capabilities: Vec<Capability>,
    /// Namespaces besides `id` that the mod's `assets/` and `data/` use, for
    /// a content mod whose files sit under other names than its id (Towns
    /// and Towers ships `kaisyn` and `towns_and_towers`). No two content mods
    /// that are on may share one (`content::clash`).
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// Other native mods whose hooks must be available before this one.
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, Dependency>,
    /// Mods this one breaks with: the game refuses to start while one of them is loaded at a
    /// version the rule matches (NeoForge's `DependencyType.INCOMPATIBLE`).
    #[serde(default)]
    pub incompatible: std::collections::BTreeMap<String, Rule>,
    /// Mods this one works badly with: they load, with a warning (`DependencyType.DISCOURAGED`).
    #[serde(default)]
    pub discouraged: std::collections::BTreeMap<String, Rule>,
    /// Mods whose hooks run after this one's when they are loaded, without depending on them
    /// (`Ordering.BEFORE`).
    #[serde(default)]
    pub load_before: Vec<String>,
    /// Original Java jars, layered in list order below this mod's own files.
    #[serde(default)]
    pub jar: Vec<JarRef>,
    /// Exact build pins and support modules for a code mod.
    pub native: Option<Native>,
    /// The data files of the package's `runtime/` folder, each with its SHA-512. Not pack data.
    #[serde(default)]
    pub runtime: Option<RuntimeTable>,
    /// Game internals used by this mod, fingerprinted against its pinned build.
    #[serde(default)]
    pub internals: Vec<Internal>,
    /// Free-form values other mods read through `PackageInfo::property`.
    #[serde(default)]
    pub properties: std::collections::BTreeMap<String, Property>,
}

/// The most `[properties]` keys a manifest may declare.
pub const MAX_PROPERTIES: usize = 64;
/// The most `[properties]` bytes: keys and strings by length, 8 per number, 1 per boolean.
pub const MAX_PROPERTY_BYTES: usize = 8 << 10;

/// One `[properties]` value: a table or array is refused.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Property {
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Dependency {
    Version(semver::VersionReq),
    Detailed(DependencyDetails),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DependencyDetails {
    pub version: semver::VersionReq,
    #[serde(default)]
    pub optional: bool,
    /// The installation on which this dependency declaration applies.
    #[serde(default = "both_sides")]
    pub side: Side,
    /// A mod-provided explanation, at most 512 bytes.
    #[serde(default)]
    pub reason: Option<String>,
    /// An HTTPS link with more information.
    #[serde(default)]
    pub url: Option<String>,
    /// Dispatch ordering only; `none` does not remove a code link requirement.
    #[serde(default)]
    pub order: mod_vocab::DependencyOrder,
}

impl Dependency {
    /// The dependency's typed declaration, with bare-version defaults filled in.
    pub fn details(&self) -> DependencyDetails {
        match self {
            Self::Version(version) => DependencyDetails {
                version: version.clone(),
                optional: false,
                side: Side::Both,
                reason: None,
                url: None,
                order: mod_vocab::DependencyOrder::After,
            },
            Self::Detailed(details) => details.clone(),
        }
    }
}

/// An `[incompatible]` or `[discouraged]` entry: the versions of the other mod it matches
/// (`"<2"`), or a table that also gives the player a reason and a link.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Rule {
    Version(semver::VersionReq),
    Detailed(RuleDetails),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuleDetails {
    /// Every version when left out.
    #[serde(default = "any_version")]
    pub version: semver::VersionReq,
    /// Why, shown when the rule fails: at most 512 bytes.
    #[serde(default)]
    pub reason: Option<String>,
    /// Where to read more, shown with the reason: an `https://` link.
    #[serde(default)]
    pub url: Option<String>,
    /// Where the rule holds: both sides, or only the client or the dedicated server.
    #[serde(default = "both_sides")]
    pub side: Side,
}

fn both_sides() -> Side {
    Side::Both
}

fn any_version() -> semver::VersionReq {
    semver::VersionReq::STAR
}

impl Rule {
    /// The versions, reason, link and side, the bare form's as defaults.
    pub fn details(&self) -> RuleDetails {
        match self {
            Self::Version(version) => RuleDetails {
                version: version.clone(),
                reason: None,
                url: None,
                side: Side::Both,
            },
            Self::Detailed(details) => details.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Native {
    pub game: String,
    pub sunrise: String,
    /// The horizon-mixins revision, when the mod pins one itself: absent, it is the one the pinned
    /// SDK names (see [`Native::mixins_or`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixins: Option<String>,
    /// Source-level edits of game functions, native only: the one form that compiles game code.
    #[serde(default)]
    pub mixin: Vec<mixin::Mixin>,
    #[serde(default)]
    pub module: Vec<Module>,
    /// Accessor and invoker bridges, forwarded as horizon-mixins' `[[bridge]]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridge: Vec<mixin::Forwarded>,
    /// State fields added to game types, forwarded as horizon-mixins' `[[state]]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<mixin::Forwarded>,
    /// Slots of published game loops, native only (`mod_api::loops`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub r#loop: Vec<loops::Loop>,
}

impl Native {
    /// The horizon-mixins revision this mod builds with: its own `mixins` pin, else `sdk`, the
    /// revision the pinned SDK names (`sdk.json`'s `mixins`).
    pub fn mixins_or<'a>(&'a self, sdk: &'a str) -> &'a str {
        self.mixins.as_deref().unwrap_or(sdk)
    }
}

/// One declared game internal, as listed in the pinned game's `internals.json`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Internal {
    pub path: String,
    /// The catalog's member kind, required when several kinds share the path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Optional pinning: the catalog fingerprint `internals add` writes. Absent, the build takes
    /// it from the pinned game's catalog, which `[native] sunrise` fixes exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// A support module merged into a game crate, forwarded as horizon-mixins'
/// `[[module]]`: self-contained Rust items, emitted into `file` as
/// `mod name`, for helpers that need more than one function.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub id: String,
    /// The game source file that receives the module, as the pin indexes it.
    pub file: String,
    /// The module's Rust name under the destination file's top level.
    pub name: String,
    /// The payload's file in this package, under `src/`.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

pub use mod_vocab::Side;

/// What a content mod adds; the player sees it before turning the mod on.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// New blocks, items and mob effects from the mod's `data/` (API 0.2).
    Registries,
}
impl Capability {
    /// What a package's capabilities are when its manifest does not say: the one label every
    /// package has carried (the Mods screen and the registry list read it), so every reader sees
    /// the same value as before.
    pub fn derived() -> Vec<Capability> {
        vec![Capability::Registries]
    }
    /// What the player reads before turning the mod on: "May <this>".
    pub fn describe(self) -> &'static str {
        match self {
            Capability::Registries => {
                "add blocks, items, mob effects and entity types to your worlds"
            }
        }
    }
}
impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Registries => f.write_str("registries"),
        }
    }
}

/// Whether a source pin is an exact lowercase Git object id.
pub fn revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn valid_mod_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && !reserved_mod_id(id)
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn reserved_mod_id(id: &str) -> bool {
    matches!(id, "con" | "prn" | "aux" | "nul")
        || (id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && matches!(id.as_bytes()[3], b'1'..=b'9'))
}

fn rust_ident(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
}

impl Internal {
    fn check_all(internals: &[Self]) -> Result<(), String> {
        let mut paths = std::collections::HashSet::new();
        for internal in internals {
            if internal.path.is_empty()
                || internal.hash.as_ref().is_some_and(String::is_empty)
                || internal.kind.as_ref().is_some_and(|kind| kind.is_empty())
                || !paths.insert((&internal.path, &internal.kind))
            {
                return Err(format!(
                    "{MANIFEST}: empty internal path, kind or hash, or duplicate internal path and kind"
                ));
            }
        }
        Ok(())
    }
}

/// A manifest's TOML error in the tool's words: no caret box, the missing field with a line to add,
/// an unknown field with the nearest valid key when one is a few edits away.
fn toml_error(text: &str, e: &toml::de::Error) -> String {
    let message = e.message();
    let line = e
        .span()
        .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1);
    let at = line.map_or(String::new(), |line| format!(" (line {line})"));
    let quoted = |after: &str| -> Option<String> {
        let rest = message.strip_prefix(after)?;
        Some(rest.split('`').next()?.to_owned())
    };
    if let Some(field) = quoted("missing field `") {
        let example = match field.as_str() {
            "id" => "id = \"my_mod\"".to_owned(),
            "name" => "name = \"My Mod\"".to_owned(),
            "version" => "version = \"0.1.0\"".to_owned(),
            "game" => "game = \"26.3\"".to_owned(),
            "sunrise" => "sunrise = \"<the 40-character game build commit>\"".to_owned(),
            other => format!("{other} = …"),
        };
        return format!("{MANIFEST}: the field {field} is missing; add {example}");
    }
    if let Some(field) = quoted("unknown field `") {
        let nearest = message
            .split("expected ")
            .nth(1)
            .into_iter()
            .flat_map(|list| list.split('`').skip(1).step_by(2))
            .map(|key| (edit_distance(&field, key), key))
            .min()
            .filter(|(distance, key)| *distance <= (key.len() / 3).clamp(1, 2));
        let hint = nearest.map_or(String::new(), |(_, key)| format!("; did you mean `{key}`?"));
        return format!("{MANIFEST}: {message}{at}{hint}");
    }
    format!("{MANIFEST}: {message}{at}")
}

/// The number of single-character edits between two keys.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(x != *y))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let m: Manifest = toml::from_str(text).map_err(|e| toml_error(text, &e))?;
        Internal::check_all(&m.internals)?;
        if !m.internals.is_empty() && m.native.is_none() {
            return Err(format!("{MANIFEST}: internals need a [native] mod"));
        }
        if RESERVED_NAMESPACES.contains(&m.id.as_str()) {
            return Err(format!(
                "{MANIFEST}: id {:?} is a reserved namespace, not a mod's to claim",
                m.id
            ));
        }
        if m.runtime.is_some() && m.native.is_none() {
            return Err(format!(
                "{MANIFEST}: a content mod has no code, so no [runtime]"
            ));
        }
        if RESERVED_IDS.contains(&m.id.as_str()) {
            return Err(format!(
                "{MANIFEST}: id {:?} is a page of the mod site, so no mod can be published under it: pick another id",
                m.id
            ));
        }
        if reserved_mod_id(&m.id) {
            return Err(format!(
                "{MANIFEST}: id {:?} is a reserved Windows device name",
                m.id
            ));
        }
        if !valid_mod_id(&m.id) {
            return Err(format!(
                "{MANIFEST}: id {:?} is not [a-z0-9_]{{1,64}}",
                m.id
            ));
        }
        let rules = m.incompatible.iter().chain(&m.discouraged);
        for id in m
            .dependencies
            .keys()
            .chain(rules.clone().map(|(id, _)| id))
            .chain(&m.load_before)
        {
            if !valid_mod_id(id) {
                return Err(format!("{MANIFEST}: invalid dependency id {id:?}"));
            }
        }
        let rule_notes = rules.map(|(id, rule)| {
            let details = rule.details();
            (id, details.reason, details.url)
        });
        let dependency_notes = m.dependencies.iter().map(|(id, dependency)| {
            let details = dependency.details();
            (id, details.reason, details.url)
        });
        for (id, reason, url) in rule_notes.chain(dependency_notes) {
            if reason.as_ref().is_some_and(|reason| reason.len() > 512) {
                return Err(format!("{MANIFEST}: rule for {id}: reason over 512 bytes"));
            }
            if url.as_ref().is_some_and(|url| !url.starts_with("https://")) {
                return Err(format!("{MANIFEST}: rule for {id}: url is not https://"));
            }
        }
        mixin::Mixin::check_all(
            m.native
                .as_ref()
                .map(|n| n.mixin.as_slice())
                .unwrap_or_default(),
            &m.id,
        )
        .map_err(|e| format!("{MANIFEST}: {e}"))?;
        loops::Loop::check_all(
            m.native
                .as_ref()
                .map(|n| n.r#loop.as_slice())
                .unwrap_or_default(),
            &m.id,
        )
        .map_err(|e| format!("{MANIFEST}: {e}"))?;
        if let Some(entry) = m
            .native
            .as_ref()
            .map(|n| n.mixin.as_slice())
            .unwrap_or_default()
            .iter()
            .find(|entry| {
                m.dependencies.keys().any(|id| {
                    entry.target.split("::").next() == Some(id.replace('-', "_").as_str())
                })
            })
        {
            return Err(format!(
                "{MANIFEST}: {}",
                mixin::MixinError::Invalid {
                    id: format!("{}:{}", m.id, entry.id),
                    field: "target",
                    fix: "a mixin targets a game function",
                }
            ));
        }
        if let Some(native) = &m.native {
            // Native source edits share one id space.
            let mut ids = std::collections::HashSet::new();
            for id in native
                .mixin
                .iter()
                .map(|entry| &entry.id)
                .chain(native.bridge.iter().chain(&native.state).map(|row| &row.id))
                .chain(native.r#loop.iter().map(|row| &row.id))
            {
                if id.trim().is_empty() || !ids.insert(id) {
                    return Err(format!(
                        "{MANIFEST}: empty or duplicate mixin, bridge, state or loop id"
                    ));
                }
            }
            // A bridge's id becomes part of its accessor's name: `-` reads as `_`, and case is
            // dropped, so the id is that alphabet and no two ids may give one name.
            let mut accessors = std::collections::HashMap::new();
            for bridge in &native.bridge {
                let id = &bridge.id;
                if !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(format!(
                        "{MANIFEST}: E_MIXIN_INVALID: {}:{id}: invalid id; a bridge id is ASCII letters, digits, `_` and `-`",
                        m.id
                    ));
                }
                if let Some(other) = accessors.insert(id.replace('-', "_").to_ascii_lowercase(), id)
                {
                    return Err(format!(
                        "{MANIFEST}: E_MIXIN_INVALID: {}:{id}: invalid id; it gives the same accessor name as bridge `{other}`",
                        m.id
                    ));
                }
            }
            for module in &native.module {
                if module.id.trim().is_empty() || !ids.insert(&module.id) {
                    return Err(format!("{MANIFEST}: empty or duplicate module id"));
                }
                if !plain_path(&module.file) {
                    return Err(format!("{MANIFEST}: module file must be a plain path"));
                }
                if !rust_ident(&module.name) {
                    return Err(format!("{MANIFEST}: module name must be an identifier"));
                }
                if !plain_path(&module.source) || !source_file(&module.source) {
                    return Err(format!(
                        "{MANIFEST}: module source must be a file under src/"
                    ));
                }
                if let Some(visibility) = &module.visibility
                    && !matches!(visibility.as_str(), "pub" | "pub(crate)")
                {
                    return Err(format!(
                        "{MANIFEST}: module visibility must be pub or pub(crate)"
                    ));
                }
            }
            if native.game.is_empty()
                || !revision(&native.sunrise)
                || native.mixins.as_deref().is_some_and(|m| !revision(m))
            {
                return Err(format!(
                    "{MANIFEST}: native requires game and exact revisions"
                ));
            }
        }
        if m.api != API {
            return Err(format!(
                "unsupported content API {}; this Sunrise reads {API}: delete the api line, the tool writes it",
                m.api
            ));
        }
        let property_bytes: usize = m
            .properties
            .iter()
            .map(|(key, value)| {
                key.len()
                    + match value {
                        Property::Boolean(_) => 1,
                        Property::Integer(_) | Property::Float(_) => 8,
                        Property::String(text) => text.len(),
                    }
            })
            .sum();
        if m.properties.len() > MAX_PROPERTIES || property_bytes > MAX_PROPERTY_BYTES {
            return Err(format!(
                "{MANIFEST}: [properties] holds at most {MAX_PROPERTIES} keys and {MAX_PROPERTY_BYTES} \
                 bytes; this one has {} keys and {property_bytes} bytes",
                m.properties.len()
            ));
        }
        if m.capabilities != [Capability::Registries] {
            let names = |list: &[Capability]| {
                format!(
                    "[{}]",
                    list.iter()
                        .map(|c| format!("\"{c}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return Err(format!(
                "{MANIFEST}: capabilities = {} but the tool writes {}; delete the capabilities line",
                names(&m.capabilities),
                names(&Capability::derived())
            ));
        }
        for (field, url) in [
            ("license_url", &m.license_url),
            ("homepage", &m.homepage),
            ("issues", &m.issues),
        ] {
            if let Some(url) = url
                && (url.len() > 2048
                    || !(url.starts_with("https://") || url.starts_with("http://")))
            {
                return Err(format!(
                    "{MANIFEST}: {field} is an http(s) link of at most 2048 characters"
                ));
            }
        }
        for (i, ns) in m.namespaces.iter().enumerate() {
            // Vanilla's namespace characters, at most as long as an id.
            let chars_ok = (1..=64).contains(&ns.len())
                && ns
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
            if !chars_ok {
                return Err(format!(
                    "{MANIFEST}: namespace {ns:?} is not [a-z0-9_.-]{{1,64}}"
                ));
            }
            if RESERVED_NAMESPACES.contains(&ns.as_str()) {
                return Err(format!(
                    "{MANIFEST}: namespace {ns} is not a mod's to claim (only to add to its tags)"
                ));
            }
            if *ns == m.id || m.namespaces[..i].contains(ns) {
                return Err(format!("{MANIFEST}: namespace {ns} is listed twice"));
            }
        }
        Ok(m)
    }
}

#[cfg(test)]
#[path = "manifest/runtime_files_tests.rs"]
mod runtime_files_tests;

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
