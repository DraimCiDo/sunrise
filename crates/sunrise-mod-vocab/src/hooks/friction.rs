//! The `friction` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The friction of `state` at `pos` (`IBlockExtension.getFriction`), for
    /// an optional entity (its id and type). The state need not occupy that position.
    /// The read-only level host borrows block, state, light and fluid data on the side
    /// that made the query. It gives no block-entity handles: use the mod's own state
    /// on each side for an answer that depends on a block entity.
    /// Set `event.friction` to answer; it starts at the state's own coefficient. Answers outside
    /// `0..=2` (including non-finite values) keep that coefficient.
    /// Runs on both sides, including during client replay. Keep the answer a pure
    /// function of the event and the level reads.
    #[notes = r#"## Block friction

`BlockPart::friction` answers NeoForge's `IBlockExtension.getFriction` for a state at a position,
with an optional entity. It runs for ground travel, ground drag, boat ground friction and lead
angular friction, and for explicit `LevelRef::friction` queries. The read-only `LevelSide` host
can read the level or ask for another state's answer at the same position, for example a block
taking its material's friction. The queried state need not be what the level holds at that position.
The borrowed view reads block states, their properties, fluids and light, without loading chunks.
`block_state_id` takes a bare block id here; use `state_with_property` to select another state.
It has no block-entity, container, shape, signal, tag-membership, weather or feature view.
`is_raining_at` reports false and `enabled_features` reports the vanilla default. For a rule
depending on block-entity data, answer from the mod's own state on both sides.

An active query on the same level, position, state and entity id (or no entity) cannot be repeated.
A recursive or over-deep query returns `None` (`Ok(None)` through `LevelRef`). Without a part,
or when the part leaves the answer alone or writes a value outside `0..=2`, the state's own
coefficient is used. Physics still applies vanilla's friction modifier and drag at the sites that
used them before. Register the part on both sides and keep its answer pure so prediction and
replay agree."#]
    #[hook(
        side = "both",
        contract = "listen",
        cancellable = false,
        scope = "block",
        key = "state",
        host = "level",
        version = 2,
        non_exhaustive = true,
        target = "sunrise_client::game::native_hooks::friction::FrictionView<'_>::native_friction",
        neoforge = "IBlockExtension.getFriction"
    )]
    fn friction();
}
