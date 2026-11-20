//! The `player_stonecutter_recipe_select` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0311

pub trait Hooks {
    /// A server player selected a stonecutter recipe, after the menu accepted
    /// its button. `recipe` is the zero-based button index. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_stonecutter_recipe_select",
        native = "fn native_player_stonecutter_recipe_select(&mut self, player: u128, name: &str, recipe: i32)"
    )]
    fn player_stonecutter_recipe_select();
}
