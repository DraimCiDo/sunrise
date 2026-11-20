//! The `tab_complete` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0333

pub trait Hooks {
    /// A guest asked to complete `command`; owned API `Suggestion` records may be
    /// added to or filtered from `suggestions`. `range` is the replaced byte range.
    /// The game snapshots completions only when a listener subscribes.
    #[hook(
        side = "server",
        contract = "collect",
        out = "suggestions",
        neoforge = "none (NeoForge has no generic tab completion event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_tab_complete",
        native = "fn native_tab_complete(&mut self, player: u128, name: &str, command: &str, range: [usize; 2], suggestions: &mut Vec<Suggestion>)"
    )]
    fn tab_complete();
}
