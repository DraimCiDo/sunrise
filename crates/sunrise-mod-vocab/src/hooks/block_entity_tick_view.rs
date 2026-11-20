//! The branch-only resident block-entity tick prototype.

pub trait Hooks {
    /// Tick a subscribed block entity in `Level.tickBlockEntities`, without taking it out of
    /// the level. The resident ticker replaces the snapshot tick for a Part's type when present.
    /// It is server-only and has no cancellation reply. The checked subject editor needs no
    /// second mutable Host borrow; it joins any transaction that encloses the tick.
    #[hook(
        side = "server",
        contract = "listen",
        scope = "block_entity",
        key = "kind",
        neoforge = "none (vanilla BlockEntityTicker; prototype)"
    )]
    fn block_entity_tick_view();
}
