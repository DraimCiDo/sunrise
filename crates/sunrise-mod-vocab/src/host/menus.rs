//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The server host's methods of the `menus` area, implemented by the game and reached through [`ServerHost`].
#[sunrise_mod_api_macros::host_area]
pub trait MenusServerHost {
    /// Side: server.
    /// Opens a chest menu of `rows` rows (1 to 6) titled `title` for the
    /// player, showing `items` (by slot, padded with empty ones), in place of
    /// the menu they had open: its window id, a new one each time
    /// (`ServerPlayer.openMenu`). The old menu closes as a close does, its
    /// cursor stack going back to the player's inventory; `replace` says
    /// whether the client is first sent a close for it
    /// (`MenuProvider.shouldTriggerClientSideContainerClosingOnOpen`).
    /// `MenuReplace::InPlace` changes the title and rows without the client
    /// centring the mouse again. Packets for the old window are ignored. The
    /// items of an open menu change through its slots (`menu`), not by
    /// opening it again.
    #[access(WorldOnly)]
    fn open_menu(
        &mut self,
        uuid: u128,
        rows: u8,
        title: Text,
        items: Vec<Option<ItemStack>>,
        replace: MenuReplace,
    ) -> Result<Option<i32>, StackWriteError>;
    /// Side: server.
    /// The menu `open_menu` opened that the player has open as window
    /// `window` (the window id `open_menu`, `menu_clicked` and `menu_closed`
    /// give). Its slots are written as `ItemPlace::MenuSlot` in a
    /// transaction, and the client is sent each changed slot with the next
    /// tick's container sync (`AbstractContainerMenu.broadcastChanges`), as
    /// for any open menu. None for another window, a menu the game opened, or
    /// a player who isn't here.
    #[access(WorldOnly)]
    fn menu(&self, uuid: u128, window: i32) -> Option<MenuRef>;
    /// Side: server.
    /// Closes the player's native menu (a `menu_closed` follows).
    #[access(WorldOnly)]
    fn close_menu(&mut self, uuid: u128);
    /// Side: server.
    /// Replaces everything the player's open `open_menu` menu `window` shows with `items` (by
    /// slot, padded with empty ones, extra ones dropped, as `open_menu` takes them) and sends
    /// it at once as one `ClientboundContainerSetContentPacket`
    /// (`AbstractContainerMenu.sendAllDataToRemote`), without reopening the menu: the title,
    /// the cursor and the screen stay. Writing slots through `menu` sends each changed slot with
    /// the next tick's sync instead; this is the whole-contents form, for a page turned or a
    /// list rebuilt. Each refusal sends nothing and changes nothing, and is named by
    /// `MenuItemsRefusal`: `Client`, `NoPlayer` (no placed player of that UUID), `NotOpen`
    /// (`window` is not their open `open_menu` menu) and `Stack` (a stack the game's item
    /// conversion refuses).
    #[access(WorldOnly)]
    fn menu_set_items(
        &mut self,
        uuid: u128,
        window: i32,
        items: Vec<Option<ItemStack>>,
    ) -> Result<(), MenuItemsRefusal>;
}
/// The client host's methods of the `menus` area, implemented by the game and reached through [`crate::ClientHost`].
#[sunrise_mod_api_macros::host_area]
pub trait MenusClientHost {
    /// Side: client.
    /// The local player in a logged-in client world.
    #[access(ClientSetup)]
    fn local_player(&self) -> Option<PlayerRef>;
}
/// The handles' reads of the `menus` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusBackend {
    /// Side: both.
    /// Lends the current menu once, or skips the callback if it is unavailable.
    #[access(WorldOnly)]
    #[decline_unavailable]
    fn with_menu_view(&self, menu: MenuRef, read: &mut dyn FnMut(&dyn MenuView));
}
/// The handles' client reads of the `menus` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusClientBackend {
    /// Side: client.
    /// Lends the current client menu once, skips a stale menu, or refuses an unavailable read.
    #[access(WorldOnly)]
    fn with_client_menu_view(
        &self,
        menu: MenuRef,
        read: &mut dyn FnMut(&dyn ClientMenuView),
    ) -> Result<(), LevelReadError>;
}
/// The handles' client writes of the `menus` area, implemented by the game and reached through [`ClientBackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusClientBackendMut {
    /// Side: client.
    /// Submit clicks on the local player's live menu through normal prediction
    /// and `ContainerClick`. Slots use the same menu order as `slot`; `None`
    /// is outside (-999). All clicks are validated before any prediction or send.
    /// A drag expands to one complete quick-craft gesture.
    /// Refuses stale menus, invalid slots/hotbar indices, spectators, non-client hosts,
    /// the creative inventory screen and empty batches without changing items.
    /// True means submitted, not accepted by the server: its corrections win.
    #[access(WorldOnly)]
    fn menu_click(&mut self, menu: MenuRef, clicks: &[MenuClick]) -> bool;
    /// Side: client.
    /// Close the local player's open container screen as Escape does
    /// (`AbstractContainerScreen.onClose`, `LocalPlayer.closeContainer`): the
    /// server is told (`ContainerClose`) and the screen goes. Refuses stale
    /// menus, non-client hosts and a menu whose screen is not open, such as
    /// the inventory menu while playing. A server closes a menu with `close_container`.
    #[access(WorldOnly)]
    fn menu_close(&mut self, menu: MenuRef) -> bool;
    /// Side: client.
    /// Press button `button` of the local player's open menu as a click on
    /// one of its buttons does (`MultiPlayerGameMode.handleInventoryButtonClick`,
    /// `ContainerButtonClick`): the server's menu decides what it does
    /// (`clickMenuButton`, and `menu_button` for a mod's menu). For a client
    /// mod's own input on a menu's screen where `screen_click` doesn't come,
    /// such as a key or the wheel; no sound, that is the screen's. Refuses
    /// stale menus, non-client hosts and the inventory menu, which no server
    /// opened. True means sent, not that the menu took it.
    #[access(WorldOnly)]
    fn menu_click_button(&mut self, menu: MenuRef, button: i32) -> bool;
}
/// The handles' server writes of the `menus` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusBackendMut {
    /// Side: server.
    /// Close a player's open menu as `ServerPlayer.closeContainer` does, running the normal
    /// lifecycle once: the container's lid, a vehicle's or a trade's stand-in, the crafting,
    /// station and mount menus, the carried stack back to the inventory,
    /// `player_container_close`, then the client's window closed (`ContainerClose`). Refuses
    /// stale menus, clients, a player whose state is swapped in for a turn, and the player's own
    /// inventory menu, which is never closed. A client closes its own screen with `close`.
    #[access(WorldOnly)]
    fn menu_close_container(&mut self, menu: MenuRef) -> bool;
    /// Side: server.
    /// Set a data slot of the owning mod's current blockless menu (`DataSlot.set`).
    /// Returns true only if the value changed. Publishes the final signed `i32` at
    /// root commit (immediately outside a transaction); an abort restores it silently.
    /// Stale handles, unsupported menus, other mods and undeclared indices are refused.
    #[access(WorldOnly)]
    fn menu_set_data_slot(
        &mut self,
        menu: MenuRef,
        index: usize,
        value: i32,
    ) -> Result<bool, MenuDataRefusal>;
}
