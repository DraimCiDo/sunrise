//! The `exp_bottle` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0211

pub trait Hooks {
    /// An experience bottle broke and its experience was awarded.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_exp_bottle",
        native = "fn native_exp_bottle(&mut self, pos: [f64; 3], experience: u32)"
    )]
    fn exp_bottle();
}
