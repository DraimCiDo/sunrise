//! The `prepare_smithing` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0326

pub trait Hooks {
    /// A smithing result was prepared for a server player at `pos`. Change it before it is sent.
    #[hook(
        side = "server",
        contract = "replace",
        out = "result",
        neoforge = "none (NeoForge has no such event)",
        paper = "PrepareSmithingEvent",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_prepare_smithing",
        native = "fn native_prepare_smithing(&mut self, player: u128, name: &str, pos: [i32; 3], result: &mut Option<ItemStack>)"
    )]
    fn prepare_smithing();
}
