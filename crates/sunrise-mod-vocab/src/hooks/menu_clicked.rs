//! The `menu_clicked` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0034

pub trait Hooks {
    /// A click in a menu a mod opened (`native_open_menu`), heard raw: the menu's `slot`
    /// (its own first, then the player's inventory, `native_menu_inventory_slot`; -999
    /// outside), the `button` and the click's `mode` (`ClickType`'s id) of the player `uuid`.
    /// A single-slot drag is heard once. Menu-button clicks, recipe placement and creative
    /// set-slot never reach it. The hook cannot know whether the click changes anything, and
    /// it carries no pickup answer and takes nothing: the client is sent the menu, its
    /// inventory and its cursor as they were.
    /// No player's state is swapped in: the menu helpers take the player's UUID.
    /// `ItemPlace::MenuSlot` reaches the menu's own storage and its player's inventory and
    /// hotbar slots. `PlayerSlot` and `PlayerHand` resolve the online clicking player through
    /// the usual acting-player or guest path. For a player still connected, the mods are
    /// told inside an action: what a listener writes through the `Host` stays when it
    /// returns, and is undone, with the rest of the action, when a listener fails (panics or
    /// traps). A click drained after its player left is still told, outside an action.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (vanilla AbstractContainerMenu.clicked; NeoForge adds no event or extension for it)",
        version = 2,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_menu_clicked",
        native = "fn native_menu_clicked(&mut self, uuid: u128, window: i32, slot: i32, button: i32, mode: i32)"
    )]
    fn menu_clicked();
}
