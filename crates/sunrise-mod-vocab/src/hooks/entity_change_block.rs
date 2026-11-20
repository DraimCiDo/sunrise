//! The `entity_change_block` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0200

pub trait Hooks {
    /// A mob changed a block at `pos`, after the new state was applied.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_entity_change_block",
        native = "fn native_entity_change_block(&mut self, entity: u32, kind: &str, pos: [i32; 3], from: &str, to: &str)"
    )]
    fn entity_change_block();
}
