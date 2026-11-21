//! stable-line: content-declarations
//! A content mod's `assets/` and `data/` (API 0.2, docs/modpacks.md): the
//! files it adds to the game's resource tree, and the blocks, items, mob
//! effects, entity types and menu types it declares in
//! `data/<id>/sunrise/{block,item,mob_effect,entity_type,fluid,fluid_interaction,menu}/`.
//!
//! Content is data, not code: nothing here executes. What a mod
//! may add is fenced instead. Its own namespaces (its id and the ones its
//! manifest lists), plus additions to vanilla tags and to the convention
//! tags (`c`, which NeoForge and Fabric mods share). Paths are plain, files
//! and totals are capped, vanilla ids cannot be redefined, and no two mods
//! share a namespace ([`clash`]).
use crate::manifest::Manifest;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

mod entries;
mod load;
pub use entries::{
    Block, BlockEntityType, CreativeTab, FluidLook, FluidModel, Item, LocalDeclaration,
};
use load::reader_of;
pub use load::{Content, FileReader, MAX_FILE, MAX_FILES, MAX_TOTAL, check_path, clash};
pub(crate) use load::{folder_files, runtime_folder_files};
pub use sunrise_core::arm_pose::ArmPose;

/// The single resource location NeoForge's `RecipePriorityManager` reads in pack order.
pub const RECIPE_PRIORITIES_PATH: &str = "data/neoforge/recipe_priorities.json";

/// The shared resource namespace fence for folders and native archives.
pub(crate) fn check_namespace(name: &str, manifest: &Manifest) -> Result<(), String> {
    let mut parts = name.splitn(3, '/');
    let (top, namespace, rest) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
    );
    let id = manifest.id.as_str();
    let own = namespace == id;
    let owned = own || manifest.namespaces.iter().any(|n| n == namespace);
    // Vanilla's own files stay vanilla's; a mod may only add to its
    // tags and atlases, and to the convention tags mods share.
    let shared = matches!(namespace, "minecraft" | "c");
    let atlas = top == "assets" && namespace == "minecraft" && rest.starts_with("atlases/");
    let mut extension = false;
    if top == "data" && namespace == "sunrise" && rest.starts_with("tags/") {
        let tag = rest
            .strip_prefix("tags/")
            .and_then(|s| s.strip_suffix(".json"));
        if tag.is_some_and(|t| {
            t.split_once('/')
                .is_some_and(|(_, path)| path.starts_with("class/"))
        }) {
            return Err(format!(
                "{name}: sunrise:class/* tags are generated; attach a part through block_part_tag instead"
            ));
        }
        extension = tag
            .and_then(|t| t.split_once('/'))
            .is_some_and(|(registry, path)| {
                crate::extension_tags::ALL.iter().any(|tag| {
                    tag.registry == registry && tag.id.strip_prefix("sunrise:") == Some(path)
                })
            });
        if !extension {
            return Err(format!(
                "{name}: not a declared Sunrise extension tag; see the SDK extension_tags list and the book's extension tags reference (mod professions use secondary_blocks)"
            ));
        }
    }
    let allowed = owned
        || (top == "data" && shared && rest.starts_with("tags/"))
        || atlas
        || extension
        || name == RECIPE_PRIORITIES_PATH;
    if !allowed {
        return Err(format!(
            "{name}: a mod adds files in its own namespaces ({top}/{id}/ and those listed in namespaces), to vanilla and convention (c) tags, to vanilla's atlases, or to declared shared resource paths"
        ));
    }

    Ok(())
}

/// A vanilla identifier, `minecraft:path`.
/// `Identifier.isValidNamespace` and `isValidPath` on an id with both parts
/// given: `[a-z0-9_.-]`, and `/` in the path too.
pub fn namespaced_id(id: &str) -> bool {
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b);
    id.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace.bytes().all(ok)
            && path.bytes().all(|b| ok(b) || b == b'/')
    })
}

fn vanilla_id(id: &str) -> bool {
    id.strip_prefix("minecraft:").is_some_and(|path| {
        !path.is_empty()
            && path
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
    })
}

/// What a declared block changes from its template; unset keeps the
/// template's value.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockProperties {
    /// Destroy time in seconds of hand mining (-1: unbreakable).
    pub hardness: Option<f32>,
    pub explosion_resistance: Option<f32>,
    /// Light emitted, 0 to 15.
    pub light: Option<u8>,
    /// `true`: the block's parts answer `light_emission`, the light a cell emits by where it is
    /// (`IBlockExtension.hasDynamicLightEmission`); `light` is a cell's without an answer.
    pub dynamic_light: Option<bool>,
    pub requires_tool: Option<bool>,
    pub friction: Option<f32>,
    /// A `MapColor` id, 0 to 61.
    pub map_color: Option<u8>,
    /// A vanilla block whose sounds this block uses.
    pub sounds: Option<String>,
    /// Which states tick randomly, whatever the template does (`isRandomlyTicking`).
    pub random_ticks: Option<RandomTicks>,
    /// How the block is drawn, whatever the template does (`BlockBehaviour.getRenderShape`).
    pub render_shape: Option<RenderShape>,
    /// Vanilla's piston push reaction, or the template's when unset.
    #[serde(default, deserialize_with = "deserialize_push_reaction")]
    pub push_reaction: Option<PushReaction>,
    /// `false`: the block occludes nothing, so its neighbours' faces are drawn and light passes
    /// it (`BlockBehaviour.Properties.noOcclusion`); `true`: occludes whatever the template
    /// does, using the declared outline or the template's published occlusion shape. Unset
    /// inherits the template.
    pub occludes: Option<bool>,
    /// `false`: the breaking crack (`destroy_stage_0..9`) is not drawn on the block, as for a
    /// block broken by use; unset or `true`, it is drawn. Only the overlay: the break and the
    /// destroy-progress packets are vanilla's.
    pub destroy_overlay: Option<bool>,
    /// Per-column model offset and translation of declared outline, collision and interaction shapes.
    /// Block-entity published shapes remain as published. Unset inherits the template.
    #[serde(default, deserialize_with = "deserialize_offset")]
    pub offset: Option<OffsetType>,
    /// Maximum horizontal displacement, 0 to 0.25 (default 0.25 for an explicit offset mode).
    #[serde(default, deserialize_with = "deserialize_horizontal_offset")]
    pub max_horizontal_offset: Option<f32>,
    /// Maximum downward displacement, 0 to 0.2 (default 0.2 for XYZ; XZ has no vertical offset).
    #[serde(default, deserialize_with = "deserialize_vertical_offset")]
    pub max_vertical_offset: Option<f32>,
    /// Vanilla's note-block instrument constant; unset inherits the template.
    #[serde(default, deserialize_with = "deserialize_instrument")]
    pub instrument: Option<mod_vocab::NoteInstrument>,
    /// Lava ignition flag. The per-position `ignited_by_lava` hook still answers first.
    #[serde(default, deserialize_with = "deserialize_ignited_by_lava")]
    pub ignited_by_lava: Option<bool>,
    /// Whether placement and features may replace this block; the held-own-item rule still applies.
    #[serde(default, deserialize_with = "deserialize_replaceable")]
    pub replaceable: Option<bool>,
    /// Skip faces against any state of this same block; false disables the template's render rule.
    /// Leaf and ice graphics culling still follows the template class.
    #[serde(default, deserialize_with = "deserialize_skip_same_block_faces")]
    pub skip_same_block_faces: Option<bool>,
    /// Opt into `VegetationBlock` survival independently of the template.
    #[serde(default)]
    pub vegetation: bool,
}

/// Vanilla's per-column offset mode (`BlockBehaviour.OffsetType`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum OffsetType {
    None,
    Xz,
    Xyz,
}

fn offset_float<'de, D: serde::Deserializer<'de>>(
    d: D,
    key: &str,
) -> Result<Option<f32>, D::Error> {
    Option::<f32>::deserialize(d).map_err(|e| serde::de::Error::custom(format!("{key}: {e}")))
}
fn deserialize_horizontal_offset<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<f32>, D::Error> {
    offset_float(d, "max_horizontal_offset")
}
fn deserialize_vertical_offset<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<f32>, D::Error> {
    offset_float(d, "max_vertical_offset")
}
fn deserialize_offset<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<OffsetType>, D::Error> {
    Option::<OffsetType>::deserialize(d)
        .map_err(|e| serde::de::Error::custom(format!("offset: {e}")))
}

fn deserialize_instrument<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<mod_vocab::NoteInstrument>, D::Error> {
    Option::<String>::deserialize(deserializer)
        .map_err(|e| serde::de::Error::custom(format!("instrument: {e}")))?
        .map(|name| {
            mod_vocab::NoteInstrument::ALL
                .into_iter()
                .find(|instrument| instrument.name().to_ascii_uppercase() == name)
                .ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        "instrument: unknown NoteBlockInstrument constant {name}"
                    ))
                })
        })
        .transpose()
}

fn property_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
    key: &str,
) -> Result<Option<bool>, D::Error> {
    Option::<bool>::deserialize(deserializer)
        .map_err(|e| serde::de::Error::custom(format!("{key}: {e}")))
}
fn deserialize_ignited_by_lava<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<bool>, D::Error> {
    property_bool(d, "ignited_by_lava")
}
fn deserialize_replaceable<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<bool>, D::Error> {
    property_bool(d, "replaceable")
}
fn deserialize_skip_same_block_faces<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<bool>, D::Error> {
    property_bool(d, "skip_same_block_faces")
}

/// Vanilla's `PushReaction`, by its constant's name.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PushReaction {
    PushPull,
    Push,
    Popped,
    Immoveable,
    IgnoreEntity,
}

impl PushReaction {
    /// The constant's name, as the block tables write it.
    pub fn name(self) -> &'static str {
        match self {
            Self::PushPull => "PUSH_PULL",
            Self::Push => "PUSH",
            Self::Popped => "POPPED",
            Self::Immoveable => "IMMOVEABLE",
            Self::IgnoreEntity => "IGNORE_ENTITY",
        }
    }
}

fn deserialize_push_reaction<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PushReaction>, D::Error> {
    Option::<PushReaction>::deserialize(deserializer)
        .map_err(|e| serde::de::Error::custom(format!("push_reaction: {e}")))
}

/// Vanilla's `RenderShape`, by its constant's name: `MODEL` puts the block model in the chunk
/// mesh, `ENTITYBLOCK_ANIMATED` leaves it out for a block whose parts are drawn every frame
/// (its block entity's drawing), and `INVISIBLE` draws nothing.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RenderShape {
    Model,
    EntityblockAnimated,
    Invisible,
}

impl RenderShape {
    /// The constant's name, as the block tables write it.
    pub fn name(self) -> &'static str {
        match self {
            RenderShape::Model => "MODEL",
            RenderShape::EntityblockAnimated => "ENTITYBLOCK_ANIMATED",
            RenderShape::Invisible => "INVISIBLE",
        }
    }
}

/// Which of a block's states get random ticks (`BlockBehaviour.isRandomlyTicking(state)`), so a
/// crop, sapling or soil on a template that doesn't tick still grows. The sections' random tick
/// counts follow it, and `native_random_tick` fires for the states it picks.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RandomTicks {
    /// Every state (`true`), or none (`false`, for a template that ticks).
    All(bool),
    /// The states whose own state property `property` is one of `values`.
    States {
        property: String,
        values: Vec<String>,
    },
}

/// Values selected by one property or an ordered tuple of state properties.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StateValues<T> {
    #[serde(default)]
    pub property: String,
    #[serde(default)]
    pub properties: Vec<String>,
    #[serde(default)]
    pub values: BTreeMap<String, T>,
    pub default: Option<T>,
}

/// Block-local boxes selected by an ordered tuple of state properties.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockShapes(pub StateValues<Vec<[f64; 6]>>);

impl<'de> Deserialize<'de> for BlockShapes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Shapes {
            #[serde(default)]
            properties: Vec<String>,
            #[serde(default)]
            values: BTreeMap<String, Vec<[f64; 6]>>,
            default: Option<Vec<[f64; 6]>>,
        }
        let shapes = Shapes::deserialize(deserializer)?;
        Ok(Self(StateValues {
            property: String::new(),
            properties: shapes.properties,
            values: shapes.values,
            default: shapes.default,
        }))
    }
}

impl std::ops::Deref for BlockShapes {
    type Target = StateValues<Vec<[f64; 6]>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for BlockShapes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Light emitted by a state, as an integer from 0 to 15.
pub type StateLight = StateValues<u8>;

impl<T> StateValues<T> {
    pub fn property_names(&self) -> Vec<&str> {
        if self.property.is_empty() {
            self.properties.iter().map(String::as_str).collect()
        } else {
            vec![self.property.as_str()]
        }
    }

    fn check_selection(&self, label: &str) -> Result<(), String> {
        let names = self.property_names();
        if (!self.property.is_empty() && !self.properties.is_empty())
            || names.iter().any(|name| !state_name(name))
            || names.iter().collect::<BTreeSet<_>>().len() != names.len()
            || (names.is_empty() && !self.values.is_empty())
            || (self.values.is_empty() && self.default.is_none())
        {
            return Err(format!(
                "{label} needs distinct properties and values, or a default"
            ));
        }
        for key in self.values.keys() {
            let values: Vec<_> = key.split(',').collect();
            if values.len() != names.len() || values.iter().any(|value| !state_name(value)) {
                return Err(format!(
                    "invalid {label} key {key:?}: one value per property"
                ));
            }
        }
        Ok(())
    }
}

impl BlockShapes {
    fn check(&self) -> Result<(), String> {
        self.check_selection("shapes")?;
        for boxes in self.values.values().chain(self.default.iter()) {
            // Vanilla's shapes have no box limit, so neither does a declared one.
            if boxes.iter().any(|bounds| {
                bounds.iter().any(|value| !value.is_finite())
                    || (0..3).any(|axis| bounds[axis] > bounds[axis + 3])
            }) {
                return Err("shape needs finite boxes with ordered bounds".into());
            }
        }
        Ok(())
    }
}

impl StateLight {
    fn check(&self) -> Result<(), String> {
        self.check_selection("state_light")?;
        if self
            .values
            .values()
            .chain(self.default.iter())
            .any(|value| *value > 15)
        {
            return Err("state_light values are integers 0 to 15".into());
        }
        Ok(())
    }
}

/// The querying entity class, captured by the real caller. Empty and position-only
/// queries have no entity and always use the ordinary shape tables.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ShapeEntityKind {
    Item,
    Player,
    Living,
    Other,
}
impl ShapeEntityKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Player => "player",
            Self::Living => "living",
            Self::Other => "other",
        }
    }
}

/// A vanilla block's context-dependent collision a block takes (`context_collision`), whatever
/// its template.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContextCollision {
    /// Scaffolding's (`ScaffoldingBlock.getCollisionShape`): solid on top unless the entity is
    /// descending, open below, and the bottom row's floor. Its states need scaffolding's
    /// `distance` (0 to 7) and `bottom` properties, from the template or declared.
    Scaffolding,
}

/// A state property a block declares on its own, beside its template's.
/// A block's states become the template's states times these values, so a
/// pipe can have a connection per side without a template that has one.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StateProperty {
    /// `false` and `true`, in vanilla's spelling.
    Boolean,
    /// A list of values, in the order the state space is built in.
    Enum { values: Vec<String> },
}

impl StateProperty {
    /// Its values, in the order the state space is built in; a boolean's two
    /// come `false` first, as vanilla orders them.
    pub fn values(&self) -> Vec<&str> {
        match self {
            StateProperty::Boolean => vec!["false", "true"],
            StateProperty::Enum { values } => values.iter().map(String::as_str).collect(),
        }
    }
}

/// A vanilla state property's name, and a value's: lower case, digits and
/// underscores (`minecraft:oak_log` is `axis`, a repeater is `delay`).
fn state_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockFile {
    template: String,
    #[serde(default)]
    properties: BlockProperties,
    shapes: Option<BlockShapes>,
    collision_shapes: Option<BlockShapes>,
    #[serde(default)]
    outline_context_shapes: BTreeMap<ShapeEntityKind, BlockShapes>,
    #[serde(default)]
    collision_context_shapes: BTreeMap<ShapeEntityKind, BlockShapes>,
    interaction_shapes: Option<BlockShapes>,
    #[serde(default)]
    shapes_from_block_entity: bool,
    context_collision: Option<ContextCollision>,
    state_light: Option<StateLight>,
    #[serde(default)]
    state_properties: BTreeMap<String, StateProperty>,
    #[serde(default)]
    default_state: BTreeMap<String, String>,
    #[serde(default)]
    connects_to: Option<String>,
    #[serde(default)]
    thickness: Option<f32>,
    #[serde(default)]
    wood_type: Option<String>,
    #[serde(default)]
    wall_block: Option<String>,
    #[serde(default)]
    sign_text_color: Option<String>,
    #[serde(default)]
    sign_text_glow: Option<bool>,
    #[serde(default)]
    base_block: Option<String>,
    #[serde(default = "yes")]
    item: bool,
    #[serde(default, deserialize_with = "GateSound::deserialize_open")]
    open_sound: Option<GateSound>,
    #[serde(default, deserialize_with = "GateSound::deserialize_close")]
    close_sound: Option<GateSound>,
    block_item: Option<String>,
    #[serde(default)]
    pick_item: Option<String>,
    #[serde(default)]
    components: BTreeMap<String, serde_json::Value>,
    suspicious_stew_effects: Option<Vec<StewEffect>>,
}

/// One entry of a block's suspicious stew effect list, in the item component's JSON form.
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
pub struct StewEffect {
    pub id: String,
    /// Duration in ticks. Omitted or invalid durations use 160.
    #[serde(
        default = "stew_duration",
        deserialize_with = "deserialize_stew_duration"
    )]
    pub duration: i32,
}

fn stew_duration() -> i32 {
    160
}

fn deserialize_stew_duration<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<i32, D::Error> {
    Ok(serde_json::Value::deserialize(deserializer)?
        .as_i64()
        .and_then(|duration| i32::try_from(duration).ok())
        .unwrap_or_else(stew_duration))
}

fn yes() -> bool {
    true
}

/// A fence gate's sound event at every site, or only the named sites.
/// Missing sites keep the template wood's sounds.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum GateSound {
    All(String),
    Sites {
        #[serde(rename = "use")]
        on_use: Option<String>,
        #[serde(rename = "explosion")]
        on_explosion: Option<String>,
        #[serde(rename = "redstone")]
        on_redstone: Option<String>,
    },
}

impl GateSound {
    fn deserialize_open<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Self>, D::Error> {
        Option::<Self>::deserialize(d)
            .map_err(|e| serde::de::Error::custom(format!("open_sound: {e}")))
    }
    fn deserialize_close<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Self>, D::Error> {
        Option::<Self>::deserialize(d)
            .map_err(|e| serde::de::Error::custom(format!("close_sound: {e}")))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreativeTabFile {
    title: String,
    icon: String,
    items: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, sunrise_mod_api_macros::Wire)]
#[wire(crate = ::mod_vocab::wire)]
#[serde(deny_unknown_fields)]
struct FluidFile {
    sprite: Option<String>,
    tint: Option<u32>,
    world: Option<WorldFluid>,
}

/// What makes a fluid exist in the world: a block holding it, as `minecraft:water`'s does, and
/// how it flows. Each value defaults to water's, in `data/<id>/sunrise/fluid/` as in code
/// (`Registrar::declare_world_fluid`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
#[wire(crate = ::mod_vocab::wire)]
#[serde(default, deny_unknown_fields)]
pub struct WorldFluid {
    /// Game ticks between two flow steps (`Fluid.getTickDelay`), 1 or more.
    pub tick_delay: i32,
    /// What it loses per block it flows (`FlowingFluid.getDropOff`), 1 to 7.
    pub drop_off: i32,
    /// How far it looks for a way down (`FlowingFluid.getSlopeFindDistance`), 1 to 4.
    pub slope_find_distance: i32,
    /// Whether two sources make a third (`FlowingFluid.canConvertToSource`). Off by default:
    /// water's comes from a game rule, which a mod's fluid does not read.
    pub source_conversion: bool,
    /// Whether it has a filled bucket, the item `<id>_bucket`.
    pub bucket: bool,
}
impl Default for WorldFluid {
    fn default() -> Self {
        Self {
            tick_delay: 5,
            drop_off: 1,
            slope_find_distance: 4,
            source_conversion: false,
            bucket: true,
        }
    }
}
impl WorldFluid {
    /// Refuses values the simulation cannot run: no step, no spread, or an unbounded search.
    pub fn check(&self) -> Result<(), String> {
        if self.tick_delay < 1 {
            return Err("tick_delay is 1 or more".into());
        }
        if !(1..=7).contains(&self.drop_off) {
            return Err("drop_off is 1 to 7".into());
        }
        if !(1..=4).contains(&self.slope_find_distance) {
            return Err("slope_find_distance is 1 to 4".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemFile {
    template: String,
    max_stack_size: Option<u8>,
    rarity: Option<Rarity>,
    #[serde(default)]
    components: BTreeMap<String, serde_json::Value>,
    crafting_remainder: Option<String>,
    block: Option<String>,
    arm_pose: Option<ArmPose>,
}

/// `MobEffectCategory`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EffectCategory {
    Beneficial,
    Harmful,
    Neutral,
}

/// A mob effect the mod adds: what `new MobEffect(category, color)` takes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MobEffect {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    pub category: EffectCategory,
    /// RGB, `0x000000` to `0xFFFFFF`.
    pub color: u32,
    /// Where the effect sits in the HUD icons and the inventory's effect list
    /// (NeoForge's `IMobEffectExtension.getSortOrder`): compared where vanilla
    /// compares the colour, which it is when absent.
    #[serde(default)]
    pub sort_order: Option<i32>,
}

/// Vanilla's `DyeColor` names, by id.
pub const DYE_COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// the vanilla entity types a mod's type may copy: thrown item
/// projectiles (`ThrowableItemProjectile`), then living mobs
/// ([`LIVING_TEMPLATES`]).
pub const ENTITY_TEMPLATES: [&str; 9] = [
    "minecraft:snowball",
    "minecraft:cat",
    "minecraft:drowned",
    "minecraft:parrot",
    "minecraft:skeleton",
    "minecraft:villager",
    "minecraft:wandering_trader",
    "minecraft:wolf",
    "minecraft:zombie",
];

/// the [`ENTITY_TEMPLATES`] that are living mobs: a type copying one is a mob
/// of its template's class (NeoForge's `EntityType.Builder.of(Zombie::new,
/// ...)`), with its own default attributes (`EntityAttributeCreationEvent`).
/// The wandering trader is the neutral one whose behaviour is all goals,
/// which a native mod can replace (`register_goals`); a villager's is its
/// brain. The cat, the parrot and the wolf are the tameable ones
/// (`TamableAnimal`): a type copying one tames, sits and follows its owner.
/// The skeleton is the ranged one (`AbstractSkeleton`): a type copying it
/// draws and shoots a bow it holds, shuns the sun and flees wolves. The
/// drowned is the aquatic fighter: a type copying it swims, goes to water,
/// and fights with a trident or in melee.
pub const LIVING_TEMPLATES: [&str; 8] = [
    "minecraft:cat",
    "minecraft:drowned",
    "minecraft:parrot",
    "minecraft:skeleton",
    "minecraft:villager",
    "minecraft:wandering_trader",
    "minecraft:wolf",
    "minecraft:zombie",
];

pub mod entity_sounds;
mod entity_type;
pub use entity_type::EntityType;
pub mod fluid_interaction;
pub use fluid_interaction::FluidInteraction;
mod lenient;
pub use lenient::without_comments;
#[cfg(feature = "references")]
mod references;
#[cfg(feature = "references")]
pub use references::{Findings, check_references};

/// a menu type the mod adds (vanilla's `MenuType`, NeoForge's
/// `IMenuTypeExtension.create`): its container's slot count, the
/// `ContainerData` count its menu syncs, and its take-only slots
/// (`FurnaceResultSlot.mayPlace`). The container slots come first, then the
/// player's 36 if included. A mod screen (`native_screen`) places them.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MenuType {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    /// 0 to [`MAX_MENU_SLOTS`].
    pub slots: u16,
    /// Append the 36 player inventory slots (default: true).
    #[serde(default = "yes")]
    pub player_inventory: bool,
    /// 0 to [`MAX_MENU_SLOTS`].
    #[serde(default)]
    pub data_slots: u16,
    /// Per-slot maxima for transient slots when the menu is opened with no backing block.
    /// When present, supplies one maximum (1–99) for each container slot. Cannot be combined
    /// with take-only `results` or a `crafting` layout.
    #[serde(default)]
    pub owned_slots: Option<Vec<u8>>,
    /// slots a player takes from but never puts into, each below `slots`.
    #[serde(default)]
    pub results: Vec<u16>,
    /// Zero-based machine input slots, disjoint from results and crafting slots.
    /// This is a client layout role, not an item acceptance or transfer rule.
    #[serde(default)]
    pub inputs: Vec<u16>,
    /// a crafting grid over some of the slots, which crafts as vanilla's
    /// crafting table does (`CraftingMenu`).
    #[serde(default)]
    pub crafting: Option<MenuCrafting>,
    /// slots that show other containers' slots instead of the block
    /// entity's, as a crafting station shows the chests beside it; a native
    /// mod names the containers (`native_menu_links`).
    #[serde(default)]
    pub links: Option<MenuLinks>,
}

/// a mod menu type's linked slots (`links`): `count` slots from `from` on,
/// which show the slots of the containers a native mod links them to
/// (`native_menu_links`), in order, rather than the block entity's. The
/// block entity never holds them; a linked slot with nothing behind it holds
/// nothing and takes nothing.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MenuLinks {
    /// the first linked slot.
    pub from: u16,
    /// how many slots, 1 or more.
    pub count: u16,
}

impl MenuLinks {
    fn range(&self) -> std::ops::Range<u32> {
        u32::from(self.from)..u32::from(self.from) + u32::from(self.count)
    }
}

/// a mod menu type's crafting grid (`crafting`): `size`x`size` slots from
/// `grid` on, row by row, and the slot that shows what they craft
/// (`ResultSlot`). The block entity holds the grid; the result is the
/// menu's, never in the block entity.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MenuCrafting {
    /// the grid's first slot.
    pub grid: u16,
    /// 2 (the inventory's grid) or 3 (a crafting table's); 3 by default.
    #[serde(default = "MenuCrafting::default_size")]
    pub size: u16,
    /// the result slot, outside the grid and not one of `results`.
    pub result: u16,
}

impl MenuCrafting {
    fn default_size() -> u16 {
        3
    }
}

/// the most container or data slots a mod menu type has, as many as a mod
/// container (`native_block_entity_container`) may.
pub const MAX_MENU_SLOTS: u16 = 256;

impl Content {
    /// Where content may go, and the declarations.
    fn check(&mut self, manifest: &Manifest, declared: &[String]) -> Result<(), String> {
        let id = manifest.id.as_str();
        let mut blocks = Vec::new();
        let mut items = Vec::new();
        let mut effects = Vec::new();
        let mut tabs = Vec::new();
        let mut fluids = Vec::new();
        let mut interactions = Vec::new();
        let mut types = Vec::new();
        let mut menus = Vec::new();
        for name in &self.files {
            let mut parts = name.splitn(3, '/');
            let (top, namespace, rest) = (
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
            );
            let owned = manifest.owned_namespaces().any(|owned| owned == namespace);
            check_namespace(name, manifest)?;
            let Some(decl) = (owned && top == "data")
                .then(|| rest.strip_prefix("sunrise/"))
                .flatten()
            else {
                continue;
            };
            // A server package's registry declaration refuses the whole package by name
            // (`server_only_refusal`), so it is not parsed: a malformed one must not turn
            // that refusal into a skipped content error.
            if manifest.side == crate::manifest::Side::Server {
                continue;
            }
            let parse = |kind: &str| -> Option<String> {
                let path = decl
                    .strip_prefix(kind)?
                    .strip_prefix('/')?
                    .strip_suffix(".json")?;
                Some(format!("{namespace}:{path}"))
            };
            if let Some(block) = parse("block") {
                blocks.push((block, name.clone()));
            } else if let Some(item) = parse("item") {
                items.push((item, name.clone()));
            } else if let Some(effect) = parse("mob_effect") {
                effects.push((effect, name.clone()));
            } else if let Some(tab) = parse("creative_tab") {
                tabs.push((tab, name.clone()));
            } else if let Some(fluid) = parse("fluid") {
                fluids.push((fluid, name.clone()));
            } else if let Some(interaction) = parse("fluid_interaction") {
                interactions.push((interaction, name.clone()));
            } else if let Some(kind) = parse("entity_type") {
                types.push((kind, name.clone()));
            } else if let Some(menu) = parse("menu") {
                menus.push((menu, name.clone()));
            } else {
                return Err(format!(
                    "{name}: data/{id}/sunrise/ holds block/, item/, mob_effect/, creative_tab/, fluid/, fluid_interaction/, entity_type/ and menu/*.json only"
                ));
            }
        }
        // One reader for every declaration: opening a packaged mod reads its
        // whole index, so a reader per file made loading a big mod quadratic.
        let source = self.source.clone();
        let mut reader = reader_of(&source)?;
        let mut ids = BTreeSet::new();
        for (block, name) in blocks {
            let file: BlockFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file
                .connects_to
                .as_ref()
                .is_some_and(|t| !t.starts_with('#'))
            {
                return Err(format!(
                    "{name}: connects_to is a block tag, #namespace:path"
                ));
            }
            let block = Block {
                id: block,
                template: file.template,
                properties: file.properties,
                shapes: file.shapes,
                collision_shapes: file.collision_shapes,
                outline_context_shapes: file.outline_context_shapes,
                collision_context_shapes: file.collision_context_shapes,
                interaction_shapes: file.interaction_shapes,
                shapes_from_block_entity: file.shapes_from_block_entity,
                context_collision: file.context_collision,
                state_light: file.state_light,
                state_properties: file.state_properties,
                default_state: file.default_state,
                item: file.item,
                open_sound: file.open_sound,
                close_sound: file.close_sound,
                block_item: file.block_item,
                pick_item: file.pick_item,
                components: file.components,
                suspicious_stew_effects: file.suspicious_stew_effects,
                connects_to: file
                    .connects_to
                    .map(|t| t.trim_start_matches('#').to_owned()),
                thickness: file.thickness,
                wood_type: file.wood_type,
                wall_block: file.wall_block,
                sign_text_color: file.sign_text_color,
                sign_text_glow: file.sign_text_glow,
                base_block: file.base_block,
            };
            check_block(&block).map_err(|e| format!("{name}: {e}"))?;
            if block.item {
                ids.insert(block.id.clone());
            }
            self.blocks.push(block);
        }
        // After every block, so a wall form may be declared in a later file.
        if let Some((id, reason)) =
            sign_wall_refusals(&self.blocks.iter().collect::<Vec<_>>()).next()
        {
            return Err(format!("{id}: {reason}"));
        }
        let mut remainders = Vec::new();
        for (item, name) in items {
            let file: ItemFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            let item = Item {
                id: item,
                template: file.template,
                max_stack_size: file.max_stack_size,
                rarity: file.rarity,
                components: file.components,
                crafting_remainder: file.crafting_remainder,
                block: file.block,
                arm_pose: file.arm_pose,
            };
            check_item(&item).map_err(|e| format!("{name}: {e}"))?;
            if !ids.insert(item.id.clone()) {
                return Err(format!("{name}: {} is also a block's item", item.id));
            }
            if let Some(rest) = &item.crafting_remainder {
                remainders.push((name, rest.clone()));
            }
            self.items.push(item);
        }
        for (effect, name) in effects {
            let file: MobEffect =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file.color > 0xFF_FFFF {
                return Err(format!("{name}: color is RGB, 0 to 16777215 (0xFFFFFF)"));
            }
            self.effects.push(MobEffect { id: effect, ..file });
        }
        // After every item, so a remainder may be declared in a later file.
        for (name, rest) in remainders {
            if rest == "minecraft:air" {
                return Err(format!(
                    "{name}: crafting_remainder {rest:?} is no remainder (leave it out)"
                ));
            }
            if !vanilla_id(&rest) && !ids.contains(&rest) && !declared.contains(&rest) {
                return Err(format!(
                    "{name}: crafting_remainder {rest:?} is not a vanilla item (minecraft:...) or one of this mod's"
                ));
            }
        }
        for (tab, name) in tabs {
            let file: CreativeTabFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            let id_like = |s: &str| {
                s.split_once(':')
                    .is_some_and(|(n, p)| !n.is_empty() && !p.is_empty())
            };
            if !id_like(&file.icon) || !file.items.iter().all(|i| id_like(i)) {
                return Err(format!(
                    "{name}: icon and items are item ids (namespace:path)"
                ));
            }
            self.creative_tabs.push(CreativeTab {
                id: tab,
                title: file.title,
                icon: file.icon,
                items: file.items,
            });
        }
        for (fluid, name) in fluids {
            let file: FluidFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file.sprite.as_deref().is_some_and(|s| !namespaced_id(s)) {
                return Err(format!(
                    "{name}: sprite is an atlas sprite id (namespace:path)"
                ));
            }
            if let Some(world) = &file.world {
                world.check().map_err(|e| format!("{name}: world: {e}"))?;
            }
            let look = FluidLook {
                id: fluid,
                sprite: file.sprite,
                tint: file.tint,
                world: file.world,
            };
            if self.fluids.iter().any(|known| {
                known.id == look.id
                    || (known.world.is_some() && known.flowing_id() == look.id)
                    || (look.world.is_some()
                        && (look.flowing_id() == known.id
                            || (known.world.is_some() && look.flowing_id() == known.flowing_id())))
            }) {
                return Err(format!("{name}: fluid name declared twice"));
            }
            if let Some(block) = look.world_block() {
                if self.blocks.iter().any(|known| known.id == block.id) {
                    return Err(format!("{name}: block {} is declared twice", block.id));
                }
                self.blocks.push(block);
            }
            if let Some(item) = look.world_bucket() {
                if self.items.iter().any(|known| known.id == item.id) {
                    return Err(format!("{name}: item {} is declared twice", item.id));
                }
                self.items.push(item);
            }
            self.fluids.push(look);
        }
        for (_, name) in interactions {
            let file: FluidInteraction =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            file.check().map_err(|e| format!("{name}: {e}"))?;
            self.fluid_interactions.push(file);
        }
        for (kind, name) in types {
            let file: EntityType =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            file.sounds.validate(&kind)?;
            if !ENTITY_TEMPLATES.contains(&file.template.as_str()) {
                return Err(format!(
                    "{name}: template {:?} is not one of {ENTITY_TEMPLATES:?}",
                    file.template
                ));
            }
            if file.is_living() {
                // a mob is spawned, not thrown: nothing throws it, and its
                // attack is its `minecraft:attack_damage`.
                if file.item.is_some() || file.damage != 0.0 {
                    return Err(format!(
                        "{name}: a living type has no item or damage (its template is {})",
                        file.template
                    ));
                }
                for (attribute, value) in &file.attributes {
                    let id_like = attribute
                        .split_once(':')
                        .is_some_and(|(n, p)| !n.is_empty() && !p.is_empty());
                    if !id_like {
                        return Err(format!(
                            "{name}: attribute {attribute:?} is an attribute id (namespace:path)"
                        ));
                    }
                    if !value.is_finite() {
                        return Err(format!("{name}: attribute {attribute} is a finite number"));
                    }
                }
                for pt in &file.passenger_attachments {
                    if !pt.iter().all(|f| f.is_finite()) {
                        return Err(format!("{name}: passenger attachments are finite numbers"));
                    }
                }
                self.entity_types.push(EntityType { id: kind, ..file });
                continue;
            }
            file.validate_thrown_overrides(&name)?;
            let Some(item) = &file.item else {
                return Err(format!(
                    "{name}: a thrown type names the item that throws it"
                ));
            };
            // its own item, so no vanilla item loses its use to it.
            if !ids.contains(item) {
                return Err(format!(
                    "{name}: item {item:?} is not one of this mod's items"
                ));
            }
            if self
                .entity_types
                .iter()
                .any(|t| t.item.as_ref() == Some(item))
            {
                return Err(format!(
                    "{name}: item {item:?} already throws another entity type"
                ));
            }
            if !(file.damage.is_finite() && file.damage >= 0.0) {
                return Err(format!("{name}: damage is 0 or more"));
            }
            self.entity_types.push(EntityType { id: kind, ..file });
        }
        for (menu, name) in menus {
            let bytes = reader.read(&name)?;
            let source = format!("{name} ({menu})");
            let file: MenuType =
                serde_json::from_slice(&bytes).map_err(|e| format!("{source}: {e}"))?;
            if file.slots > MAX_MENU_SLOTS || file.data_slots > MAX_MENU_SLOTS {
                return Err(format!(
                    "{source}: slots ({}) and data_slots ({}) are each 0 to {MAX_MENU_SLOTS}",
                    file.slots, file.data_slots
                ));
            }
            if let Some(limits) = &file.owned_slots
                && (limits.len() != usize::from(file.slots)
                    || limits.iter().any(|limit| !(1..=99).contains(limit)))
            {
                return Err(format!(
                    "{name} ({menu}): owned_slots must give one maximum in 1..=99 per container slot"
                ));
            }
            if file.owned_slots.is_some() && (!file.results.is_empty() || file.crafting.is_some()) {
                return Err(format!(
                    "{source}: owned_slots cannot be combined with results or crafting"
                ));
            }
            let mut seen = BTreeSet::new();
            for &r in &file.results {
                if r >= file.slots {
                    return Err(format!(
                        "{source}: result slot {r} is outside the {} container slots",
                        file.slots
                    ));
                }
                if !seen.insert(r) {
                    return Err(format!("{source}: result {r} is listed twice"));
                }
            }
            for &input in &file.inputs {
                if input >= file.slots {
                    return Err(format!(
                        "{source}: input {input} is outside the {} container slots",
                        file.slots
                    ));
                }
                if !seen.insert(input) {
                    return Err(format!(
                        "{source}: input {input} is listed twice or is a result"
                    ));
                }
            }
            if let Some(c) = file.crafting {
                let end = u32::from(c.grid) + u32::from(c.size) * u32::from(c.size);
                if !matches!(c.size, 2 | 3) {
                    return Err(format!("{source}: crafting size is 2 or 3"));
                }
                if end > u32::from(file.slots) || c.result >= file.slots {
                    return Err(format!(
                        "{source}: crafting grid {}..{end} and result slot {} must be within the {} container slots",
                        c.grid, c.result, file.slots
                    ));
                }
                if (u32::from(c.grid)..end).contains(&u32::from(c.result)) {
                    return Err(format!("{source}: the crafting result is in its grid"));
                }
                let is_crafting_slot =
                    |&s: &u16| s == c.result || (u32::from(c.grid)..end).contains(&u32::from(s));
                if file.results.iter().any(is_crafting_slot) {
                    return Err(format!(
                        "{source}: results lists a crafting slot (the result is take-only already)"
                    ));
                }
                if file.inputs.iter().any(is_crafting_slot) {
                    return Err(format!("{source}: inputs lists a crafting slot"));
                }
            }
            if let Some(l) = file.links {
                let linked = l.range();
                if l.count == 0 || linked.end > u32::from(file.slots) {
                    return Err(format!(
                        "{source}: linked slots {linked:?} must be nonempty and within the {} container slots",
                        file.slots
                    ));
                }
                if file.results.iter().any(|&r| linked.contains(&u32::from(r))) {
                    return Err(format!("{source}: results lists a linked slot"));
                }
                if let Some(c) = file.crafting {
                    let end = u32::from(c.grid) + u32::from(c.size) * u32::from(c.size);
                    if linked.contains(&u32::from(c.result))
                        || (u32::from(c.grid)..end).any(|s| linked.contains(&s))
                    {
                        return Err(format!("{source}: links overlap the crafting slots"));
                    }
                }
            }
            self.menu_types.push(MenuType { id: menu, ..file });
        }
        Ok(())
    }
}

/// Namespaced component types, removals as `{}`, and none of `fields`,
/// which the file sets with its own keys.
fn check_components(
    components: &BTreeMap<String, serde_json::Value>,
    fields: &[&str],
) -> Result<(), String> {
    for (key, value) in components {
        let (removed, kind) = match key.strip_prefix('!') {
            Some(kind) => (true, kind),
            None => (false, key.as_str()),
        };
        if !namespaced_id(kind) {
            return Err(format!("component {key:?} must be namespaced and valid"));
        }
        if let Some(own) = kind
            .strip_prefix("minecraft:")
            .filter(|k| fields.contains(k))
        {
            return Err(format!("component {key:?}: set {own} instead"));
        }
        if removed && !value.as_object().is_some_and(|o| o.is_empty()) {
            return Err(format!("component {key:?}: a removal's value is {{}}"));
        }
    }
    Ok(())
}

/// An item's fields, as a file or code gives them.
fn check_item(item: &Item) -> Result<(), String> {
    if !vanilla_id(&item.template) {
        return Err(format!(
            "template {:?} is not a vanilla item (minecraft:...)",
            item.template
        ));
    }
    if item.block.as_ref().is_some_and(|id| !namespaced_id(id)) {
        return Err("block must be a namespaced block id".into());
    }
    if item.max_stack_size.is_some_and(|n| !(1..=99).contains(&n)) {
        return Err("max_stack_size is 1 to 99".into());
    }
    if let Some(block) = &item.block
        && (!namespaced_id(block)
            || block.split_once(':').map(|(ns, _)| ns) != item.id.split_once(':').map(|(ns, _)| ns)
                && !vanilla_id(block))
    {
        return Err("block is a vanilla block or a block in this item's namespace".into());
    }
    check_components(&item.components, &["max_stack_size", "rarity"])
}

/// A block's fields, as a file or code gives them (`connects_to` without
/// its `#`).
fn check_block(file: &Block) -> Result<(), String> {
    if !vanilla_id(&file.template) {
        return Err(format!(
            "template {:?} is not a vanilla block (minecraft:...)",
            file.template
        ));
    }
    for shapes in file
        .shapes
        .iter()
        .chain(file.collision_shapes.iter())
        .chain(file.outline_context_shapes.values())
        .chain(file.collision_context_shapes.values())
        .chain(file.interaction_shapes.iter())
    {
        shapes.check()?;
    }
    if let Some(light) = &file.state_light {
        light.check()?;
    }
    for (name, property) in &file.state_properties {
        if name == "waterlogged" && !matches!(property, StateProperty::Boolean) {
            return Err("state_properties.waterlogged must have type boolean".into());
        }
        if !state_name(name) {
            return Err(format!(
                "state property {name:?} is lower case letters, digits and underscores"
            ));
        }
        let values = property.values();
        if values.is_empty() {
            return Err(format!("state property {name:?} needs a value"));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            if !state_name(value) {
                return Err(format!(
                    "state property {name:?}: value {value:?} is lower case letters, digits and underscores"
                ));
            }
            if !seen.insert(value) {
                return Err(format!("state property {name:?}: value {value:?} repeats"));
            }
        }
    }
    for key in file.default_state.keys() {
        if !state_name(key) {
            return Err(format!(
                "default_state key {key:?} is lower case letters, digits and underscores"
            ));
        }
    }
    let p = &file.properties;
    if let Some(RandomTicks::States { property, values }) = &p.random_ticks {
        let declared = file
            .state_properties
            .get(property)
            .map(StateProperty::values);
        if declared.is_none_or(|all| {
            values.is_empty() || values.iter().any(|v| !all.contains(&v.as_str()))
        }) {
            return Err(format!(
                "random_ticks needs values of the block's own state property {property:?} (state_properties)"
            ));
        }
    }
    for (key, value, max) in [
        ("max_horizontal_offset", p.max_horizontal_offset, 0.25),
        ("max_vertical_offset", p.max_vertical_offset, 0.2),
    ] {
        if value.is_some_and(|v| !v.is_finite() || !(0.0..=max).contains(&v)) {
            return Err(format!("{key} is a finite number from 0 to {max}"));
        }
    }
    let finite = |v: Option<f32>| v.is_none_or(f32::is_finite);
    if !finite(p.hardness) || !finite(p.explosion_resistance) || !finite(p.friction) {
        return Err("hardness, explosion_resistance and friction are numbers".into());
    }
    if p.hardness.is_some_and(|h| h < 0.0 && h != -1.0) {
        return Err("hardness is 0 or more, or -1 (unbreakable)".into());
    }
    if p.explosion_resistance.is_some_and(|r| r < 0.0) {
        return Err("explosion_resistance is 0 or more".into());
    }
    if p.light.is_some_and(|l| l > 15) {
        return Err("light is 0 to 15".into());
    }
    if p.map_color.is_some_and(|c| c > 61) {
        return Err("map_color is a MapColor id, 0 to 61".into());
    }
    if p.sounds.as_deref().is_some_and(|s| !vanilla_id(s)) {
        return Err("sounds names a vanilla block (minecraft:...)".into());
    }
    if !file.item && !file.components.is_empty() {
        return Err("item is false, so the block has no item to take components".into());
    }
    check_components(&file.components, &[])?;
    // A block file has no `max_stack_size` / `rarity` keys, so those two take the same checks here.
    if let Some(n) = file.components.get("minecraft:max_stack_size")
        && !n.as_u64().is_some_and(|n| (1..=99).contains(&n))
    {
        return Err("component \"minecraft:max_stack_size\" is an integer 1 to 99".into());
    }
    if let Some(r) = file.components.get("minecraft:rarity")
        && serde_json::from_value::<Rarity>(r.clone()).is_err()
    {
        return Err("component \"minecraft:rarity\" is common, uncommon, rare or epic".into());
    }
    if let Some(tag) = &file.connects_to {
        if file.template != "minecraft:chorus_plant" {
            return Err("connects_to needs template minecraft:chorus_plant (a PipeBlock)".into());
        }
        let id = tag.as_str();
        let valid = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c))
        };
        if !id
            .split_once(':')
            .is_some_and(|(ns, path)| valid(ns) && valid(path))
        {
            return Err("connects_to is a block tag, #namespace:path".into());
        }
    }
    match file.thickness {
        Some(_) if file.connects_to.is_none() => {
            return Err("thickness is a connecting block's (connects_to)".into());
        }
        Some(t) if !(2.0..=14.0).contains(&t) => return Err("thickness is 2 to 14 pixels".into()),
        Some(_) if file.shapes.is_some() => {
            return Err("thickness sets the shapes, so a block has one or the other".into());
        }
        _ => {}
    }
    if let Some(base) = &file.base_block {
        if file.template != "minecraft:farmland" {
            return Err("base_block needs template minecraft:farmland".into());
        }
        if !namespaced_id(base) {
            return Err("base_block is a block id, namespace:path".into());
        }
    }
    // Every vanilla `SignBlock`'s id ends in `_sign`, and no other block's does.
    let sign = file.template.ends_with("_sign");
    if let Some(wood) = &file.wood_type {
        if !sign {
            return Err("wood_type needs a vanilla sign template".into());
        }
        let (namespace, path) = wood.split_once(':').unwrap_or(("minecraft", wood));
        if !namespaced_id(&format!("{namespace}:{path}"))
            || check_path(&format!("assets/{namespace}/{path}")).is_err()
        {
            return Err("wood_type is a name, or namespace:path, in lower case".into());
        }
    }
    if let Some(color) = &file.sign_text_color {
        if !sign {
            return Err("sign_text_color needs a vanilla sign template".into());
        }
        if !DYE_COLORS.contains(&color.as_str()) {
            return Err("sign_text_color is a DyeColor name, such as white or light_blue".into());
        }
    }
    if file.sign_text_glow.is_some() && !sign {
        return Err("sign_text_glow needs a vanilla sign template".into());
    }
    for (key, sound) in [
        ("open_sound", &file.open_sound),
        ("close_sound", &file.close_sound),
    ] {
        let Some(sound) = sound else { continue };
        if !file.template.ends_with("_fence_gate") {
            return Err(format!(
                "{key} needs a fence gate template (minecraft:*_fence_gate), not {}",
                file.template
            ));
        }
        let sites = match sound {
            GateSound::All(sound) => [("", Some(sound)), ("", None), ("", None)],
            GateSound::Sites {
                on_use,
                on_explosion,
                on_redstone,
            } => [
                (".use", on_use.as_ref()),
                (".explosion", on_explosion.as_ref()),
                (".redstone", on_redstone.as_ref()),
            ],
        };
        for (site, sound) in sites {
            if let Some(sound) = sound
                && !namespaced_id(sound)
            {
                return Err(format!(
                    "{key}{site} {sound:?} is a sound event id, namespace:path"
                ));
            }
        }
    }
    if let Some(item) = &file.block_item {
        if item == "none" {
            return Err("block_item is an item id; a block with no item is \"item\": false".into());
        }
        if file.item {
            return Err(
                "block_item is the item this block yields instead of its own, and item is true"
                    .into(),
            );
        }
        if !namespaced_id(item) {
            return Err("block_item is an item id, namespace:path".into());
        }
    }
    if let Some(item) = &file.pick_item
        && !namespaced_id(item)
    {
        return Err("pick_item is an item id, namespace:path".into());
    }
    if let Some(wall) = &file.wall_block {
        if !sign || file.template.contains("_wall_") {
            return Err("wall_block needs a standing or ceiling sign template".into());
        }
        if !file.item {
            return Err("wall_block is where the item places the block, and item is false".into());
        }
        if !namespaced_id(wall) {
            return Err("wall_block is a block id, namespace:path".into());
        }
    }
    Ok(())
}

/// A sign's `wall_block` is one of the mod's own blocks: the wall form of its
/// kind, with no item of its own and no other sign. Checked here, a pair that
/// doesn't fit leaves out its mod alone.
fn sign_wall_refusals<'a>(blocks: &'a [&'a Block]) -> impl Iterator<Item = (&'a str, String)> + 'a {
    let mut paired = BTreeMap::new();
    blocks.iter().filter_map(move |sign| {
        let wall = sign.wall_block.as_ref()?;
        let id = sign.id.as_str();
        let form = if sign.template.ends_with("_hanging_sign") {
            "_wall_hanging_sign"
        } else {
            "_wall_sign"
        };
        let Some(block) = blocks.iter().find(|b| b.id == *wall) else {
            return Some((
                id,
                format!("wall_block {wall} is not one of this mod's blocks"),
            ));
        };
        if !block.template.ends_with(form) {
            return Some((
                id,
                format!("wall_block {wall} needs a template ending in {form}"),
            ));
        }
        if block.block_item.is_some() {
            return Some((
                id,
                format!("wall_block {wall} has this sign's item, so it declares no block_item"),
            ));
        }
        if block.pick_item.is_some() {
            return Some((
                id,
                format!("wall_block {wall} picks this sign's item, so it declares no pick_item"),
            ));
        }
        if block.item {
            return Some((
                id,
                format!("wall_block {wall} has this sign's item, so its own \"item\" is false"),
            ));
        }
        fn text(b: &Block) -> (&str, bool) {
            (
                b.sign_text_color.as_deref().unwrap_or("black"),
                b.sign_text_glow.unwrap_or(false),
            )
        }
        if text(sign) != text(block) {
            return Some((
                id,
                format!("wall_block {wall} declares a different sign_text_color or sign_text_glow"),
            ));
        }
        match paired.entry(wall) {
            std::collections::btree_map::Entry::Occupied(other) => Some((
                id,
                format!("wall_block {wall} is already paired with {}", other.get()),
            )),
            std::collections::btree_map::Entry::Vacant(first) => {
                first.insert(id);
                None
            }
        }
    })
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "content_context_shape_tests.rs"]
mod context_shape_tests;

#[cfg(test)]
#[path = "fluid_wire_tests.rs"]
mod fluid_wire_tests;
