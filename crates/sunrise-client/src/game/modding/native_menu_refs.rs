//! Handles to supported menus: shared reads, server storage and client clicks.
use super::api_values::{TryIntoApi, stack_value};
use super::{
    Game, Screen,
    lan::LanHost,
    lan_crafting::{load_player, machine_menu_kind},
    native_menu_slot_kinds::layout_kinds,
};
use mod_api::__private::game::*;
use mod_api::{
    ClientMenuView, DragKind, EntityRef, LevelReadError, LevelRef, MenuClick, MenuRef,
    MenuSlotBacking, MenuSlotKind, MenuSlotPlace, MenuView, MerchantOffer, MouseButton, PlayerRef,
    SwapWith, WorldEpoch,
};
use std::cell::OnceCell;
use sunrise_client::{
    containers::{BlockEntity, ModMenuType, mod_menu_type},
    items::ItemStack,
    lan::{
        guest::Guest,
        menu::{Menu as OpenMenu, SlotBacking, Slots, VEHICLE_KEY_Y},
    },
    menu::{Input, Kind, Menu},
    world::World,
};

/// A current server window. The guest owns the menu, the crafting grid and native slots;
/// the acting guest's inventory, equipment, cursor and ender chest are in `Game`.
// no wire: the read scope borrows live server menu storage.
pub(super) struct ServerMenuRead<'read, 'game> {
    game: &'read Game<'game>,
    host: &'read LanHost,
    guest: &'read Guest,
    epoch: WorldEpoch,
    window: i32,
    open: Option<&'read OpenMenu>,
    acting: bool,
    registry_name: OnceCell<Option<&'read str>>,
    backing: OnceCell<Option<OpenBacking<'read>>>,
    rules: OnceCell<Menu>,
}

// no wire: the menu scope's backing borrows a world and its slot layout.
struct OpenBacking<'read> {
    level: LevelRef,
    world: &'read World,
    places: Option<&'read [MenuSlotPlace]>,
}

/// A current client window, independent of whether the player can see its screen.
// no wire: the read scope borrows live client menu state.
struct ClientMenuRead<'read, 'game> {
    game: &'read Game<'game>,
    name: &'read str,
    window: i32,
    has_level: bool,
    // The built screen determines geometry and count; indexed prediction and roles
    // can also read the content declaration before a screen is built.
    window_type: OnceCell<Option<&'static ModMenuType>>,
    declared_type: OnceCell<Option<&'static ModMenuType>>,
    slot_count: OnceCell<Option<usize>>,
}

/// A slot in the game's own menu storage, not a handler's display mirror.
// no wire: a short-lived target for an internal journaled write.
pub(super) struct MenuStorageSlot {
    conn: u32,
    target: MenuStorageTarget,
}

// no wire: internal indices in the game or guest's menu storage.
enum MenuStorageTarget {
    ActingEnder(usize),
    GuestEnder(usize),
    CraftGrid(usize),
    Native(usize),
}

impl<'game> Game<'game> {
    /// Submit clicks on the local player's live menu through normal prediction
    /// and `ContainerClick`. Slots use the same menu order as `slot`; `None`
    /// is outside (-999). All clicks are validated before any prediction or send.
    /// A drag expands to one complete quick-craft gesture.
    /// Refuses stale menus, invalid slots/hotbar indices, spectators, non-client hosts,
    /// the creative inventory screen and empty batches without changing items.
    /// True means submitted, not accepted by the server: its corrections win.
    pub(super) fn menu_click(&mut self, menu: MenuRef, clicks: &[MenuClick]) -> bool {
        if self.is_logical_server() || self.screen == super::Screen::Creative || self.spectator() {
            return false;
        }
        let Some(view) = self.resolve_client_menu(menu) else {
            return false;
        };
        let Some(ops) = client_click_ops(view.name, clicks, self.instabuild()) else {
            return false;
        };
        self.remote_menu_ops(&ops)
    }

    /// Close the local player's open container screen as Escape does
    /// (`AbstractContainerScreen.onClose`, `LocalPlayer.closeContainer`): the
    /// server is told (`ContainerClose`) and the screen goes. Refuses stale
    /// menus, non-client hosts and a menu whose screen is not open, such as
    /// the inventory menu while playing.
    pub(super) fn menu_close(&mut self, menu: MenuRef) -> bool {
        if self.is_logical_server()
            || self.screen != super::Screen::Inventory
            || self.resolve_client_menu(menu).is_none()
        {
            return false;
        }
        self.close_container();
        self.set_screen(super::Screen::Playing);
        self.capture(true);
        true
    }

    /// `ServerPlayer.closeContainer` of a guest's menu: every menu they have open closes through
    /// the path a disconnect and a `stillValid` failure use, then their client is told. Refuses
    /// clients, stale menus, a player swapped in for a turn, and the inventory menu (window 0),
    /// which is never closed.
    pub(super) fn menu_close_container(&mut self, menu: MenuRef) -> bool {
        if menu.fields().window == 0 {
            return false;
        }
        let Some(view) = self.resolve_server_menu(menu) else {
            return false;
        };
        if view.acting {
            return false;
        }
        let conn = view.guest.conn;
        self.guest_close_menus_and_tell(conn);
        true
    }

    /// `DataSlot.set` of an owning mod's blockless menu, journaled by window generation.
    pub(super) fn menu_set_data_slot(
        &mut self,
        menu: MenuRef,
        index: usize,
        value: i32,
    ) -> Result<bool, mod_api::MenuDataRefusal> {
        use mod_api::MenuDataRefusal::*;
        let view = self.resolve_server_menu(menu).ok_or_else(|| {
            if self.is_logical_server() {
                Stale
            } else {
                Unsupported
            }
        })?;
        let open = view
            .open
            .filter(|m| m.slots == Slots::Native)
            .ok_or(Unsupported)?;
        let kind = open.mod_type.ok_or(Unsupported)?;
        if mod_api::content::crash::current_native_mod() != Some(kind.owner) {
            return Err(WrongOwner);
        }
        let &committed = open.blockless_data.get(index).ok_or(InvalidIndex)?;
        let (conn, index) = (view.guest.conn, index as u16);
        let key = (conn, view.guest.menu_generation, index);
        if self.pending_menu_data(key).unwrap_or(committed) == value {
            return Ok(false);
        }
        if !self.journal_menu_data(key, value) {
            self.lan.as_mut().unwrap().hub.update_guest(conn, |guest| {
                guest.set_menu_data(index, value);
            });
        }
        Ok(true)
    }

    /// Press button `button` of the local player's open menu as a click on
    /// one of its buttons does (`MultiPlayerGameMode.handleInventoryButtonClick`,
    /// `ContainerButtonClick`): the server's menu decides what it does
    /// (`clickMenuButton`, and `menu_button` for a mod's menu). For a client
    /// mod's own input on a menu's screen where `screen_click` doesn't come,
    /// such as a key or the wheel; no sound, that is the screen's. Refuses
    /// stale menus, non-client hosts and the inventory menu, which no server
    /// opened. True means sent, not that the menu took it.
    pub(super) fn menu_click_button(&mut self, menu: MenuRef, button: i32) -> bool {
        !self.is_logical_server()
            && self.resolve_client_menu(menu).is_some()
            && self.remote_button_click(button)
    }

    pub(super) fn with_menu_view(&self, menu: MenuRef, read: &mut dyn FnMut(&dyn MenuView)) {
        if self.is_logical_server() {
            if let Some(view) = self.resolve_server_menu(menu) {
                read(&view);
            }
        } else if let Some(view) = self.resolve_client_menu(menu) {
            read(&view);
        }
    }

    pub(super) fn with_client_menu_view(
        &self,
        menu: MenuRef,
        read: &mut dyn FnMut(&dyn ClientMenuView),
    ) -> Result<(), LevelReadError> {
        if let Some(view) = self.resolve_client_menu(menu) {
            read(&view);
        }
        Ok(())
    }

    fn resolve_client_menu(&self, menu: MenuRef) -> Option<ClientMenuRead<'_, 'game>> {
        let remote = self.remote.as_ref().filter(|remote| remote.logged_in)?;
        let epoch = self.native_lifecycle.epoch()?;
        let fields = menu.fields();
        let player = fields.player.fields();
        let (window, name) = remote
            .menu
            .as_ref()
            .map_or((0, "inventory"), |entry| (entry.0, entry.1.as_str()));
        if player.entity.epoch() != epoch
            || player.entity.id() != u32::try_from(remote.entity_id).ok()?
            || player.uuid != remote.profile
            || fields.generation != remote.menu.generation
            || fields.window != window
        {
            return None;
        }
        Some(ClientMenuRead {
            game: self,
            name,
            window,
            has_level: remote.has_level(),
            window_type: OnceCell::new(),
            declared_type: OnceCell::new(),
            slot_count: OnceCell::new(),
        })
    }

    /// The local player in a logged-in client world.
    pub(in crate::game) fn local_player(&self) -> Option<PlayerRef> {
        let remote = self.remote.as_ref().filter(|remote| remote.logged_in)?;
        self.player(remote.profile)
    }

    /// Resolve the menu's owner through the shared player boundary. The menu metadata,
    /// crafting grid and native slots stay on the guest during a turn; inventory,
    /// equipment, cursor and ender contents are swapped into Game.
    pub(super) fn native_menu_player(&self, player: PlayerRef) -> Option<&Guest> {
        match self.player_handle_read(player)? {
            super::native_server::PlayerRead::Guest(guest) => Some(guest),
            super::native_server::PlayerRead::Acting { .. } => {
                self.lan.as_ref()?.hub.guest(self.lan_acting?)
            }
        }
    }

    /// Shared owner and current-window checks for opening a read scope or issuing a handle.
    fn current_server_menu(&self, player: PlayerRef) -> Option<(&LanHost, &Guest, i32)> {
        let guest = self.native_menu_player(player)?;
        let host = self.lan.as_ref()?;
        (host.hub.holder_guest(guest.uuid, self.dim.id())?.conn == guest.conn).then_some(())?;
        if let Some(open) = &guest.menu {
            return Some((host, guest, open.window));
        }
        // Window 0 is current only if no other, unsupported menu is open.
        let play = host.play.get(&guest.conn);
        let menus = host.menus.get(&guest.conn);
        if menus.is_some_and(|m| m.crafting.table.is_some() || m.station.is_some())
            || play.is_some_and(|p| {
                p.merchant.is_some() || p.lectern.is_some() || p.mount_window.is_some()
            })
        {
            return None;
        }
        Some((host, guest, 0))
    }

    pub(super) fn resolve_server_menu(&self, menu: MenuRef) -> Option<ServerMenuRead<'_, 'game>> {
        if !self.is_logical_server() {
            return None;
        }
        let fields = menu.fields();
        let (host, guest, window) = self.current_server_menu(fields.player)?;
        if window != fields.window || guest.menu_generation != fields.generation {
            return None;
        }
        Some(ServerMenuRead {
            game: self,
            host,
            guest,
            epoch: fields.player.entity().epoch(),
            window,
            open: guest.menu.as_ref(),
            acting: self.lan_acting == Some(guest.conn),
            registry_name: OnceCell::new(),
            backing: OnceCell::new(),
            rules: OnceCell::new(),
        })
    }

    /// A player's current supported menu. Refuses crafting tables, merchants,
    /// workstations, lecterns and mounts. Hidden slots remain unavailable through
    /// the slot methods without invalidating the menu handle. Also refuses stale
    /// players and players whose state is swapped out by another active turn.
    /// The acting player's inventory, equipment, cursor and ender reads use Game's state.
    /// On a client, only the local player's supported menu is available.
    pub(super) fn player_container_menu(&self, player: PlayerRef) -> Option<MenuRef> {
        if !self.is_logical_server() {
            self.player_read_both(player)?;
            let remote = self.remote.as_ref()?;
            let (window, name) = remote
                .menu
                .as_ref()
                .map_or((0, "inventory"), |entry| (entry.0, entry.1.as_str()));
            if self.native_screen_menu_type(window, name).is_none() {
                Self::inventory_screen_slot_with_type(name, 0, || None)?;
            }
            return Some(mod_api::__private::game::menu_ref(
                player,
                window,
                remote.menu.generation,
            ));
        }
        let (_, guest, window) = self.current_server_menu(player)?;
        Some(menu_ref(player, window, guest.menu_generation))
    }

    #[cfg(test)]
    pub(super) fn native_menu_storage(
        &mut self,
        menu: MenuRef,
        slot: usize,
    ) -> Option<&mut Option<ItemStack>> {
        let (target, _) = self.resolve_server_menu(menu)?.storage(slot)?;
        self.menu_storage_mut(target)
    }

    /// Lend only the storage selected by a read just before journaling the write.
    pub(super) fn menu_storage_mut(
        &mut self,
        target: MenuStorageSlot,
    ) -> Option<&mut Option<ItemStack>> {
        if let MenuStorageTarget::ActingEnder(n) = target.target {
            return self.ender_items.get_mut(n);
        }
        let guests = &mut self.lan.as_mut()?.hub.guests;
        let index = guests.connection_index(target.conn)?;
        match target.target {
            MenuStorageTarget::ActingEnder(_) => None,
            MenuStorageTarget::GuestEnder(n) => guests.ender_items_mut(index)?.get_mut(n),
            MenuStorageTarget::CraftGrid(n) => guests.craft_grid_mut(index)?.get_mut(n),
            MenuStorageTarget::Native(n) => guests.native_menu_mut(index)?.get_mut(n),
        }
    }

    /// Write back an already-held engine stack after in-place item processing, without
    /// routing it through insertion rules or changing its count. Refuses virtual previews
    /// and handler-owned slots: their display mirrors do not own the item.
    pub(super) fn replace_crafted_menu_slot(
        &mut self,
        menu: MenuRef,
        slot: usize,
        stack: ItemStack,
    ) -> bool {
        let player = menu.fields().player;
        let Some(view) = self.resolve_server_menu(menu) else {
            return false;
        };
        let Some(backing) = view.slot_backing(slot) else {
            return false;
        };
        match backing {
            MenuSlotBacking::Player(index) => self.native_as_player_handle(player, |game| {
                game.journal_player(player.uuid());
                let held = match index {
                    0..36 => &mut game.inventory[index],
                    36..40 => &mut game.armor[39 - index],
                    _ => &mut game.offhand,
                };
                *held = Some(stack);
            }),
            MenuSlotBacking::BlockEntity { .. } | MenuSlotBacking::Entity { .. } => {
                // The transaction journals block and vehicle slots in the level in place.
                let Some((pos, index)) = view
                    .open
                    .and_then(|open| open.slots.held_block_slot(&self.world, view.guest, slot))
                else {
                    return false;
                };
                if pos.1 == VEHICLE_KEY_Y {
                    let entity = pos.2 as u32;
                    self.journal_entity(entity);
                    return self
                        .edit_entity_slots(entity, |slots, _| {
                            let Some(held) = slots.get_mut(index) else {
                                return false;
                            };
                            *held = Some(stack);
                            true
                        })
                        .unwrap_or(false);
                }
                self.journal_block_entity_slot(pos, index);
                let Some(held) = self
                    .world
                    .block_entities
                    .get_mut(&pos)
                    .and_then(|block| block.slot_mut(index))
                else {
                    return false;
                };
                *held = Some(stack);
                self.after_commit(super::transactions::After::Changed(pos));
                true
            }
            _ => {
                let Some((target, _)) = view.storage_at(backing) else {
                    return false;
                };
                self.journal_menus(player);
                let Some(held) = self.menu_storage_mut(target) else {
                    return false;
                };
                *held = Some(stack);
                true
            }
        }
    }
}

impl MenuView for ServerMenuRead<'_, '_> {
    fn container_id(&self) -> Option<i32> {
        Some(self.window)
    }

    fn kind(&self) -> Option<Option<&str>> {
        match self.open {
            Some(open) => Some(Some(self.menu_registry_name(open)?)),
            None => Some(None),
        }
    }

    fn slot_count(&self) -> Option<usize> {
        Some(self.open.map_or(46, OpenMenu::slot_count))
    }

    fn slot(&self, slot: usize) -> Option<Option<mod_api::ItemStack>> {
        self.shown_stack(slot)?.try_into_api().ok()
    }

    fn carried(&self) -> Option<Option<mod_api::ItemStack>> {
        self.carried_stack().try_into_api().ok()
    }

    fn slot_backing(&self, slot: usize) -> Option<MenuSlotBacking> {
        ServerMenuRead::slot_backing(self, slot)
    }

    fn slot_holds(&self, slot: usize, stack: &mod_api::ItemStack) -> Option<u32> {
        let stack = stack_value(stack).ok()?;
        let rules = self.native_menu_rules(slot)?;
        Some(rules.slot_holds(&self.game.menu_env(), slot, &stack))
    }

    fn slot_gives(&self, slot: usize) -> Option<bool> {
        let rules = self.native_menu_rules(slot)?;
        Some(rules.slot_gives(&self.game.menu_env(), slot))
    }
}

impl<'read> ServerMenuRead<'read, '_> {
    fn menu_registry_name(&self, open: &OpenMenu) -> Option<&'read str> {
        *self
            .registry_name
            .get_or_init(|| self.host.hub.registry_name("minecraft:menu", open.kind))
    }

    // Metadata and cursor reads don't require live backing. Check it once, if a slot needs it.
    fn open_backing(&self) -> Option<&OpenBacking<'read>> {
        self.backing
            .get_or_init(|| {
                let game = self.game;
                let open = self.open?;
                let (dim, world) = if self.guest.dimension == game.dim.id() {
                    (game.dim, &game.world)
                } else {
                    let (dim, level) = game.levels.parked_entry_of(&self.guest.dimension)?;
                    (dim, &level.world)
                };
                // A block menu no longer showing its original number of slots backs none of
                // them, the player's included, until the close pass ends it (`stillValid`).
                if open
                    .slots
                    .block_slots(world)
                    .is_some_and(|n| n != open.size)
                {
                    return None;
                }
                let places = if open.slots == Slots::Native {
                    self.host.native_menu_places(self.guest.conn, self.window)
                } else {
                    None
                };
                Some(OpenBacking {
                    level: game.level_ref(self.epoch, dim.key()),
                    world,
                    places,
                })
            })
            .as_ref()
    }

    pub(super) fn slot_backing(&self, slot: usize) -> Option<MenuSlotBacking> {
        let Some(open) = self.open else {
            return match slot {
                0 => Some(MenuSlotBacking::CraftResult),
                1..=4 => Some(MenuSlotBacking::CraftGrid(slot - 1)),
                5..=8 => Some(MenuSlotBacking::Player(39 - (slot - 5))),
                9..=35 => Some(MenuSlotBacking::Player(slot)),
                36..=44 => Some(MenuSlotBacking::Player(slot - 36)),
                45 => Some(MenuSlotBacking::Player(40)),
                _ => None,
            };
        };
        if slot >= open.slot_count() || open.hidden.contains(&slot) {
            return None;
        }
        let backing = self.open_backing()?;
        if slot >= open.size {
            return match slot - open.size {
                n @ 0..=26 => Some(MenuSlotBacking::Player(n + 9)),
                n @ 27..=35 => Some(MenuSlotBacking::Player(n - 27)),
                _ => None,
            };
        }
        Some(match open.slots.backing(backing.world, slot)? {
            SlotBacking::Block(pos, n) if pos.1 == VEHICLE_KEY_Y => MenuSlotBacking::Entity {
                entity: EntityRef::new(backing.level.epoch(), pos.2 as u32),
                slot: n,
            },
            SlotBacking::Block(pos, n) => MenuSlotBacking::BlockEntity {
                level: backing.level,
                pos: [pos.0, pos.1, pos.2],
                slot: n,
            },
            SlotBacking::Ender(n) => MenuSlotBacking::Ender(n),
            // Only a slot a mod backed (`player_back_mod_menu_slots`) reads as a place.
            SlotBacking::Native(n) => match backing.places.and_then(|places| places.get(n)) {
                Some(&place) => MenuSlotBacking::Place(place),
                None => MenuSlotBacking::Native(n),
            },
        })
    }

    pub(super) fn shown_stack(&self, slot: usize) -> Option<Option<ItemStack>> {
        self.slot_stack(slot, self.slot_backing(slot)?)
    }

    /// The visible stack for `slot` and its backing from this read, not the storage that owns
    /// a mod's display mirror or a crafting preview. Stay in engine stacks for engine readers.
    pub(super) fn slot_stack(
        &self,
        slot: usize,
        backing: MenuSlotBacking,
    ) -> Option<Option<ItemStack>> {
        let game = self.game;
        let guest = self.guest;
        Some(match backing {
            MenuSlotBacking::BlockEntity { .. } | MenuSlotBacking::Entity { .. } => {
                let world = self.open_backing()?.world;
                self.open?.slots.get(world, guest, slot)
            }
            MenuSlotBacking::Player(n @ 0..36) => {
                let inventory = if self.acting {
                    &game.inventory
                } else {
                    &guest.inventory
                };
                inventory.get(n)?.clone()
            }
            MenuSlotBacking::Player(n @ 36..40) => {
                let armor = if self.acting {
                    &game.armor
                } else {
                    &guest.armor
                };
                armor[39 - n].clone()
            }
            MenuSlotBacking::Player(_) => {
                if self.acting {
                    game.offhand.clone()
                } else {
                    guest.offhand.clone()
                }
            }
            MenuSlotBacking::CraftGrid(n) => guest.craft_grid.get(n)?.clone(),
            MenuSlotBacking::CraftResult => guest.craft_result.clone(),
            MenuSlotBacking::Ender(n) => {
                let items = if self.acting {
                    &game.ender_items
                } else {
                    &guest.ender_items
                };
                items.get(n)?.clone()
            }
            MenuSlotBacking::Native(n) => guest.native_menu.get(n)?.clone(),
            // The place's mirror, refreshed after each click and each tick.
            MenuSlotBacking::Place(_) => guest.native_menu.get(slot)?.clone(),
            _ => return None,
        })
    }

    /// Make the same detached Menu used for the slot rules, only for a rule query with a
    /// backed slot. The cell is initialized before the caller can dispatch a rule.
    fn native_menu_rules(&self, slot: usize) -> Option<&Menu> {
        self.slot_backing(slot)?;
        if let Some(rules) = self.rules.get() {
            return (slot < rules.slot_count()).then_some(rules);
        }
        let open = match self.open {
            Some(open) => Some((open, self.open_backing()?.world)),
            None => None,
        };
        let (kind, disabled, registry_name) = match open {
            None => (Kind::Inventory, None, None),
            Some((open, world)) => {
                let registry_name = self.menu_registry_name(open);
                let entity = match open.slots {
                    Slots::Block(pos) => world.block_entities.get(&pos),
                    _ => None,
                };
                let kind = match entity.and_then(|e| machine_menu_kind(e, open.mod_type)) {
                    Some(kind) => kind,
                    None if registry_name == Some("minecraft:shulker_box") => Kind::Shulker,
                    None if open.size == 5 => Kind::Hopper,
                    None if open.size == 9 => Kind::Dispenser,
                    None => Kind::Chest(open.size / 9),
                };
                // `CrafterSlot.mayPlace`: a disabled slot takes nothing.
                let disabled = match entity {
                    Some(BlockEntity::Crafter(c)) => Some(c.disabled),
                    _ => None,
                };
                (kind, disabled, registry_name)
            }
        };
        let mut rules = Menu::new(kind);
        rules.set_registry_id(registry_name);
        if let Some((open, world)) = open {
            rules.set_container_limits(open.slots.stack_limits(world, open.size));
        }
        // Some open windows have a layout the vanilla rules model doesn't represent.
        if slot >= rules.slot_count() {
            return None;
        }
        let game = self.game;
        if self.acting {
            for (n, stack) in game.inventory.iter().enumerate().take(36) {
                rules.set_player_item(n, stack.clone());
            }
            for (n, stack) in game.armor.iter().enumerate() {
                rules.set_player_item(39 - n, stack.clone());
            }
            rules.set_player_item(40, game.offhand.clone());
            rules.selected = game.selected;
            rules.creative = game.instabuild();
            rules.levels = game.experience.level;
            rules.carried = game.carried.clone();
        } else {
            load_player(&mut rules, self.guest);
        }
        for (k, off) in disabled.iter().flatten().enumerate() {
            rules.data[k] = i32::from(*off);
        }
        Some(self.rules.get_or_init(|| rules))
    }

    pub(super) fn carried_stack(&self) -> &'read Option<ItemStack> {
        if self.acting {
            &self.game.carried
        } else {
            &self.guest.carried
        }
    }

    /// Owned menu storage that the journal may write directly. Backed mirrors and previews
    /// are never write targets, even though a read may show their stack.
    pub(super) fn storage(
        &self,
        slot: usize,
    ) -> Option<(MenuStorageSlot, &'read Option<ItemStack>)> {
        self.storage_at(self.slot_backing(slot)?)
    }

    fn storage_at(
        &self,
        backing: MenuSlotBacking,
    ) -> Option<(MenuStorageSlot, &'read Option<ItemStack>)> {
        let (target, held) = match backing {
            MenuSlotBacking::Ender(n) if self.acting => (
                MenuStorageTarget::ActingEnder(n),
                self.game.ender_items.get(n)?,
            ),
            MenuSlotBacking::Ender(n) => (
                MenuStorageTarget::GuestEnder(n),
                self.guest.ender_items.get(n)?,
            ),
            MenuSlotBacking::CraftGrid(n) => (
                MenuStorageTarget::CraftGrid(n),
                self.guest.craft_grid.get(n)?,
            ),
            MenuSlotBacking::Native(n) => {
                (MenuStorageTarget::Native(n), self.guest.native_menu.get(n)?)
            }
            _ => return None,
        };
        Some((
            MenuStorageSlot {
                conn: self.guest.conn,
                target,
            },
            held,
        ))
    }
}

impl ClientMenuRead<'_, '_> {
    fn window_type(&self) -> Option<&'static ModMenuType> {
        *self
            .window_type
            .get_or_init(|| self.game.native_screen_menu_type(self.window, self.name))
    }

    fn declared_type(&self) -> Option<&'static ModMenuType> {
        *self.declared_type.get_or_init(|| mod_menu_type(self.name))
    }

    fn inventory_slot_boxes(&self, batch: &super::HudBatch) -> Option<Vec<Option<[f32; 4]>>> {
        let game = self.game;
        if game.book_covers_menu(batch.width) {
            return None;
        }
        let kind = self.window_type();
        let slot_id = |slot| Game::inventory_screen_slot_with_type(self.name, slot, || kind);
        match kind {
            None => {
                slot_id(0)?;
            }
            Some(kind) if kind.slot_count() == 0 => return Some(Vec::new()),
            Some(_) => {}
        }
        let visible = game.slots();
        let (ox, oy) = game.inventory_origin(batch);
        Some(
            (0..)
                .map_while(slot_id)
                .map(|slot| {
                    visible.contains(&slot).then(|| {
                        let (x, y) = game.slot_position(slot);
                        [ox + x, oy + y, 16.0, 16.0]
                    })
                })
                .collect(),
        )
    }
}

impl MenuView for ClientMenuRead<'_, '_> {
    fn container_id(&self) -> Option<i32> {
        None
    }

    fn kind(&self) -> Option<Option<&str>> {
        None
    }

    fn slot_count(&self) -> Option<usize> {
        *self.slot_count.get_or_init(|| {
            if let Some(kind) = self.window_type() {
                return Some(kind.slot_count());
            }
            let count = (0..)
                .map_while(|slot| Game::inventory_screen_slot_with_type(self.name, slot, || None))
                .count();
            (count != 0).then_some(count)
        })
    }

    fn slot(&self, slot: usize) -> Option<Option<mod_api::ItemStack>> {
        let index =
            Game::inventory_screen_slot_with_type(self.name, i32::try_from(slot).ok()?, || {
                self.declared_type()
            })?;
        self.game.slot_view(index).try_into_api().ok()
    }

    fn carried(&self) -> Option<Option<mod_api::ItemStack>> {
        self.game.carried.try_into_api().ok()
    }

    fn slot_backing(&self, _: usize) -> Option<MenuSlotBacking> {
        None
    }

    fn slot_holds(&self, _: usize, _: &mod_api::ItemStack) -> Option<u32> {
        None
    }

    fn slot_gives(&self, _: usize) -> Option<bool> {
        None
    }
}

impl ClientMenuView for ClientMenuRead<'_, '_> {
    fn slot_boxes(&self) -> Option<Vec<Option<[f32; 4]>>> {
        let game = self.game;
        if game.screen != Screen::Inventory || !game.screen_hooks_open() {
            return None;
        }
        let renderer = game.renderer.as_ref()?;
        let batch = super::HudBatch::new(
            [renderer.config.width, renderer.config.height],
            game.settings.gui_scale_inputs(),
        );
        self.inventory_slot_boxes(&batch)
    }

    fn slot_kinds(&self) -> Option<Vec<MenuSlotKind>> {
        if !matches!(self.game.screen, Screen::Inventory | Screen::Creative) || !self.has_level {
            return None;
        }
        let content = self.declared_type();
        let count = match content {
            Some(content) => content.slot_count(),
            None => self.slot_count()?,
        };
        Some(layout_kinds(self.name, count, content, |slot| {
            Game::inventory_screen_slot_with_type(self.name, slot as i32, || content)
        }))
    }

    fn merchant_offers(&self) -> Result<Option<Vec<MerchantOffer>>, LevelReadError> {
        if self.name != "minecraft:merchant" {
            return Ok(None);
        }
        let merchant = self
            .game
            .station
            .as_ref()
            .and_then(|station| station.merchant.as_ref())
            .ok_or(LevelReadError::Unavailable)?;
        merchant
            .session
            .offers
            .try_into_api()
            .map(Some)
            .map_err(|_| LevelReadError::Unavailable)
    }
}

fn client_click_ops(
    menu: &str,
    clicks: &[MenuClick],
    creative: bool,
) -> Option<Vec<(Option<usize>, i32, Input)>> {
    if clicks.is_empty() {
        return None;
    }
    let mapped_slot =
        |slot| Game::inventory_screen_slot(menu, i32::from(i16::try_from(slot).ok()?));
    let mut ops = Vec::new();
    for click in clicks {
        let op = match click {
            MenuClick::Pickup { slot, button } => (
                match slot {
                    Some(slot) => Some(mapped_slot(*slot)?),
                    None => None,
                },
                match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                },
                Input::Pickup,
            ),
            MenuClick::QuickMove { slot } => (Some(mapped_slot(*slot)?), 0, Input::QuickMove),
            MenuClick::Swap { slot, with } => (
                Some(mapped_slot(*slot)?),
                match with {
                    SwapWith::Hotbar(index) if *index <= 8 => i32::from(*index),
                    SwapWith::Offhand => 40,
                    _ => return None,
                },
                Input::Swap,
            ),
            MenuClick::Clone { slot } if creative => (Some(mapped_slot(*slot)?), 2, Input::Clone),
            MenuClick::Throw { slot, whole_stack } => (
                Some(mapped_slot(*slot)?),
                i32::from(*whole_stack),
                Input::Throw,
            ),
            MenuClick::Drag { kind, slots } => {
                let mode = match kind {
                    DragKind::Split => 0,
                    DragKind::One => 4,
                    DragKind::Clone if creative => 8,
                    _ => return None,
                };
                ops.push((None, mode, Input::QuickCraft));
                for slot in slots {
                    ops.push((Some(mapped_slot(*slot)?), mode + 1, Input::QuickCraft));
                }
                ops.push((None, mode + 2, Input::QuickCraft));
                continue;
            }
            MenuClick::PickupAll { slot } => (Some(mapped_slot(*slot)?), 0, Input::PickupAll),
            _ => return None,
        };
        ops.push(op);
    }
    Some(ops)
}

#[cfg(test)]
mod tests {
    use super::super::{lan::LanSettings, native_entity_refs::tests::with_game};
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicUsize},
    };
    use sunrise_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use sunrise_protocol::packets::configuration::ClientInformation;

    const PANEL: &str = "test:panel";

    #[derive(Clone)]
    struct Panel {
        slots: usize,
        opening: Arc<Mutex<Option<(MenuRef, usize)>>>,
    }
    #[mod_api::sunrise_part]
    impl mod_api::MenuPart for Panel {
        fn screen(
            &self,
            _: &dyn mod_api::Host,
            _: &mut mod_api::ScreenEvent<'_>,
        ) -> Option<mod_api::Screen> {
            Some(mod_api::Screen {
                slots: vec![Some(mod_api::ScreenSlot::default()); self.slots],
                ..Default::default()
            })
        }
    }
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Panel {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            r.menu_part(PANEL, self.clone());
        }

        fn screen_opening(
            &self,
            host: &dyn mod_api::ClientHost,
            event: &mut mod_api::ScreenOpeningEvent<'_>,
        ) -> Option<mod_api::ScreenOpening> {
            if event.current.id == "playing" && event.new.menu == Some(PANEL) {
                let menu = host.local_player()?.container_menu(host)?;
                let count = menu.with_view(host, |view| view.slot_count()).flatten()?;
                *self.opening.lock().unwrap() = Some((menu, count));
            }
            None
        }
    }

    fn with_panel(
        slots: usize,
        test: impl FnOnce(&mut Game<'_>, &Mutex<Option<(MenuRef, usize)>>),
    ) {
        let opening = Arc::new(Mutex::new(None));
        let panel = Panel {
            slots,
            opening: opening.clone(),
        };
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new("test", Box::new(panel))].into();
        mod_platform::testing::with_mods(mods.clone(), || {
            mod_platform::__private::part_tests::install_parts_fixture(&mods).unwrap();
            sunrise_client::containers::with_mod_menu_types(
                [(
                    PANEL.into(),
                    "test".into(),
                    slots,
                    0,
                    vec![],
                    vec![],
                    None,
                    None,
                    false,
                )],
                || {
                    super::super::native_entity_refs::tests::with_client_game(|game| {
                        let _server =
                            super::super::native_client_session::tests::logged_client(game);
                        Arc::make_mut(&mut game.remote.as_mut().unwrap().builtin)
                            .registries
                            .insert(
                                "minecraft:menu".into(),
                                sunrise_protocol::registry::Ids::from_names(vec![PANEL.into()]),
                            );
                        let _phase = mod_api::__private::PhaseOwner::scope(
                            mod_api::__private::ModPhase::World,
                        );
                        test(game, &opening);
                    })
                },
            );
        });
    }

    fn open_panel(game: &mut Game<'_>) {
        use sunrise_protocol::packets::play::{Clientbound, OpenScreen};
        game.handle_packet(Clientbound::OpenScreen(OpenScreen {
            container: 1,
            menu: 0,
            title: sunrise_core::nbt::Tag::String("Panel".into()),
        }))
        .unwrap();
        assert!(game.native_screen_shown());
    }

    #[test]
    #[ignore = "daily"]
    fn inventory_boxes_follow_menu_indices_and_the_input_layout() {
        with_panel(0, |game, opening| {
            game.screen = super::super::Screen::Inventory;
            let batch = super::super::HudBatch::new([800, 600], game.settings.gui_scale_inputs());
            let player = game.local_player().unwrap();
            let inventory = game.player_container_menu(player).unwrap();
            let boxes = game
                .resolve_client_menu(inventory)
                .unwrap()
                .inventory_slot_boxes(&batch)
                .unwrap();
            assert_eq!(boxes.len(), 46);
            for (index, expected_slot) in [(0, 200), (9, 9), (36, 0), (45, 504)] {
                let [x, y, w, h] = boxes[index].unwrap();
                assert_eq!(
                    game.slot_at(&batch, x + w / 2.0, y + h / 2.0),
                    Some(expected_slot)
                );
            }
            let future = menu_ref(player, 1, inventory.fields().generation);
            assert!(game.resolve_client_menu(future).is_none());
            game.screen = super::super::Screen::Playing;
            open_panel(game);
            let menu = game
                .player_container_menu(game.local_player().unwrap())
                .unwrap();
            assert_eq!(
                *opening.lock().unwrap(),
                Some((menu, 0)),
                "the same live handle and count are available during screen_opening"
            );
            assert_eq!(
                game.resolve_client_menu(menu)
                    .unwrap()
                    .inventory_slot_boxes(&batch),
                Some(vec![])
            );
            let remote = game.remote.as_mut().unwrap();
            remote.menu.set(Some((1, "unsupported".into())));
            let unknown = menu_ref(player, 1, remote.menu.generation);
            let view = game.resolve_client_menu(unknown).unwrap();
            assert_eq!(view.inventory_slot_boxes(&batch), None);
            assert_eq!(view.slot_count(), None);
            assert_eq!(view.carried(), Some(None));
        });
    }

    #[test]
    #[ignore = "daily"]
    fn number_key_swaps_predict_with_the_undisplayed_hotbar() {
        with_panel(1, |game, _| {
            open_panel(game);
            assert_eq!(game.slots(), [300]);
            let held = ItemStack::new("minecraft:stone", 5);
            let stored = ItemStack::new("minecraft:dirt", 2);
            game.inventory[2] = Some(held.clone());
            *game.slot_mut(300) = Some(stored.clone());
            assert!(game.menu_ops(&[(Some(300), 2, Input::Swap)]));
            assert_eq!(game.inventory[2], Some(stored));
            assert_eq!(*game.slot_mut(300), Some(held));
            assert_eq!(game.slots(), [300]);
        });
    }

    #[test]
    fn client_click_actions_encode_vanilla_ops() {
        let actions = [
            MenuClick::Pickup {
                slot: Some(36),
                button: MouseButton::Left,
            },
            MenuClick::Pickup {
                slot: None,
                button: MouseButton::Right,
            },
            MenuClick::QuickMove { slot: 9 },
            MenuClick::Swap {
                slot: 9,
                with: SwapWith::Hotbar(8),
            },
            MenuClick::Swap {
                slot: 9,
                with: SwapWith::Offhand,
            },
            MenuClick::Clone { slot: 9 },
            MenuClick::Throw {
                slot: 9,
                whole_stack: false,
            },
            MenuClick::Throw {
                slot: 9,
                whole_stack: true,
            },
            MenuClick::PickupAll { slot: 9 },
        ];
        assert_eq!(
            client_click_ops("inventory", &actions, true),
            Some(vec![
                (Some(0), 0, Input::Pickup),
                (None, 1, Input::Pickup),
                (Some(9), 0, Input::QuickMove),
                (Some(9), 8, Input::Swap),
                (Some(9), 40, Input::Swap),
                (Some(9), 2, Input::Clone),
                (Some(9), 0, Input::Throw),
                (Some(9), 1, Input::Throw),
                (Some(9), 0, Input::PickupAll),
            ])
        );
        for (kind, mode, creative) in [
            (DragKind::Split, 0, false),
            (DragKind::One, 4, false),
            (DragKind::Clone, 8, true),
        ] {
            assert_eq!(
                client_click_ops(
                    "inventory",
                    &[
                        MenuClick::Drag {
                            kind,
                            slots: vec![36, 9]
                        },
                        MenuClick::QuickMove { slot: 9 },
                    ],
                    creative
                ),
                Some(vec![
                    (None, mode, Input::QuickCraft),
                    (Some(0), mode + 1, Input::QuickCraft),
                    (Some(9), mode + 1, Input::QuickCraft),
                    (None, mode + 2, Input::QuickCraft),
                    (Some(9), 0, Input::QuickMove),
                ])
            );
        }
    }

    #[test]
    fn client_click_actions_refuse_invalid_batches_before_prediction() {
        assert!(client_click_ops("inventory", &[], false).is_none());
        for invalid in [
            MenuClick::QuickMove { slot: 46 },
            MenuClick::QuickMove { slot: usize::MAX },
            MenuClick::Swap {
                slot: 9,
                with: SwapWith::Hotbar(9),
            },
            MenuClick::Swap {
                slot: 9,
                with: SwapWith::Hotbar(u8::MAX),
            },
            MenuClick::Clone { slot: 9 },
            MenuClick::Drag {
                kind: DragKind::Clone,
                slots: vec![9],
            },
            MenuClick::Drag {
                kind: DragKind::Split,
                slots: vec![9, usize::MAX],
            },
        ] {
            assert!(
                client_click_ops(
                    "inventory",
                    &[MenuClick::QuickMove { slot: 9 }, invalid,],
                    false
                )
                .is_none()
            );
        }
        assert!(
            client_click_ops("unsupported", &[MenuClick::QuickMove { slot: 0 }], true).is_none()
        );
    }

    fn guest() -> Guest {
        let (tx, _) = std::sync::mpsc::channel();
        let mut g = Guest::new(
            Joined {
                conn: 1,
                uuid: 123,
                name: "guest".into(),
                properties: Vec::new(),
                info: ClientInformation::new(8),
                brand: String::new(),
                sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:1".parse().unwrap(),
                expected: false,
            },
            1,
            [0.0; 3],
            10,
        );
        g.placed = true;
        g
    }

    #[test]
    #[ignore = "daily"]
    fn a_server_closes_a_guests_menu_and_sets_their_selected_slot() {
        with_game(|game| {
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 4,
                simulation_distance: 4,
                dimension: "minecraft:overworld".into(),
                allow_flight: true,
                seed: 0,
                max_players: 10,
                integrated: true,
                levels: game.level_shapes,
                level_seeds: game.levels.runtime.seeds(),
            };
            let mut net = NetConfig::lan(0, "test".into(), 10);
            net.listen = false;
            game.lan_host(
                LanSettings {
                    port: 0,
                    guest_commands: false,
                    force_game_mode: false,
                },
                net,
                config,
            )
            .unwrap();
            game.lan.as_mut().unwrap().hub.guests.push(guest());
            let player = game.player(123).unwrap();

            // The selected slot: a slot past the hotbar is refused, a real one is held.
            assert_eq!(player.selected_slot(game), Some(0));
            assert!(!player.set_selected_slot(game, 9));
            assert!(player.set_selected_slot(game, 4));
            assert!(
                player.set_selected_slot(game, 4),
                "the same slot is no error"
            );
            assert_eq!(player.selected_slot(game), Some(4));
            assert_eq!(game.lan.as_ref().unwrap().hub.guests[0].selected, 4);

            // A menu: closing it returns the carried stack and leaves no menu behind.
            let stone = ItemStack::new("minecraft:stone", 4);
            let host = game.lan.as_mut().unwrap();
            {
                let value = Some(stone.clone());
                host.hub.guests.update(0, |guest| guest.carried = value);
            }
            host.hub
                .open_native_menu(
                    1,
                    1,
                    sunrise_core::nbt::Tag::String("test".into()),
                    vec![Some(stone.clone())],
                )
                .unwrap();
            let menu = game.player_container_menu(player).unwrap();
            assert_ne!(
                menu.with_view(game, |view| view.container_id()).flatten(),
                Some(0)
            );
            assert!(menu.close_container(game));
            let guest = &game.lan.as_ref().unwrap().hub.guests[0];
            assert!(guest.menu.is_none(), "the menu is gone");
            assert!(guest.carried.is_none(), "the cursor stack went back");
            assert_eq!(
                guest
                    .inventory
                    .iter()
                    .flatten()
                    .map(|s| s.count)
                    .sum::<u32>(),
                4,
                "to the inventory, none lost or doubled"
            );
            assert!(!menu.close_container(game), "a stale menu is refused");
            // The player's own inventory menu is never closed.
            let inventory = game.player_container_menu(player).unwrap();
            assert_eq!(
                inventory
                    .with_view(game, |view| view.container_id())
                    .flatten(),
                Some(0)
            );
            assert!(!inventory.close_container(game));
        });
    }

    #[test]
    #[ignore = "daily"]
    fn a_stack_is_thrown_from_the_eyes_as_player_drop_does() {
        use sunrise_client::motion::PlayerMotion;
        use sunrise_core::geometry::Vec3;

        with_game(|game| {
            // A body to throw from: `with_game` makes none.
            game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
            let stone = ItemStack::new("minecraft:stone", 3);
            let before = game.dropped.len();
            let id = game.drop_stack_from_eyes(stone.clone(), true).unwrap();
            assert_eq!(game.dropped.len(), before + 1);
            let thrown = game.dropped.last().unwrap();
            assert_eq!((thrown.id, &thrown.stack), (id, &stone));
            assert!(thrown.thrower.is_some(), "the drop key names the thrower");
            game.drop_stack_from_eyes(stone.clone(), false).unwrap();
            assert!(game.dropped.last().unwrap().thrower.is_none());
            assert_eq!(
                game.drop_stack_from_eyes(ItemStack::new("minecraft:stone", 0), true),
                None,
                "an empty stack is no drop"
            );
        });
    }

    #[test]
    #[ignore = "daily"]
    fn menus_read_storage_and_invalidate_on_replacement_wrap_and_reconnect() {
        with_game(|game| {
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 4,
                simulation_distance: 4,
                dimension: "minecraft:overworld".into(),
                allow_flight: true,
                seed: 0,
                max_players: 10,
                integrated: true,
                levels: game.level_shapes,
                level_seeds: game.levels.runtime.seeds(),
            };
            let mut net = NetConfig::lan(0, "test".into(), 10);
            net.listen = false;
            game.lan_host(
                LanSettings {
                    port: 0,
                    guest_commands: false,
                    force_game_mode: false,
                },
                net,
                config,
            )
            .unwrap();
            game.lan.as_mut().unwrap().hub.guests.push(guest());
            let player = game.player(123).unwrap();
            let inventory = game.player_container_menu(player).unwrap();
            assert_eq!(
                inventory.with_view(game, |view| {
                    assert_eq!(view.container_id(), Some(0));
                    assert_eq!(view.kind(), Some(None));
                    assert_eq!(view.slot_count(), Some(46));
                    assert_eq!(view.slot(0), Some(None));
                    assert_eq!(view.slot_backing(36), Some(MenuSlotBacking::Player(0)));
                    assert_eq!(view.slot_backing(45), Some(MenuSlotBacking::Player(40)));
                    assert_eq!(view.slot(46), None);
                }),
                Some(())
            );
            let stone = ItemStack::new("minecraft:stone", 4);
            let host = game.lan.as_mut().unwrap();
            {
                let value = Some(stone.clone());
                host.hub.guests.update(0, |g| g.inventory[9] = value);
            }
            host.hub.guests.update(0, |guest| {
                guest.carried = Some(ItemStack::new("minecraft:stick", 2));
            });
            let title = sunrise_core::nbt::Tag::String("test".into());
            let window = host
                .hub
                .open_native_menu(1, 1, title.clone(), vec![Some(stone.clone())])
                .unwrap();
            assert_eq!(
                inventory
                    .with_view(game, |view| view.slot_count())
                    .flatten(),
                None
            );
            let menu = game.player_container_menu(player).unwrap();
            let expected_stone = mod_api::ItemStack::new(
                mod_api::__private::item_key("minecraft:stone").unwrap(),
                4,
            );
            let expected_cursor = mod_api::ItemStack::new(
                mod_api::__private::item_key("minecraft:stick").unwrap(),
                2,
            );
            assert_eq!(
                menu.with_view(game, |view| {
                    assert_eq!(view.container_id(), Some(1));
                    assert_eq!(view.kind(), Some(Some("minecraft:generic_9x1")));
                    assert_eq!(view.slot_count(), Some(45));
                    assert_eq!(view.slot(0), Some(expected_stone.clone()));
                    assert_eq!(view.slot(1), Some(None));
                    assert_eq!(view.slot(9), Some(expected_stone));
                    assert_eq!(view.carried(), Some(expected_cursor));
                    assert_eq!(view.slot_backing(0), Some(MenuSlotBacking::Native(0)));
                    assert_eq!(view.slot_backing(9), Some(MenuSlotBacking::Player(9)));
                    assert_eq!(view.slot_backing(36), Some(MenuSlotBacking::Player(0)));
                    assert_eq!(view.slot_backing(45), None);
                }),
                Some(())
            );
            let host = game.lan.as_mut().unwrap();
            for _ in 0..100 {
                host.hub
                    .open_native_menu(1, 1, title.clone(), vec![])
                    .unwrap();
            }
            assert_eq!(host.hub.guests[0].menu.as_ref().unwrap().window, window);
            let mut stale_called = false;
            assert_eq!(menu.with_view(game, |_| stale_called = true), None);
            assert!(
                !stale_called,
                "the reused window id cannot lend its replacement"
            );
            let newer = game.player_container_menu(player).unwrap();
            let host = game.lan.as_mut().unwrap();
            host.hub.close_container(1);
            assert_eq!(
                newer.with_view(game, |view| view.slot_count()).flatten(),
                None
            );
            let inventory = game.player_container_menu(player).unwrap();
            let host = game.lan.as_mut().unwrap();
            *host.menus.entry(1).or_default().crafting.table = Some((
                7,
                0,
                Box::new(sunrise_client::menu::Menu::new(
                    sunrise_client::menu::Kind::Crafting,
                )),
                [0; 3],
            ));
            assert_eq!(game.player_container_menu(player), None);
            assert_eq!(
                inventory
                    .with_view(game, |view| view.slot_count())
                    .flatten(),
                None
            );
            let host = game.lan.as_mut().unwrap();
            host.play.clear();
            host.menus.clear();
            host.hub.guests.replace(0, guest());
            assert_eq!(
                inventory
                    .with_view(game, |view| view.slot_count())
                    .flatten(),
                None
            );

            use sunrise_client::{containers::BlockEntity, lan::menu::Slots};
            let a = (1, 2, 3);
            let b = (2, 2, 3);
            let stone = ItemStack::new("minecraft:stone", 3);
            game.world
                .block_entities
                .insert(a, BlockEntity::Chest(vec![None; 2]));
            game.world
                .block_entities
                .insert(b, BlockEntity::Chest(vec![Some(stone.clone()); 3]));
            game.lan.as_mut().unwrap().hub.open_container(
                1,
                "minecraft:generic_9x1",
                title.clone(),
                a,
                Slots::Double(a, b),
                vec![None; 5],
                None,
            );
            let menu = game.player_container_menu(player).unwrap();
            assert_eq!(
                menu.with_view(game, |view| view.slot_backing(2)).flatten(),
                Some(MenuSlotBacking::BlockEntity {
                    level: game.level("minecraft:overworld").unwrap(),
                    pos: [b.0, b.1, b.2],
                    slot: 0
                })
            );
            assert_eq!(
                menu.with_view(game, |view| view.slot(2)).flatten(),
                Some(Some(stone.try_into_api().unwrap()))
            );
            let vehicle = (i32::MAX - 15, VEHICLE_KEY_Y, 42);
            game.world
                .block_entities
                .insert(vehicle, BlockEntity::Chest(vec![Some(stone.clone())]));
            game.lan.as_mut().unwrap().hub.open_container(
                1,
                "minecraft:generic_9x1",
                title,
                vehicle,
                Slots::Block(vehicle),
                vec![None],
                None,
            );
            let menu = game.player_container_menu(player).unwrap();
            assert_eq!(
                menu.with_view(game, |view| view.slot_backing(0)).flatten(),
                Some(MenuSlotBacking::Entity {
                    entity: EntityRef::new(game.level("minecraft:overworld").unwrap().epoch(), 42),
                    slot: 0
                })
            );
            assert_eq!(
                menu.with_view(game, |view| view.slot(0)).flatten(),
                Some(Some(stone.try_into_api().unwrap()))
            );

            // Identical coordinates in different dimensions must not alias.
            let gold = ItemStack::new("minecraft:gold_ingot", 2);
            let emerald = ItemStack::new("minecraft:emerald", 3);
            game.world
                .block_entities
                .insert(a, BlockEntity::Chest(vec![Some(gold.clone())]));
            let title = sunrise_core::nbt::Tag::String("dimension".into());
            game.lan.as_mut().unwrap().hub.open_container(
                1,
                "minecraft:generic_9x1",
                title.clone(),
                a,
                Slots::Block(a),
                vec![Some(gold.clone())],
                None,
            );
            let overworld_menu = game.player_container_menu(player).unwrap();
            let overworld = game.level("minecraft:overworld").unwrap();
            assert!(
                game.enter_command_level(super::super::Dim::Nether)
                    .is_some()
            );
            game.world
                .block_entities
                .insert(a, BlockEntity::Chest(vec![Some(emerald.clone())]));
            let mut nether_guest = guest();
            nether_guest.conn = 2;
            nether_guest.uuid = 456;
            nether_guest.entity_id = 2;
            nether_guest.dimension = "minecraft:the_nether".into();
            game.lan.as_mut().unwrap().hub.guests.push(nether_guest);
            game.lan.as_mut().unwrap().hub.open_container(
                2,
                "minecraft:generic_9x1",
                title,
                a,
                Slots::Block(a),
                vec![Some(emerald.clone())],
                None,
            );
            let other_player = game.player(456).unwrap();
            let nether_menu = game.player_container_menu(other_player).unwrap();
            let nether = game.level("minecraft:the_nether").unwrap();
            assert_eq!(
                overworld_menu
                    .with_view(game, |view| view.slot(0))
                    .flatten(),
                Some(Some(gold.try_into_api().unwrap()))
            );
            assert_eq!(
                overworld_menu
                    .with_view(game, |view| view.slot_backing(0))
                    .flatten(),
                Some(MenuSlotBacking::BlockEntity {
                    level: overworld,
                    pos: [a.0, a.1, a.2],
                    slot: 0
                })
            );
            assert!(
                game.enter_command_level(super::super::Dim::Overworld)
                    .is_some()
            );
            assert_eq!(
                nether_menu.with_view(game, |view| view.slot(0)).flatten(),
                Some(Some(emerald.try_into_api().unwrap()))
            );
            assert_eq!(
                nether_menu
                    .with_view(game, |view| view.slot_backing(0))
                    .flatten(),
                Some(MenuSlotBacking::BlockEntity {
                    level: nether,
                    pos: [a.0, a.1, a.2],
                    slot: 0
                })
            );
        });
    }
}
