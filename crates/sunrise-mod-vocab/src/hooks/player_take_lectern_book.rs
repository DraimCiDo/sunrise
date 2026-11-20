//! The `player_take_lectern_book` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0263

pub trait Hooks {
    /// A server player took the book from a lectern, after the menu and block entity changed. `book`
    /// is the removed item id. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_take_lectern_book",
        native = "fn native_player_take_lectern_book(&mut self, player: u128, name: &str, pos: [i32; 3], book: &str)"
    )]
    fn player_take_lectern_book();
}
