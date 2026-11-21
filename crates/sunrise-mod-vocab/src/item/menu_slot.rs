//! A player's open menu's slots as item places (arismoko/horizon#41 §7), a fork addition beside
//! NeoForge 26.3's `ItemAccess.forPlayerSlot`. A slot is the storage it shows, so a write through
//! [`ItemPlace::MenuSlot`] is a write to that storage, journaled once, through its own handler,
//! under the slot's own rules.
use crate::{
    ClientHost, EntityRef, Host, ItemPlace, ItemStack, LevelReadError, LevelRef, MenuRef,
    MenuSlotPlace, MerchantOffer, backend_of,
};

/// Read-only access to one resolved menu (`PlayerContainerEvent.getContainer`).
/// Use [`MenuRef::with_view`] to read it while it is lent to your callback.
// no wire: a borrowed view of a live menu, not a transported value.
pub trait MenuView {
    /// The server's container id (0 for inventory). `None` on the client.
    fn container_id(&self) -> Option<i32>;

    /// The server's registry menu type, borrowed for this read. `Some(None)` is the
    /// inventory menu; `None` means missing registry metadata or a client view.
    fn kind(&self) -> Option<Option<&str>>;

    /// Number of slots in `AbstractContainerMenu.slots`. `None` for an unknown client layout.
    fn slot_count(&self) -> Option<usize>;

    /// The stack at vanilla's `Slot.index`, in menu order, as an owned API value.
    /// `Some(None)` is an empty slot; `None` is an unavailable or out-of-range slot,
    /// or a stack that could not be projected. [`Self::slot_backing`] gives the server's
    /// index in the backing container instead.
    fn slot(&self, slot: usize) -> Option<Option<ItemStack>>;

    /// Carried (cursor) stack as an owned API value, even if the layout is unknown.
    /// `Some(None)` is an empty cursor; `None` means the stack could not be projected.
    fn carried(&self) -> Option<Option<ItemStack>>;

    /// The storage a slot shows (`Slot.container` and `getContainerSlot`): the player's
    /// [`ENTITY_ITEMS`](crate::ENTITY_ITEMS) index, a block entity or entity slot, the
    /// ender chest, a crafting grid or a mod's menu. `None` for an unavailable or
    /// out-of-range slot, or on the client.
    fn slot_backing(&self, slot: usize) -> Option<MenuSlotBacking>;

    /// How many of `stack` this server slot holds (`Slot.mayPlace`, then
    /// `Slot.getMaxStackSize`): a furnace's fuel slot one bucket, an armour slot one
    /// piece that fits it, a crafting result zero. `None` when the rules or stack are
    /// unavailable, the slot is out of range, or this is a client view.
    fn slot_holds(&self, slot: usize, stack: &ItemStack) -> Option<u32>;

    /// Whether this server slot gives up what it holds (`Slot.mayPickup`); for example,
    /// bound armour stays on outside creative. `None` when the rules or slot are
    /// unavailable, or on the client.
    fn slot_gives(&self, slot: usize) -> Option<bool>;
}

/// A resolved client menu, including queries about its layout and synchronized offers.
/// Use [`MenuRef::with_client_view`] to read it while it is lent to your callback.
// no wire: a borrowed view of a live menu, not a transported value.
pub trait ClientMenuView: MenuView {
    /// The open inventory screen's slot boxes in menu order, as `[x, y, width, height]`
    /// in GUI pixels (`AbstractContainerScreen.leftPos`/`topPos` plus each `Slot.x`/`Slot.y`,
    /// 16x16). A `None` entry is a hidden slot. The whole answer is `None` when geometry
    /// is unavailable, including on another screen or with the recipe book covering the
    /// inventory. The boxes follow the GUI scale and recipe book offset.
    fn slot_boxes(&self) -> Option<Vec<Option<[f32; 4]>>>;

    /// The client layout's slot roles in menu order, with the length of [`MenuView::slot_count`]
    /// when known. Machine inputs (including fuel and brewing bottle slots) are `Input`;
    /// unclassified roles are `Other`. In creative mode this describes the underlying
    /// inventory, not the tab wrappers. `None` without an inventory screen or known layout.
    fn slot_kinds(&self) -> Option<Vec<MenuSlotKind>>;

    /// A merchant's synchronized offers, copied in display order (`MerchantMenu.getOffers`).
    /// Includes both payments, the result with its components, and stock and price data.
    /// Use [`MerchantOffer::cost_a_count`] for the adjusted first payment. No trade is
    /// selected or taken and no packet is sent.
    ///
    /// A non-merchant menu returns `Ok(None)`; an open merchant with no offers returns
    /// `Ok(Some(Vec::new()))`. An unavailable merchant or an offer that cannot be projected
    /// returns [`LevelReadError::Unavailable`].
    fn merchant_offers(&self) -> Result<Option<Vec<MerchantOffer>>, LevelReadError>;
}

/// What a menu slot shows: where [`MenuView::slot_backing`] says its stack lives.
/// This is an owned description, not a live slot or a write permission. Resolve again after
/// mutation or reentry before relying on the backing still belonging to the same menu slot.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
pub enum MenuSlotBacking {
    /// Index `.0` of the menu's player's [`ENTITY_ITEMS`](crate::ENTITY_ITEMS): the inventory
    /// 0..36, the armour 36..40 from feet to head, the off hand 40.
    Player(usize),
    /// Slot `slot` of the block entity at `pos`, through its [`ITEMS`](crate::ITEMS) handler.
    BlockEntity {
        /// The level the block entity is in.
        level: LevelRef,
        /// The block entity's position.
        pos: [i32; 3],
        /// The slot's index in the handler.
        slot: usize,
    },
    /// Slot `slot` of `entity`'s [`ENTITY_ITEMS`](crate::ENTITY_ITEMS): a chest minecart's, a
    /// chest boat's or a chested mount's.
    Entity {
        /// The entity holding the inventory.
        entity: EntityRef,
        /// The slot's index in the handler.
        slot: usize,
    },
    /// Slot `.0` of the player's ender chest.
    Ender(usize),
    /// Slot `.0` of the player's inventory crafting grid, 0..4.
    CraftGrid(usize),
    /// The inventory crafting grid's result, which nothing writes: a craft is a click.
    CraftResult,
    /// Slot `.0` of a mod's chest menu (`open_menu`).
    Native(usize),
    /// A container slot of a mod menu type opened with no block, backed by storage that is not a
    /// block entity ([`PlayerRef::back_mod_menu_slots`](crate::PlayerRef::back_mod_menu_slots)):
    /// a held item's contents or an entity's item handler. Only such a slot has this backing.
    Place(MenuSlotPlace),
}

// no wire: An in-process refusal from the native menu setter, not a transported payload.
/// Why [`MenuRef::set_data_slot`] left the menu unchanged.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuDataRefusal {
    /// This host or menu does not support blockless mod menu data.
    Unsupported,
    /// The player or the menu generation is no longer current.
    Stale,
    /// The caller does not own the menu type.
    WrongOwner,
    /// The index is outside the menu type's declared data slots.
    InvalidIndex,
}

impl crate::NoBackend for MenuDataRefusal {
    fn no_backend() -> Self {
        Self::Unsupported
    }
}

impl crate::AccessRefusal for MenuDataRefusal {
    fn unavailable() -> Self {
        Self::Unsupported
    }
}

impl MenuRef {
    /// Resolve this menu once for related reads. Calls `read` once if it is current and the
    /// host can supply it, or returns `None` without calling it. A resolved view can still
    /// report an unavailable field or an empty slot. No residency guard or lock is held during
    /// the callback. Return owned results, such as a copied kind name, instead of borrowing
    /// from the view. Resolve again after mutation or reentry.
    pub fn with_view<R>(self, host: &dyn Host, read: impl FnOnce(&dyn MenuView) -> R) -> Option<R> {
        let backend = backend_of(host)?;
        let mut read = Some(read);
        let mut result = None;
        backend.with_menu_view(self, &mut |view| {
            if let Some(read) = read.take() {
                result = Some(read(view));
            }
        });
        result
    }

    /// Resolve this client's current menu once, including when its screen is not visible.
    /// Calls `read` once if the menu resolves, returns `Ok(None)` for a stale or foreign
    /// handle, and returns [`LevelReadError::Unavailable`] when the phase or host cannot
    /// supply the read. A resolved view may still report unavailable geometry or its own
    /// merchant-read error. Like [`Self::with_view`], it only lends a view during `read`.
    pub fn with_client_view<R>(
        self,
        host: &dyn ClientHost,
        read: impl FnOnce(&dyn ClientMenuView) -> R,
    ) -> Result<Option<R>, LevelReadError> {
        let backend = backend_of(host).ok_or(LevelReadError::Unavailable)?;
        let mut read = Some(read);
        let mut result = None;
        backend.with_client_menu_view(self, &mut |view| {
            if let Some(read) = read.take() {
                result = Some(read(view));
            }
        })?;
        Ok(result)
    }

    /// The stack on this menu's cursor, as a place (`ItemAccess.forPlayerCursor`): nothing once
    /// the menu is gone.
    pub fn cursor(self) -> ItemPlace {
        ItemPlace::PlayerCursor { menu: self }
    }
}

/// A slot's role in the client's layout, not its server backing or acceptance rules.
/// Player indices are hotbar 0..=8, main 9..=35, armour 36..=39 (feet to head),
/// and off hand 40. Equipment belongs to the open container, never the player.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
pub enum MenuSlotKind {
    /// The player inventory index, independent of menu order.
    Player(u32),
    /// General storage, including linked storage not marked as an input.
    Storage,
    /// A crafting input grid.
    CraftingGrid,
    /// An output slot.
    Result,
    /// Equipment of the open container, such as a horse saddle.
    Equipment,
    /// A role the client layout cannot classify.
    #[default]
    Other,
    /// A machine input in the client layout, including fuel. This does not
    /// imply any particular acceptance or transfer rules.
    Input,
}
