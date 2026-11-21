//! The `player_food` area: food level, saturation and exhaustion. Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The handles' writes of the `player_food` area, implemented by the game and reached through [`BackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait PlayerFoodBackendMut {
    /// Side: server.
    /// `FoodData.eat(nutrition, saturationModifier)`: adds `nutrition` to
    /// the food level (0 to 20) and `nutrition * modifier * 2` to the
    /// saturation, which stays at most the food level.
    #[access(WorldOnly)]
    fn player_feed(&mut self, player: PlayerRef, nutrition: i32, saturation_modifier: f32) -> bool;
    /// Side: server.
    /// `Player.causeFoodExhaustion`: adds exhaustion (at most 40), as
    /// sprinting or jumping does. An invulnerable player (creative) or a
    /// spectator gets none, and the call still returns true.
    #[access(WorldOnly)]
    fn player_add_exhaustion(&mut self, player: PlayerRef, amount: f32) -> bool;
    /// Side: server.
    /// `FoodData.setFoodLevel`: sets the food level as it is, unclamped,
    /// as vanilla does.
    #[access(WorldOnly)]
    fn player_set_food_level(&mut self, player: PlayerRef, level: i32) -> bool;
    /// Side: server.
    /// `FoodData.setSaturation`: sets the saturation as it is, unclamped.
    #[access(WorldOnly)]
    fn player_set_saturation(&mut self, player: PlayerRef, saturation: f32) -> bool;
    /// Side: server.
    /// Sets the exhaustion as it is (Paper's `HumanEntity#setExhaustion`;
    /// vanilla's `FoodData` only adds to it).
    #[access(WorldOnly)]
    fn player_set_exhaustion(&mut self, player: PlayerRef, exhaustion: f32) -> bool;
}
