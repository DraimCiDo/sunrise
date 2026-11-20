//! A configuration task's tick callback.
pub trait Hooks {
    /// Check this running task once per server tick. Set finished when its check succeeds.
    /// connection.finish(host, task_type) verifies the active type. An error or
    /// panic fails this connecting client with the configuration-error reason.
    #[hook(
        side = "server",
        contract = "collect",
        out = "finished",
        target = "sunrise_client::game::native_hooks::register_configuration_tasks::Game<'_>::native_configuration_task_tick",
        native = "fn native_configuration_task_tick(&mut self, connection: mod_api::ConfigurationConnection, task_type: &Identifier, finished: &mut bool)"
    )]
    fn configuration_task_tick();
}
