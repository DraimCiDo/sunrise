//! The `brewing_start` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0184

pub trait Hooks {
    /// A brewing stand at `pos` began brewing `ingredient`, after fuel and the timer were set. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_brewing_start",
        native = "fn native_brewing_start(&mut self, pos: [i32; 3], ingredient: &str)"
    )]
    fn brewing_start();
}
