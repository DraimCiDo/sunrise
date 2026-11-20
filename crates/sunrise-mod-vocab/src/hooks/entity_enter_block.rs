//! The `entity_enter_block` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0202

pub trait Hooks {
    /// A bee entered the hive at `pos` and was stored there.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_entity_enter_block",
        native = "fn native_entity_enter_block(&mut self, entity: u32, kind: &str, pos: [i32; 3], block: &str)"
    )]
    fn entity_enter_block();
}
