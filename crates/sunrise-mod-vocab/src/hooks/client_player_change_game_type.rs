//! The `client_player_change_game_type` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `ClientPlayerChangeGameTypeEvent`: observe the local player info game type change.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "ClientPlayerChangeGameTypeEvent",
        cancellable = false,
        native = "fn native_client_player_change_game_type(&mut self, player_id: i32, player_uuid: u128, current_game_type: i32, new_game_type: i32)"
    )]
    fn client_player_change_game_type();
}
