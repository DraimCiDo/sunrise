/// A block the mod adds: a vanilla template's behaviour, states and shapes
/// under a new id, with `properties` applied. A mod's code declares one as a
/// `mod_api::Block` (`native_blocks`), whose `item` is true as a file's is.
/// Here `..Default::default()` leaves the rest as a file leaves it out, except
/// `item`, false: the engine's own blocks, such as a fluid's, have none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    /// `<mod id>:<path>`.
    pub id: String,
    /// The vanilla block it copies, `minecraft:<path>`.
    pub template: String,
    pub properties: BlockProperties,
    pub shapes: Option<BlockShapes>,
    pub collision_shapes: Option<BlockShapes>,
    /// Independent overrides for actual entity queries; omitted kinds use the defaults.
    pub outline_context_shapes: BTreeMap<ShapeEntityKind, BlockShapes>,
    pub collision_context_shapes: BTreeMap<ShapeEntityKind, BlockShapes>,
    /// Ray-face override; does not replace the outline hit point or create a hit.
    pub interaction_shapes: Option<BlockShapes>,
    /// The block's block entity publishes its outline and collision shapes, which replace the
    /// declared ones until cleared. Needs a block entity type that owns the block.
    pub shapes_from_block_entity: bool,
    /// A vanilla block's context collision it takes, over its shapes for entities that have
    /// no `collision_context_shapes` of their own.
    pub context_collision: Option<ContextCollision>,
    /// Per-state emission; takes precedence over `properties.light`.
    pub state_light: Option<StateLight>,
    /// State properties of its own, which its template does not have.
    pub state_properties: BTreeMap<String, StateProperty>,
    /// State properties overlaid on the template default.
    pub default_state: BTreeMap<String, String>,
    /// Also add a block item with the same id.
    pub item: bool,
    /// A fence gate's opening sounds (`WoodType.fenceGateOpen`). `All` replaces
    /// every site; `Sites` keeps the template wood's sound for each missing site.
    /// Needs a `*_fence_gate` template.
    pub open_sound: Option<GateSound>,
    /// A fence gate's closing sounds (`WoodType.fenceGateClose`), with the same
    /// per-site fallback as `open_sound`.
    pub close_sound: Option<GateSound>,
    /// The item this block is, for pick-block and `Block.asItem`, when
    /// it is not a block item of its own: an item of this mod or a vanilla
    /// one, checked to exist when the content loads. Needs `item` false.
    pub block_item: Option<String>,
    /// The item pick-block takes from this block (`getCloneItemStack`), when it is not
    /// the block's item: a mature crop picks its seed. An item of this mod or a vanilla
    /// one, checked to exist when the content loads. Absent is the block's item.
    pub pick_item: Option<String>,
    /// Its item's component patch, as an [`Item`]'s.
    pub components: BTreeMap<String, serde_json::Value>,
    /// Ordered effects supplied by this block, independent of its item's components.
    pub suspicious_stew_effects: Option<Vec<StewEffect>>,
    /// A connecting block (template `minecraft:chorus_plant`, a `PipeBlock`
    /// without the chorus plant's rules): the block tag, without `#`, whose
    /// blocks it joins besides itself (a cable's machines).
    pub connects_to: Option<String>,
    /// A connecting block's width in pixels (2 to 14): its shapes, the
    /// chorus plant's (10) when unset.
    pub thickness: Option<f32>,
    /// A sign's `WoodType` (template a vanilla sign), which names its editor's
    /// background: `<namespace>:<path>` a mod's texture, a bare name vanilla's.
    /// The template's when unset.
    pub wood_type: Option<String>,
    /// The block a standing or ceiling sign's item places on a wall
    /// (`StandingAndWallBlockItem.wallBlock`): one of this mod's blocks, whose
    /// template is the matching wall sign and which has no item. The
    /// template's when unset.
    pub wall_block: Option<String>,
    /// A sign's new text colour (`SignBlockEntity.createDefaultSignText`): a `DyeColor` name,
    /// black when unset. Applies to a sign when it is placed, front and back.
    pub sign_text_color: Option<String>,
    /// Whether a new sign's text glows, false when unset.
    pub sign_text_glow: Option<bool>,
    /// The block farmland turns into; the template's base when unset.
    pub base_block: Option<String>,
}

/// An item the mod adds: a vanilla template item's components under a new
/// id, with these changed. A native mod may build one in code
/// (`native_items`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    /// The vanilla item it copies, `minecraft:<path>`.
    pub template: String,
    pub max_stack_size: Option<u8>,
    pub rarity: Option<Rarity>,
    /// A 26.3 data component patch over the template's: `!minecraft:<type>` removes one.
    pub components: BTreeMap<String, serde_json::Value>,
    /// What it leaves in the crafting grid (`Item.Properties.craftRemainder`):
    /// a vanilla item or one of the mod's own.
    pub crafting_remainder: Option<String>,
    /// The block placed by this item, if any.
    pub block: Option<String>,
    /// The arm pose a humanoid takes holding it, in place of vanilla's (`ArmPose`).
    pub arm_pose: Option<ArmPose>,
}

/// How a fluid the mod names looks on the client (`data/<id>/sunrise/fluid/`), as
/// NeoForge's `IClientFluidTypeExtensions`: `getStillTexture` and `getTintColor`.
/// The fluid needs no block, so it can be a fluid only a tank holds.
#[derive(Clone, Debug, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
#[wire(crate = ::mod_vocab::wire)]
pub struct FluidLook {
    /// `<mod id>:<path>`.
    pub id: String,
    /// An atlas sprite id (`namespace:path`); none draws a flat square of the tint.
    pub sprite: Option<String>,
    /// `0xAARRGGBB`.
    pub tint: Option<u32>,
    /// How it exists in the world; none for a fluid only a tank holds.
    pub world: Option<WorldFluid>,
}

impl FluidLook {
    /// The registry name of its flowing form: `<namespace>:flowing_<path>`.
    pub fn flowing_id(&self) -> String {
        match self.id.split_once(':') {
            Some((namespace, path)) => format!("{namespace}:flowing_{path}"),
            None => format!("flowing_{}", self.id),
        }
    }
    /// The filled bucket a world fluid has.
    pub fn bucket_id(&self) -> Option<String> {
        self.world
            .filter(|world| world.bucket)
            .map(|_| format!("{}_bucket", self.id))
    }
    /// The block a world fluid is in the world: water's behaviour and states under its own id.
    pub fn world_block(&self) -> Option<Block> {
        self.world.map(|_| Block {
            id: self.id.clone(),
            template: "minecraft:water".into(),
            ..Block::default()
        })
    }
    /// The item its bucket is: a water bucket's under its own id, leaving the empty bucket.
    pub fn world_bucket(&self) -> Option<Item> {
        self.bucket_id().map(|id| Item {
            id,
            template: "minecraft:water_bucket".into(),
            crafting_remainder: Some("minecraft:bucket".into()),
            ..Item::default()
        })
    }
}

/// What a world fluid draws with beyond its [`FluidLook`], as NeoForge's fluid models: registered
/// by a client mod in `register_fluid_models`. An unset sprite keeps the look's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FluidModel {
    /// The world fluid's id, `<mod id>:<path>`.
    pub fluid: String,
    /// The block atlas sprite on a surface at rest; the look's `sprite` when unset.
    pub still: Option<String>,
    /// The sprite on a surface that flows, and on its sides; `still` when unset.
    pub flowing: Option<String>,
    /// The sprite on sides against glass and leaves, as water's overlay; `flowing` when unset.
    pub overlay: Option<String>,
}

/// A creative inventory tab the mod adds (`data/<id>/sunrise/creative_tab/`):
/// its title, the item on its button, and its items in order. Items no mod
/// or vanilla registers are left out when the game loads.
#[derive(Clone, Debug, PartialEq)]
pub struct CreativeTab {
    /// `<mod id>:<path>`.
    pub id: String,
    pub title: String,
    pub icon: String,
    pub items: Vec<String>,
}

/// A block entity type declared in code, with the blocks it supports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockEntityType {
    pub id: String,
    pub blocks: Vec<String>,
}

impl RandomTicks {
    /// Whether a state ticks, `combo` being its values of the block's own state properties.
    pub fn applies(&self, combo: &[(String, String)]) -> bool {
        match self {
            RandomTicks::All(all) => *all,
            RandomTicks::States { property, values } => combo
                .iter()
                .any(|(name, value)| name == property && values.contains(value)),
        }
    }
}

/// A declaration checked without consulting other entries or loaded resources.
#[derive(Clone)]
pub struct LocalDeclaration<T>(T);

impl<T> LocalDeclaration<T> {
    pub fn get(&self) -> &T {
        &self.0
    }
}

impl Block {
    pub fn validate_local(self) -> Result<LocalDeclaration<Self>, String> {
        check_block(&self)?;
        Ok(LocalDeclaration(self))
    }
}

impl Item {
    pub fn validate_local(self) -> Result<LocalDeclaration<Self>, String> {
        check_item(&self)?;
        Ok(LocalDeclaration(self))
    }
}
