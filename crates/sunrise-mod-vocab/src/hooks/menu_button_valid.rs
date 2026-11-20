//! The `menu_button_valid` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Whether this menu's owner accepts a button id before `menu_button` runs.
    /// The owner chooses the ids for its own menu; there is no shared numeric
    /// range. Leave `accepted` false for ids the active menu does not support.
    #[notes = r#"### Menu button IDs

`menu_button_valid` asks the owner of the active menu whether it accepts a button ID. The owner chooses its IDs;
there is no shared range. Only an accepted ID reaches `menu_button`."#]
    #[hook(
        side = "server",
        contract = "cancel",
        out = "accepted",
        scope = "menu",
        key = "menu",
        host = "read",
        neoforge = "none (vanilla menu button ids are handled by their menu implementation)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_menu_button_valid",
        native = "fn native_menu_button_valid(&self, menu: &str, block: Option<&str>, id: i32, accepted: &mut bool)"
    )]
    fn menu_button_valid();
}
