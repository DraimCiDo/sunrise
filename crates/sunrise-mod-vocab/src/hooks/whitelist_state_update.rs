//! The `whitelist_state_update` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0342

pub trait Hooks {
    /// The dedicated server's whitelist changed to `enabled`.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's WhitelistToggleEvent)",
        paper = "WhitelistToggleEvent",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_whitelist_state_update",
        native = "fn native_whitelist_state_update(&mut self, enabled: bool)"
    )]
    fn whitelist_state_update();
}
