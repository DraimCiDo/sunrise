//! The `component_types` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0074

pub trait Hooks {
    /// Register namespaced NBT component types before content loads.
    #[hook(
        side = "both",
        contract = "collect",
        out = "types",
        context = "registration",
        neoforge = "RegisterEvent",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_component_types",
        native = "fn native_component_types(context: &mod_api::RegistrationContext, types: &mut Vec<String>)"
    )]
    fn component_types();
}
