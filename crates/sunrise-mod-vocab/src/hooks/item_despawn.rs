//! The `item_despawn` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0230

pub trait Hooks {
    /// A dropped item entity reached its 6000-tick lifetime and is about to be
    /// removed (Bukkit's `ItemDespawnEvent`).
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's ItemDespawnEvent)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_item_despawn",
        native = "fn native_item_despawn(&mut self, entity: u32, item: &str, count: i32, pos: [f64; 3])"
    )]
    fn item_despawn();
}
