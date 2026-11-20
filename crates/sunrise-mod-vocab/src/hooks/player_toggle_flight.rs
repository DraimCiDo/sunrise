//! The `player_toggle_flight` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0165

pub trait Hooks {
    /// A player's client started (`flying`) or stopped flying (vanilla's
    /// `handlePlayerAbilities`, which honours it only for a player who may
    /// fly: creative or spectator), heard once for each change, on a server,
    /// with the player's state swapped in, so `PlayerRef::tell` and the other
    /// helpers reach them. The player is already flying or not as asked, as
    /// vanilla leaves them; set `cancel` to refuse it: they keep the state
    /// they had and the server sends their abilities back, as vanilla does
    /// for what it refuses, so the client lands or flies again. Only what a
    /// client asks for is heard: not the flight a game-mode change starts or
    /// ends (`/gamemode`), and not a LAN host's own player.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no flight toggle event; Bukkit's PlayerToggleFlightEvent)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_toggle_flight",
        native = "fn native_player_toggle_flight(&mut self, player: u128, name: &str, flying: bool, cancel: &mut bool)"
    )]
    fn player_toggle_flight();
}
