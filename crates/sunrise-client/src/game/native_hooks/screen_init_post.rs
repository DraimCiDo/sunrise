use super::*;
use mod_api::client::widgets::ScreenWidgets;

impl Game<'_> {
    pub(in crate::game) fn has_screen_init_post(&self) -> bool {
        self.options.execution_role.is_client()
            && self.lan_acting.is_none()
            && mod_api::__private::listened(mod_api::HookId::ScreenInitPost)
    }

    /// The stock screen has built its widgets and established initial focus.
    pub(in crate::game) fn native_screen_init_post(
        &self,
        screen: &ScreenView<'_>,
        widgets: &mut ScreenWidgets,
    ) {
        self.fire_screen_init_post(screen, widgets);
    }
}
