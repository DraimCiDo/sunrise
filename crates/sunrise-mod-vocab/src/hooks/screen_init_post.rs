//! The `screen_init_post` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// After the screen builds its stock widgets and establishes initial focus,
    /// before it draws or accepts input (`ScreenEvent.Init.Post`). A rebuild
    /// fires again; an unchanged frame does not. `widgets` exposes the staged
    /// listener list on `title` and `paused`.
    ///
    /// Not on wasm: the listener list binds native button callbacks.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "ScreenEvent.Init.Post",
        version = 2,
        cancellable = false
    )]
    fn screen_init_post();
}
