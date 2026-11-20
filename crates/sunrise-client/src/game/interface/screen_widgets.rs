//! Stock screen listener edits are installed once per ScreenBuild. Stock
//! controls retain their normal handlers; added controls use the same widget
//! rendering, controller targets and narrator registration as those handlers.

use super::{Game, Screen, glfw_modifiers};
use mod_api::client::widgets::{
    Outcome, ScreenWidgetAction, ScreenWidgets, Widget, WidgetId, Widgets,
};
use mod_api::content::crash::{ModCall, enter_native_mod};
use std::cell::RefCell;
use sunrise_client::{hud::HudBatch, ui, window::GuiSize};
use winit::keyboard::KeyCode;

pub(super) struct StockControl {
    pub id: u8,
    pub bounds: [f32; 4],
    pub label: mod_api::Text,
    pub active: bool,
}

use sunrise_client::widgets::{Kind, Target};

pub(super) struct BuiltScreenWidgets {
    stock: Vec<StockControl>,
    removed: [u64; 4],
    added: RefCell<Widgets>,
    actions: Vec<(WidgetId, ScreenWidgetAction)>,
    targets: Vec<Target>,
    added_targets: Vec<(Target, WidgetId)>,
}

impl BuiltScreenWidgets {
    fn action(&self, outcome: Outcome) -> Option<&ScreenWidgetAction> {
        let Outcome::Pressed(id) = outcome else {
            return None;
        };
        self.actions
            .iter()
            .find_map(|(widget, action)| (*widget == id).then_some(action))
    }
}

impl Game<'_> {
    pub(super) fn init_stock_screen_widgets(
        &mut self,
        identity: Option<&'static str>,
        gui: GuiSize,
        overlay: bool,
    ) {
        let size = (gui.size[0] as f32, gui.size[1] as f32);
        let title = !overlay && self.screen == Screen::Playing && identity == Some("title");
        let pause = !overlay && self.screen == Screen::Paused && self.pause_menu;
        let stock: Vec<StockControl> = if title {
            let title = self.title.as_mut().expect("title build has a title");
            title.mods = self.settings.mods.installed.len();
            title
                .main_widget_rows(&self.gui.atlas, size)
                .into_iter()
                .map(|(id, bounds, label, active)| StockControl {
                    id,
                    bounds,
                    label: mod_api::Text::literal(label),
                    active,
                })
                .collect()
        } else if pause {
            self.pause_stock_widgets(size)
        } else {
            Vec::new()
        };
        let mut staged = ScreenWidgets::__new(title || pause);
        for widget in &stock {
            staged.__stock(widget.id, widget.bounds, Some(widget.label.clone()));
        }
        let epoch = self.screen_state.build.widget_epoch;
        let mut view = self
            .observed_screen_view()
            .unwrap_or_else(|| self.screen_change_view(self.screen));
        if let Some(id) = identity {
            view.id = id;
        }
        if view.rect == [0.0; 4] {
            view.rect = [0.0, 0.0, size.0, size.1];
        }
        self.native_screen_init_post(&view, &mut staged);
        if self.screen_state.build.widget_epoch != epoch || (!title && !pause) {
            return;
        }
        let mut removed = [0u64; 4];
        for widget in &stock {
            removed[usize::from(widget.id / 64)] |= 1 << (widget.id % 64);
        }
        let mut added = Widgets::new();
        let mut actions = Vec::new();
        let mut added_targets = Vec::new();
        for entry in staged.__finish() {
            if let Some(id) = entry.stock {
                removed[usize::from(id / 64)] &= !(1 << (id % 64));
                continue;
            }
            let (id, active, rect, kind) = match entry.widget {
                Some(Widget::Button(button)) => {
                    let (active, rect) = (button.active, button.rect);
                    (added.add_button(button), active, rect, Kind::Button)
                }
                Some(Widget::TextField(field)) => {
                    let (active, rect) = (field.active, field.rect);
                    (added.add_text_field(field), active, rect, Kind::Field)
                }
                _ => continue,
            };
            if active {
                added_targets.push((Target { rect, kind }, id));
            }
            if let Some(action) = entry.action {
                actions.push((id, action));
            }
        }
        let mut stock = stock;
        stock.retain(|widget| removed[usize::from(widget.id / 64)] & (1 << (widget.id % 64)) == 0);
        let targets = stock
            .iter()
            .filter(|control| control.active)
            .map(|control| Target {
                rect: control.bounds,
                kind: Kind::Button,
            })
            .chain(added_targets.iter().map(|&(target, _)| target))
            .collect::<Vec<_>>();
        if self
            .pad
            .menu
            .focus
            .is_some_and(|old| !targets.iter().any(|target| target.rect == old.rect))
        {
            self.pad_clear_focus();
        }
        if title && let Some(title) = &mut self.title {
            title.set_removed_main_widgets(removed);
        }
        let stock_focus = if title {
            self.title.as_mut().map(|title| &mut title.focused_button)
        } else {
            Some(&mut self.pause_focus)
        };
        if let Some(focus) = stock_focus
            && focus.is_some_and(|id| removed[usize::from(id / 64)] & (1 << (id % 64)) != 0)
        {
            *focus = stock
                .iter()
                .find(|control| control.active)
                .map(|control| control.id);
        }
        let added = RefCell::new(added);
        self.screen_state.build.widgets = Some(BuiltScreenWidgets {
            stock,
            removed,
            added,
            actions,
            targets,
            added_targets,
        });
    }

    fn active_stock_widgets(&self) -> Option<&BuiltScreenWidgets> {
        self.screen_state
            .build
            .widgets
            .as_ref()
            .filter(|_| self.overlay_screen_id().is_none())
    }

    pub(super) fn pause_widget_removed(&self, id: u8) -> bool {
        self.screen_state
            .build
            .widgets
            .as_ref()
            .is_some_and(|widgets| widgets.removed[usize::from(id / 64)] & (1 << (id % 64)) != 0)
    }

    pub(super) fn draw_stock_screen_additions(&self, batch: &mut HudBatch) {
        let Some(build) = self.active_stock_widgets() else {
            return;
        };
        let mut draw = super::host::draw::GameDrawContext::new(self, batch);
        let _ = build
            .added
            .borrow_mut()
            .draw(&mut draw, self, [self.cursor.0, self.cursor.1]);
    }

    fn press_stock_screen_button(&mut self, action: ScreenWidgetAction) {
        let _owner = enter_native_mod(ModCall {
            id: action.owner,
            hook: "screen_widget_press",
            part: None,
        });
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (action.press)(self, &action.id)
        }));
    }

    pub(super) fn stock_widget_mouse(&mut self, pressed: bool) -> bool {
        let Some(build) = self.active_stock_widgets() else {
            return false;
        };
        let outcome = if pressed {
            if build.stock.iter().any(|s| {
                let [x, y, w, h] = s.bounds;
                s.active && ui::hovered(self.cursor, x, y, w, h)
            }) {
                build.added.borrow_mut().set_focus(self, None);
                return false;
            }
            build.added.borrow_mut().mouse_pressed(
                self,
                [self.cursor.0, self.cursor.1],
                0,
                glfw_modifiers(self.modifiers),
            )
        } else {
            build.added.borrow_mut().mouse_released(0)
        };
        if pressed && outcome.handled() {
            let action = build.action(outcome).cloned();
            if self.screen == Screen::Paused {
                self.pause_focus = None;
            } else if let Some(title) = &mut self.title {
                title.focused_button = None;
            }
            if let Some(action) = action {
                self.press_stock_screen_button(action);
            }
        }
        outcome.handled()
    }

    pub(super) fn stock_widget_drag(&self) -> bool {
        self.active_stock_widgets().is_some_and(|build| {
            build
                .added
                .borrow_mut()
                .mouse_dragged(self, [self.cursor.0, self.cursor.1], 0)
                .handled()
        })
    }

    pub(super) fn stock_widget_text_focused(&self) -> bool {
        self.active_stock_widgets().is_some_and(|build| {
            let added = build.added.borrow();
            added
                .focused()
                .is_some_and(|id| added.text_field(id).is_some())
        })
    }

    pub(super) fn stock_widget_type(&self, text: &str) -> bool {
        let Some(build) = self.active_stock_widgets() else {
            return false;
        };
        let mut added = build.added.borrow_mut();
        let Some(id) = added.focused() else {
            return false;
        };
        if added.text_field(id).is_none() {
            return false;
        }
        for c in text.chars() {
            added.char_typed(self, c);
        }
        true
    }

    pub(super) fn clear_stock_widget_focus(&self) {
        if let Some(build) = self.active_stock_widgets() {
            build.added.borrow_mut().set_focus(self, None);
        }
    }

    pub(super) fn stock_widget_targets(&self) -> Option<&[Target]> {
        Some(&self.active_stock_widgets()?.targets)
    }

    pub(super) fn focus_stock_widget_field(&self, target: Target) -> bool {
        let Some(build) = self.active_stock_widgets() else {
            return false;
        };
        let Some((_, id)) = build.added_targets.iter().find(|(at, _)| *at == target) else {
            return false;
        };
        build.added.borrow_mut().set_focus(self, Some(*id));
        true
    }

    pub(super) fn press_stock_widget_target(&mut self, target: Target) -> bool {
        let Some(build) = self.active_stock_widgets() else {
            return false;
        };
        let Some(&(_, id)) = build
            .added_targets
            .iter()
            .find(|(candidate, _)| *candidate == target)
        else {
            return false;
        };
        if target.kind != Kind::Button {
            return false;
        }
        // Arrow/Tab focus already identifies the target. Don't route the
        // activation through a pointer hit test that may find a different widget.
        let mut added = build.added.borrow_mut();
        added.set_focus(self, Some(id));
        let outcome = added.key_pressed(self, "key.keyboard.enter", 0);
        drop(added);
        if let Some(action) = build.action(outcome).cloned() {
            self.press_stock_screen_button(action);
        }
        outcome.handled()
    }

    pub(super) fn stock_widget_key(&mut self, key: KeyCode) -> bool {
        if key == KeyCode::Tab {
            return false;
        }
        let Some(name) = sunrise_client::keybinds::key_name(key) else {
            return false;
        };
        let Some(build) = self.active_stock_widgets() else {
            return false;
        };
        let outcome =
            build
                .added
                .borrow_mut()
                .key_pressed(self, name, glfw_modifiers(self.modifiers));
        if let Some(action) = build.action(outcome).cloned() {
            self.press_stock_screen_button(action);
        }
        outcome.handled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{event_loop::EventLoopCtl, native_entity_refs::tests::with_client_game};
    use mod_api::client::widgets::{Button, ScreenWidgetError, ScreenWidgetRef};
    use mod_api::{ClientHost, Hooks, ModEntry, ScreenInitPostEvent, Text, testing::with_mods};
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};
    use sunrise_client::{gamepad, widgets};

    const ADDED: [f32; 4] = [9.0, 17.0, 60.0, 20.0];
    thread_local! {
        static PRESSES: RefCell<Vec<(&'static str, String)>> = const { RefCell::new(Vec::new()) };
    }

    fn press(_: &mut dyn ClientHost, action: &str) {
        let owner = mod_api::content::crash::current_native_mod().unwrap();
        PRESSES.with_borrow_mut(|p| p.push((owner, action.to_owned())));
    }

    struct NoLoop;
    impl EventLoopCtl for NoLoop {
        fn exit(&self) {}
        fn set_control_flow(&self, _: winit::event_loop::ControlFlow) {}
    }

    struct Edits(Arc<Mutex<Vec<ScreenWidgetRef>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Edits {
        fn screen_init_post(&self, _: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_>) {
            if event.screen.id != "paused" {
                return;
            }
            let mut refs = self.0.lock().unwrap();
            if let Some(&old) = refs.last() {
                assert_eq!(
                    event.widgets.remove_listener(old),
                    Err(ScreenWidgetError::UnknownListener)
                );
            }
            let options = event
                .widgets
                .listeners()
                .find(|view| view.label == Some(&Text::translate("menu.options", vec![])))
                .unwrap();
            event.widgets.remove_listener(options.listener).unwrap();
            let widget = Widget::Button(Button::new(ADDED, Text::literal("Example action")));
            refs.push(
                event
                    .widgets
                    .add_listener(widget, Some(("settings", press)))
                    .unwrap(),
            );
        }
    }

    struct Refused;
    #[mod_api::__subscriptions]
    impl Hooks for Refused {
        fn screen_init_post(&self, _: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_>) {
            if event.screen.id != "paused" {
                return;
            }
            let widget = Widget::Button(Button::new(
                [5.0, 60.0, 60.0, 20.0],
                Text::literal("Refused"),
            ));
            event
                .widgets
                .add_listener(widget, Some(("refused", press)))
                .unwrap();
            panic!("reject this listener's edits");
        }
    }

    struct Later;
    #[mod_api::__subscriptions]
    impl Hooks for Later {
        fn screen_init_post(&self, _: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_>) {
            if event.screen.id != "paused" {
                return;
            }
            assert_eq!(event.widgets.listeners().last().unwrap().bounds, ADDED);
            assert!(
                !event
                    .widgets
                    .listeners()
                    .any(|view| view.label == Some(&Text::literal("Refused")))
            );
            let widget = Widget::Button(Button::new(
                [9.0, 40.0, 60.0, 20.0],
                Text::literal("Second action"),
            ));
            event
                .widgets
                .add_listener(widget, Some(("settings", press)))
                .unwrap();
        }
    }

    #[test]
    #[ignore = "daily"]
    fn pause_listeners_are_ordered_and_pressed_only_by_the_owner() {
        let refs = Arc::new(Mutex::new(Vec::new()));
        let mods = vec![
            ModEntry::new("first", Box::new(Edits(refs.clone()))),
            ModEntry::new("refused", Box::new(Refused)),
            ModEntry::new("later", Box::new(Later)),
        ]
        .into();
        with_mods(mods, || {
            with_client_game(|game| {
                game.title = None;
                game.settings.gui_scale = 3;
                game.pause_menu = true;
                game.pause_focus = Some(1); // Options, removed by the first listener.
                game.set_screen(Screen::Paused);
                assert_eq!(game.pause_focus, Some(0)); // First remaining stock control.
                assert!(game.pause_widget_removed(1));
                let targets = game.stock_widget_targets().unwrap();
                let second = targets[targets.len() - 1];
                let first = targets[targets.len() - 2];
                let last_stock = targets[targets.len() - 3];
                assert_eq!(first.rect, ADDED);
                assert_eq!(second.rect, [9.0, 40.0, 60.0, 20.0]);
                game.cursor = (20.0, 20.0);
                assert!(game.stock_widget_mouse(true));
                assert!(game.stock_widget_mouse(false));
                game.clear_stock_widget_focus();
                game.pad.menu.focus = Some(last_stock);
                game.pad.menu.keyboard = true;
                assert!(game.key_focus_key(&NoLoop, KeyCode::Tab));
                assert_eq!(game.pad.menu.focus, Some(first));
                assert!(game.key_focus_key(&NoLoop, KeyCode::Tab));
                assert_eq!(game.pad.menu.focus, Some(second));
                assert!(game.key_focus_key(&NoLoop, KeyCode::Enter));
                game.pad.menu.focus = Some(first);
                game.pad_menu_button(&NoLoop, gamepad::Button::South, true)
                    .unwrap();
                game.pad_menu_button(&NoLoop, gamepad::Button::South, false)
                    .unwrap();
                assert_eq!(
                    PRESSES.with_borrow_mut(std::mem::take),
                    [
                        ("first", "settings".to_owned()),
                        ("later", "settings".to_owned()),
                        ("first", "settings".to_owned())
                    ]
                );

                widgets::set_narrating(true);
                let mut batch = HudBatch::new([1280, 720], game.settings.gui_scale_inputs());
                game.draw_pause_menu(&mut batch);
                widgets::end_frame(1);
                let said = widgets::take_last_narration();
                widgets::set_narrating(false);
                let buttons: Vec<_> = said
                    .items
                    .iter()
                    .filter_map(|item| match &item.what {
                        widgets::What::Button { message, .. } => Some(message.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(buttons.ends_with(&["Example action", "Second action"]));
                assert!(
                    !said
                        .items
                        .iter()
                        .any(|item| item.rect == [111.0, 140.0, 98.0, 20.0])
                );

                game.build_screen();
                assert_eq!(refs.lock().unwrap().len(), 1);
                game.pad.menu.focus = Some(first);
                game.pad_menu_button(&NoLoop, gamepad::Button::South, true)
                    .unwrap();
                game.settings.gui_scale = 1;
                game.build_screen();
                game.pad_menu_button(&NoLoop, gamepad::Button::South, false)
                    .unwrap();
                assert!(PRESSES.with_borrow(Vec::is_empty));
                assert_eq!(refs.lock().unwrap().len(), 2);
                assert_eq!(
                    game.stock_widget_targets()
                        .unwrap()
                        .iter()
                        .filter(|target| target.rect == ADDED)
                        .count(),
                    1
                );
                game.set_screen(Screen::Playing);
                assert!(game.stock_widget_targets().is_none());
                assert!(!game.stock_widget_mouse(true));
            })
        });
    }

    #[test]
    #[ignore = "daily"]
    fn no_post_listener_preserves_stock_widgets() {
        with_mods(Vec::new().into(), || {
            with_client_game(|game| {
                game.title = None;
                game.set_screen(Screen::Paused);
                assert!(game.stock_widget_targets().is_none());
                assert!(!game.pause_widget_removed(1));
                assert!(!game.stock_widget_mouse(true));
            })
        });
    }

    struct TitleField;
    #[mod_api::__subscriptions]
    impl Hooks for TitleField {
        fn screen_init_post(&self, _: &dyn ClientHost, event: &mut ScreenInitPostEvent<'_>) {
            if event.screen.id != "title" {
                return;
            }
            let standard_options = event
                .widgets
                .listeners()
                .find(|row| row.bounds == [113.0, 204.0, 98.0, 20.0])
                .unwrap();
            event
                .widgets
                .remove_listener(standard_options.listener)
                .unwrap();
            let mut field = mod_api::client::widgets::TextField::new(ADDED, 32);
            field.message = "Example query".into();
            event
                .widgets
                .add_listener(Widget::TextField(field), None)
                .unwrap();
        }
    }

    #[test]
    #[ignore = "daily"]
    fn title_post_mounts_an_edit_box_and_a_modal_does_not_reinitialize_it() {
        let mods = vec![ModEntry::new("title_field", Box::new(TitleField))].into();
        with_mods(mods, || {
            with_client_game(|game| {
                game.settings.gui_scale = 3;
                let saves = tempfile::tempdir().unwrap();
                game.show_title(game.new_title(saves.path().to_owned()));
                assert_eq!(
                    game.stock_widget_targets().unwrap().last().unwrap().kind,
                    Kind::Field
                );
                assert!(
                    !game
                        .stock_widget_targets()
                        .unwrap()
                        .iter()
                        .any(|row| row.rect == [113.0, 204.0, 98.0, 20.0])
                );
                game.cursor = (20.0, 20.0);
                assert!(game.stock_widget_mouse(true));
                assert!(game.stock_widget_text_focused());
                assert!(game.stock_widget_type("abcd"));
                assert!(game.stock_widget_key(KeyCode::Backspace));
                // A sign editor is an overlay, not a new title build.
                game.open_sign_editor((0, 64, 0), true);
                assert!(game.stock_widget_targets().is_none());
                game.sign_edit = None;
                game.build_screen();
                let added = game.active_stock_widgets().unwrap().added.borrow();
                assert_eq!(
                    added.text_field(added.focused().unwrap()).unwrap().value(),
                    "abc"
                );
            })
        });
    }
}
