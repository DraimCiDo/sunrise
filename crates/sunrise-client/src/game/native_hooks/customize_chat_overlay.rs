use super::*;

impl Game<'_> {
    /// The original background chat body is about to draw (`CustomizeGuiOverlayEvent.Chat`):
    /// `x`, `y` are its bottom-left corner in GUI pixels, which a mod may move.
    pub(in crate::game) fn native_customize_chat_overlay(
        &mut self,
        gui_width: i32,
        gui_height: i32,
        x: &mut i32,
        y: &mut i32,
    ) {
        self.fire_customize_chat_overlay(gui_width, gui_height, x, y);
    }
}

#[cfg(test)]
mod customize_chat_overlay_tests {
    use mod_api::{ClientHost, CustomizeChatOverlayEvent, Hooks, ModEntry};
    use mod_platform::testing::with_mods;
    use std::sync::{Arc, Mutex};
    use sunrise_client::hud::HudBatch;
    use sunrise_client::text::Text;

    type Seen = Arc<Mutex<Vec<(i32, i32, i32, i32)>>>;

    /// Lifts the chat 100 GUI pixels, logging what it was told.
    struct Lift(Seen);
    #[mod_api::__subscriptions]
    impl Hooks for Lift {
        fn customize_chat_overlay(
            &self,
            _: &mut dyn ClientHost,
            event: &mut CustomizeChatOverlayEvent,
        ) {
            let e = &*event;
            self.0
                .lock()
                .unwrap()
                .push((e.gui_width, e.gui_height, e.x, e.y));
            event.y -= 100;
        }
    }

    /// The top of the original HUD chat's paint, in GUI pixels.
    fn chat_top(game: &mut super::Game<'_>) -> f32 {
        let mut batch = HudBatch::scratch([1280, 720]);
        game.draw_background_chat(&mut batch);
        batch
            .vertices
            .iter()
            .map(|v| v.pos[1])
            .reduce(f32::min)
            .unwrap()
    }

    /// `CustomizeGuiOverlayEvent.Chat`: the mod gets vanilla's place (0, GUI height - 40) and the
    /// GUI size, and the background chat is drawn where it moved it.
    #[test]
    #[ignore = "daily"]
    fn a_client_mod_moves_the_chat() {
        let message = Text::literal("hello from the chat test");
        let mut plain = None;
        crate::game::native_entity_refs::tests::with_game(|game| {
            game.options.execution_role = super::ExecutionRole::Client;
            game.system_message(message.clone());
            plain = Some(chat_top(game));
        });
        let plain = plain.expect("the message is drawn");
        let seen = Seen::default();
        let mods: mod_api::Mods =
            vec![ModEntry::new("lift", Box::new(Lift(Arc::clone(&seen))))].into();
        with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.execution_role = super::ExecutionRole::Client;
                game.system_message(message.clone());
                let moved = chat_top(game);
                assert_eq!(plain - moved, 100.0);
                let b = HudBatch::scratch([1280, 720]);
                let (w, h) = (b.width as i32, b.height as i32);
                assert_eq!(*seen.lock().unwrap(), [(w, h, 0, h - 40)]);
            });
        });
    }
}
