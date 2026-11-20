//! The `item_spawn` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0232

pub trait Hooks {
    /// A dropped item entity was successfully added to the server level.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_item_spawn",
        native = "fn native_item_spawn(&mut self, entity: u32, item: &str, count: i32, pos: [f64; 3])"
    )]
    fn item_spawn();
}
