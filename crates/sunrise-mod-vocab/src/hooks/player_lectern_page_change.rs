//! The `player_lectern_page_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0257

pub trait Hooks {
    /// A server player changed the page of a lectern, after its page and redstone pulse changed.
    /// `page` is the zero-based new page. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_lectern_page_change",
        native = "fn native_player_lectern_page_change(&mut self, player: u128, name: &str, pos: [i32; 3], page: i32)"
    )]
    fn player_lectern_page_change();
}
