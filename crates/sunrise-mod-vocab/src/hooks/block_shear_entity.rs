//! The `block_shear_entity` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0181

pub trait Hooks {
    /// A dispenser sheared `entity` of kind `kind` at the block in front of
    /// its own block at `pos`, after the shearing effects. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_block_shear_entity",
        native = "fn native_block_shear_entity(&mut self, dimension: &str, pos: [i32; 3], entity: u32, kind: &str)"
    )]
    fn block_shear_entity();
}
