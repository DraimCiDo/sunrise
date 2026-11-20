use super::*;
use crate::game::native_entity_refs::tests::{run_fresh_cases, with_client_game};
use mod_api::{
    ClientHost, ConfigScreenContext, ConfigScreenFactory, ConfigScreenParent, ConfigScreenRefusal,
    ConfigScreenRegistrationError, ConfigScreenSession, Flow, Hooks, ModEntry, Registrar,
    RegistrationContext, ScreenCharTypedPreEvent, ScreenKeyPressedPreEvent,
    ScreenKeyReleasedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseButtonReleasedPreEvent,
    ScreenMouseDraggedPreEvent, ScreenMouseScrolledPreEvent, ScreenOpeningEvent,
    ScreenRenderPostEvent, SettingSpec,
};
use std::sync::{Arc, Mutex};
use sunrise_client::mod_list::Installed;
use sunrise_client::options_ui::Page;
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};

#[derive(Default)]
struct State {
    refuse: bool,
    veto: bool,
    calls: Vec<bool>,
    input: Vec<&'static str>,
    parent: Option<ConfigScreenParent>,
}

struct Factory(Arc<Mutex<State>>);
impl ConfigScreenFactory for Factory {
    fn create_screen(
        &self,
        owner: &str,
        context: ConfigScreenContext,
        parent: ConfigScreenParent,
    ) -> Result<ConfigScreenSession, ConfigScreenRefusal> {
        assert_eq!(owner, "fixture");
        let mut state = self.0.lock().unwrap();
        state.calls.push(context.in_world());
        if state.refuse {
            return Err(ConfigScreenRefusal::Unavailable);
        }
        state.parent = Some(parent.clone());
        Ok(parent.screen())
    }
}

struct Duplicate;
impl ConfigScreenFactory for Duplicate {
    fn create_screen(
        &self,
        _: &str,
        _: ConfigScreenContext,
        _: ConfigScreenParent,
    ) -> Result<ConfigScreenSession, ConfigScreenRefusal> {
        panic!("duplicate registration replaced the first binding")
    }
}

struct ConfigMod(Arc<Mutex<State>>);
#[mod_api::__subscriptions]
impl Hooks for ConfigMod {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_setting(SettingSpec::toggle("show", "test.show", false))
            .unwrap();
        r.register_config_screen_factory(Box::new(Factory(self.0.clone())))
            .unwrap();
        assert_eq!(
            r.register_config_screen_factory(Box::new(Duplicate)),
            Err(ConfigScreenRegistrationError::Duplicate),
        );
    }

    fn screen_opening(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenOpeningEvent<'_>,
    ) -> Option<ScreenOpening> {
        (event.new.id == "mod" && self.0.lock().unwrap().veto).then_some(ScreenOpening::Cancel)
    }

    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        if event.screen.id == "mod" && host.mod_screen() == Some("fixture") {
            event.draw.fill([10.0, 20.0, 12.0, 8.0], 0xfffa20e0);
        }
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        self.input(event.screen.id, "press")
    }

    fn screen_mouse_button_released_pre(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenMouseButtonReleasedPreEvent<'_>,
    ) -> Flow {
        self.input(event.screen.id, "release")
    }

    fn screen_mouse_dragged_pre(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenMouseDraggedPreEvent<'_>,
    ) -> Flow {
        self.input(event.screen.id, "drag")
    }

    fn screen_mouse_scrolled_pre(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenMouseScrolledPreEvent<'_>,
    ) -> Flow {
        self.input(event.screen.id, "scroll")
    }

    fn screen_key_pressed_pre(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenKeyPressedPreEvent<'_>,
    ) -> Flow {
        if event.key == "key.keyboard.w" {
            self.input(event.screen.id, "key down")
        } else {
            Flow::Continue
        }
    }

    fn screen_key_released_pre(
        &self,
        _: &dyn ClientHost,
        event: &mut ScreenKeyReleasedPreEvent<'_>,
    ) -> Flow {
        if event.key == "key.keyboard.w" {
            self.input(event.screen.id, "key up")
        } else {
            Flow::Continue
        }
    }

    fn screen_char_typed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenCharTypedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id == "mod" && event.c == 'x' {
            assert_eq!(host.setting_bool("show"), Some(false));
            host.set_setting("fixture", "show", "true");
            host.set_setting("fixture", "show", "invalid");
            assert_eq!(host.setting_bool("show"), Some(true));
        }
        self.input(event.screen.id, "char")
    }
}

impl ConfigMod {
    fn input(&self, screen: &str, event: &'static str) -> Flow {
        if screen != "mod" {
            return Flow::Continue;
        }
        self.0.lock().unwrap().input.push(event);
        if event == "press" {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }
}

struct Generated;
#[mod_api::__subscriptions]
impl Hooks for Generated {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_setting(SettingSpec::toggle("show", "test.show", false))
            .unwrap();
    }
}

struct NoLoop;
impl EventLoopCtl for NoLoop {
    fn exit(&self) {
        panic!("fixture requested exit");
    }
    fn set_control_flow(&self, _: winit::event_loop::ControlFlow) {}
}

#[test]
#[ignore = "daily"]
fn typed_config_screen_keeps_its_mods_parent_and_owns_input() {
    if run_fresh_cases(
        "game::native_hooks::base::config_screen_tests::typed_config_screen_keeps_its_mods_parent_and_owns_input",
        "SUNRISE_CONFIG_SCREEN_CHILD",
        &["window"],
    ) {
        return;
    }
    let state = Arc::new(Mutex::new(State::default()));
    let mods = vec![
        ModEntry::new("fixture", Box::new(ConfigMod(state.clone()))),
        ModEntry::new("generated", Box::new(Generated)),
    ]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_client_game(|game| {
            let mut builder = winit::event_loop::EventLoop::builder();
            #[cfg(target_os = "linux")]
            {
                use winit::platform::x11::EventLoopBuilderExtX11;
                builder.with_x11().with_any_thread(true);
            }
            let event_loop = builder.build().unwrap();
            #[allow(deprecated)]
            let window = Arc::new(
                event_loop
                    .create_window(
                        winit::window::Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(winit::dpi::PhysicalSize::new(480, 240)),
                    )
                    .unwrap(),
            );
            game.options.offscreen = true;
            game.renderer = Some(game.new_renderer(window.clone()).unwrap());
            let window_id = window.id();
            game.window = Some(window);

            let row = |id: &str| Installed {
                id: id.into(),
                title: "Z Test Mod".into(),
                description: String::new(),
                problem: None,
                dev: false,
                native: None,
            };
            let click = |game: &mut Game<'_>, x, y| {
                let events = game.ui.options.click(
                    &mut game.settings,
                    &game.gui.atlas,
                    x,
                    y,
                    (480.0, 240.0),
                    [480, 240],
                    false,
                    false,
                );
                game.handle_option_events(events).unwrap();
            };
            let activate = |game: &mut Game<'_>| click(game, 397.0, 192.0);
            let assert_parent = |game: &Game<'_>| {
                assert_eq!(game.screen, Screen::Options);
                assert_eq!(game.ui.options.page(), Page::Mods);
                assert_eq!(
                    game.ui.options.mods_screen.as_ref().unwrap().selected_mod(),
                    Some((0, "fixture"))
                );
            };

            game.settings.mods.installed = vec![row("fixture")];
            game.list_native_mods();
            game.player = None;
            game.title = Some(game.new_title(game.save.root.clone()));
            game.title_action(&NoLoop, Action::Mods).unwrap();
            click(game, 61.0, 76.0);
            assert_parent(game);
            activate(game);
            assert_eq!(game.mod_screen(), Some("fixture"));
            assert_eq!(
                game.stored_setting("fixture", "show"),
                None,
                "opening saves nothing"
            );
            let mut batch = HudBatch::scratch([480, 240]);
            game.draw_screen_with_post_hook(&mut batch);
            assert_eq!(batch.vertices.len(), 4);
            assert!(
                batch
                    .vertices
                    .iter()
                    .all(|v| v.color == sunrise_client::hud::rgba(250, 32, 224, 255))
            );

            let button = |state| WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
            };
            let input = |game: &mut Game<'_>, event| {
                game.handle_window_event(
                    &NoLoop,
                    window_id,
                    event,
                    crate::game::input_trace::InputOrigin::Agent,
                );
            };
            let move_to = |x, y| WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(x, y),
            };
            input(game, move_to(61.0, 52.0)); // The Minecraft row in the hidden list.
            input(game, button(ElementState::Pressed));
            input(game, move_to(80.0, 60.0));
            input(
                game,
                WindowEvent::MouseWheel {
                    device_id: DeviceId::dummy(),
                    delta: MouseScrollDelta::LineDelta(0.0, 1.0),
                    phase: TouchPhase::Moved,
                },
            );
            input(game, button(ElementState::Released));
            game.replay_line("key w tap", &NoLoop);
            game.replay_line("type x", &NoLoop);
            game.replay_line("key esc tap", &NoLoop);
            assert_parent(game);
            assert!(!game.keys.raw(&KeyCode::KeyW));
            assert_eq!(
                game.stored_setting("fixture", "show").as_deref(),
                Some("true")
            );
            assert_eq!(
                state.lock().unwrap().input,
                [
                    "press", "drag", "scroll", "release", "key down", "key up", "char"
                ]
            );

            state.lock().unwrap().refuse = true;
            activate(game);
            assert_parent(game);
            state.lock().unwrap().refuse = false;
            state.lock().unwrap().veto = true;
            activate(game);
            assert_parent(game);
            let stale = state.lock().unwrap().parent.clone().unwrap();
            state.lock().unwrap().veto = false;

            game.title = None;
            game.player = Some(
                sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::ZERO,
                )
                .unwrap(),
            );
            game.open_options_at(false, Page::Mods);
            click(game, 61.0, 76.0);
            activate(game);
            assert_eq!(game.mod_screen(), Some("fixture"));
            stale.go_back();
            game.settle_config_screen();
            assert_eq!(game.mod_screen(), Some("fixture"));
            state.lock().unwrap().parent.as_ref().unwrap().go_back();
            game.settle_config_screen();
            assert_parent(game);
            assert_eq!(state.lock().unwrap().calls, [false, false, false, true]);

            game.settings.mods.installed = vec![row("generated")];
            game.list_native_mods();
            game.open_options_at(false, Page::Mods);
            click(game, 61.0, 76.0);
            activate(game);
            assert_eq!(game.screen, Screen::Options);
            assert_eq!(game.ui.options.page(), Page::NativeMod(0));
        })
    });
}
