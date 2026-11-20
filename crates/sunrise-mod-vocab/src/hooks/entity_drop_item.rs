//! The `entity_drop_item` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0201

pub trait Hooks {
    /// A mob is about to drop `count` of `item` at `pos` (Bukkit's
    /// `EntityDropItemEvent`).
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's EntityDropItemEvent)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_entity_drop_item",
        native = "fn native_entity_drop_item(&mut self, entity: u32, item: &str, count: i32, pos: [f64; 3])"
    )]
    fn entity_drop_item();
}
