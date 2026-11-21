//! HUD slots in a real client: placement, scoped wrapping and replacement, and layer cancellation.
use super::{
    execution_role::ExecutionRole,
    mods,
    native_entity_refs::tests::{run_fresh_cases, with_client_game},
};
use mod_api::{
    ClientHost, Flow, GuiLayerPlacement, GuiLayerPostEvent, GuiLayerPreEvent, Hooks, ModEntry,
    PlayerHeartTypeEvent, RegisterGuiLayersEvent, RegistrationContext, RenderGuiPreEvent,
    client::hud_layout::GuiDraw, content::crash::current_native_mod,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use sunrise_client::{
    camera::CameraType,
    chunk::Chunk,
    gui_layers::VanillaGuiLayer,
    hud::HudBatch,
    motion::{MotionWorld, PlayerMotion, WATER, dry_side},
};
use sunrise_core::geometry::Vec3;

#[path = "gui_layer_tests/hidden_panic.rs"]
mod hidden_panic;

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log(entry: impl Into<String>) {
    LOG.lock().unwrap().push(entry.into());
}

fn assert_owner(id: &'static str) {
    assert_eq!(current_native_mod(), Some(id));
}

struct Compass;
#[mod_api::__subscriptions]
impl Hooks for Compass {
    fn register_gui_layers(&self, _: &RegistrationContext, event: &mut RegisterGuiLayersEvent<'_>) {
        use GuiLayerPlacement::*;
        // The layer namespace is deliberately different from the registering mod.
        for (id, placement) in [
            (
                "hud_test:under_crosshair",
                Below("minecraft:crosshair".into()),
            ),
            (
                "hud_test:over_crosshair",
                Above("minecraft:crosshair".into()),
            ),
            ("hud_test:bar", Above("minecraft:hotbar".into())),
            (
                "hud_test:above_health",
                Above("minecraft:player_health".into()),
            ),
            (
                "hud_test:below_contextual",
                Below("minecraft:contextual_info_bar".into()),
            ),
            ("hud_test:top", AboveAll),
        ] {
            assert!(event.layers.register(
                id.into(),
                placement,
                Box::new(move |_, draw| {
                    assert_owner("compass");
                    log(format!("draw {id}"));
                    if id == "hud_test:under_crosshair" {
                        draw.fill([0.0, 0.0, 4.0, 4.0], 0xFF00_FF00);
                    }
                }),
            ));
        }
    }
    fn gui_layer_pre(&self, _: &dyn ClientHost, event: &mut GuiLayerPreEvent<'_>) -> Flow {
        log(format!("pre {}", event.layer));
        Flow::Continue
    }
    fn gui_layer_post(&self, _: &dyn ClientHost, event: &mut GuiLayerPostEvent<'_>) {
        log(format!("post {}", event.layer));
    }
}

struct Inner;
#[mod_api::__subscriptions]
impl Hooks for Inner {
    fn register_gui_layers(&self, _: &RegistrationContext, event: &mut RegisterGuiLayersEvent<'_>) {
        assert!(event.layers.wrap_layer(
            "hud_test:under_crosshair",
            Box::new(|ctx| {
                assert_owner("inner");
                log("inner before");
                ctx.draw_previous();
                assert_owner("inner");
                log("inner after");
            }),
        ));
        assert!(event.layers.wrap_layer(
            "minecraft:hotbar",
            Box::new(|ctx| {
                log("hotbar wrapper before");
                ctx.draw_previous();
                log("hotbar wrapper after");
            }),
        ));
        for (id, label) in [
            ("minecraft:crosshair", "crosshair wrapper"),
            ("minecraft:contextual_info_bar", "contextual wrapper"),
        ] {
            assert!(event.layers.wrap_layer(
                id,
                Box::new(move |ctx| {
                    log(label);
                    ctx.draw_previous();
                })
            ));
        }
    }
}

struct Editor;
#[mod_api::__subscriptions]
impl Hooks for Editor {
    fn register_gui_layers(&self, _: &RegistrationContext, event: &mut RegisterGuiLayersEvent<'_>) {
        assert!(
            !event
                .layers
                .wrap_layer("hud_test:missing", Box::new(|_| {}))
        );
        assert!(
            !event
                .layers
                .replace_layer("hud_test:missing", Box::new(|_, _| {}))
        );
        assert!(event.layers.wrap_layer(
            "hud_test:under_crosshair",
            Box::new(|ctx| {
                assert_owner("editor");
                log("outer before");
                ctx.with_draw(&mut |draw| draw.fill([0.0, 0.0, 4.0, 4.0], 0xFFFF_0000));
                ctx.draw_previous();
                assert_owner("editor");
                ctx.with_draw(&mut |draw| draw.fill([0.0, 0.0, 4.0, 4.0], 0xFF00_00FF));
                log("outer after");
            }),
        ));
        assert!(event.layers.replace_layer(
            "minecraft:contextual_info_bar",
            Box::new(|_, _| {
                assert_owner("editor");
                log("contextual replacement");
            }),
        ));
    }
    fn gui_layer_pre(&self, _: &dyn ClientHost, event: &mut GuiLayerPreEvent<'_>) -> Flow {
        if matches!(event.layer, "hud_test:bar" | "minecraft:effects") {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

fn take() -> Vec<String> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

const PLAYER_HEALTH: &str = "minecraft:player_health";
const AIR_LEVEL: &str = "minecraft:air_level";
const BUBBLE_POP: &str = "ui.hud.bubble_pop";
// Equal RGB channels make the marker's packed vertex colour independent of channel order.
const HEALTH_MARKER: u32 = 0xFF2D_2D2D;

#[derive(Default)]
struct HealthTrace {
    cancel_gui: bool,
    cancel_health: bool,
    cancel_air: bool,
    calls: Vec<&'static str>,
}

struct HealthLayer {
    replace: bool,
    trace: Arc<Mutex<HealthTrace>>,
}

#[mod_api::__subscriptions]
impl Hooks for HealthLayer {
    fn register_gui_layers(&self, _: &RegistrationContext, event: &mut RegisterGuiLayersEvent<'_>) {
        if self.replace {
            let trace = self.trace.clone();
            assert!(event.layers.replace_layer(
                PLAYER_HEALTH,
                Box::new(move |_, draw| {
                    trace.lock().unwrap().calls.push("replacement");
                    draw.gui_paint(&[GuiDraw::Fill {
                        x0: 80,
                        y0: 100,
                        x1: 150,
                        y1: 104,
                        color: HEALTH_MARKER,
                    }])
                    .unwrap();
                }),
            ));
        }
        let trace = self.trace.clone();
        assert!(event.layers.wrap_layer(
            AIR_LEVEL,
            Box::new(move |ctx| {
                trace.lock().unwrap().calls.push("air wrapper");
                ctx.draw_previous();
            }),
        ));
    }

    fn render_gui_pre(&self, _: &dyn ClientHost, _: &mut RenderGuiPreEvent) -> Flow {
        if self.trace.lock().unwrap().cancel_gui {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn gui_layer_pre(&self, _: &dyn ClientHost, event: &mut GuiLayerPreEvent<'_>) -> Flow {
        let mut trace = self.trace.lock().unwrap();
        match event.layer {
            PLAYER_HEALTH => {
                trace.calls.push("health pre");
                if trace.cancel_health {
                    return Flow::Cancel;
                }
            }
            "minecraft:armor_level" => trace.calls.push("armor pre"),
            "minecraft:food_level" => trace.calls.push("food pre"),
            "minecraft:vehicle_health" => trace.calls.push("vehicle pre"),
            AIR_LEVEL => {
                trace.calls.push("air pre");
                if trace.cancel_air {
                    return Flow::Cancel;
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    fn player_heart_type(&self, _: &mut dyn ClientHost, _: &mut PlayerHeartTypeEvent) {
        self.trace.lock().unwrap().calls.push("hearts");
    }

    fn gui_layer_post(&self, _: &dyn ClientHost, event: &mut GuiLayerPostEvent<'_>) {
        let call = match event.layer {
            PLAYER_HEALTH => "health post",
            "minecraft:armor_level" => "armor post",
            "minecraft:food_level" => "food post",
            "minecraft:vehicle_health" => "vehicle post",
            AIR_LEVEL => "air post",
            _ => return,
        };
        self.trace.lock().unwrap().calls.push(call);
    }
}

#[test]
#[ignore = "daily"]
fn health_extraction_runs_only_in_the_vanilla_layer_body() {
    const CASE: &str = "SUNRISE_HEALTH_LAYER_FIXTURE";
    if run_fresh_cases(
        "game::gui_layer_tests::health_extraction_runs_only_in_the_vanilla_layer_body",
        CASE,
        &["vanilla", "replace"],
    ) {
        return;
    }
    let replace = std::env::var(CASE).unwrap() == "replace";
    let trace = Arc::new(Mutex::new(HealthTrace::default()));
    with_client_game(|game| {
        mod_platform::__private::install(
            vec![ModEntry::new(
                "health-test",
                Box::new(HealthLayer {
                    replace,
                    trace: trace.clone(),
                }),
            )]
            .into(),
        );
        mods::register_gui_layers(ExecutionRole::Client);
        game.title = None;
        game.screen = super::Screen::Playing;
        game.ui.hide_gui = false;
        game.set_mode_id(0);
        let mut player = PlayerMotion::new(game.motion, Vec3::new(2.0, 64.0, 2.0)).unwrap();
        player.health.set_health(12.0, false).unwrap();
        player
            .set_modifier(
                "minecraft:armor",
                "health-test:armor",
                2.0,
                "add_value",
                true,
            )
            .unwrap();
        // At 181/300, Hud.extractAirBubbles has six full bubbles and one popping.
        // Above water the same depleted air instead resets the pop latch, with no sound.
        player.air = 181;
        game.player = Some(player);
        for (under_water, creative, hidden) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            game.set_mode_id(if creative { 1 } else { 0 });
            game.ui.hide_gui = hidden;
            if under_water && !game.player.as_ref().unwrap().is_eye_in_fluid(WATER) {
                let water = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:water")
                    .unwrap();
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                for y in 64..=66 {
                    game.world.set(2, y, 2, water).unwrap();
                }
                let player = game.player.as_mut().unwrap();
                MotionWorld {
                    entities: &[],
                    player,
                    scene: &mut game.world,
                    assets: &game.pack.assets,
                    collision_binding: game.collision,
                    resources: game.motion,
                    health: &game.health_resources,
                    tag_items: None,
                    loop_context: &game.loop_context,
                    side: dry_side(),
                    damage_hooks: &game.damage_hooks,
                    block_registries: None,
                }
                .update_fluids()
                .unwrap();
                assert!(player.is_eye_in_fluid(WATER));
            }
            let vanilla_eligible = !creative && !hidden;
            let vetoes: &[(bool, bool)] = if !vanilla_eligible {
                &[(false, false)]
            } else {
                &[(true, false), (true, true), (false, false), (false, true)]
            };
            for &(cancel_health, cancel_air) in vetoes {
                game.health_hud.last_health = 17;
                game.health_hud.last_bubble_pop_sound = 3;
                game.audio.heard.remove(BUBBLE_POP);
                *trace.lock().unwrap() = HealthTrace {
                    cancel_health,
                    cancel_air,
                    ..HealthTrace::default()
                };
                let batch = game.hud_layers([640, 480]);
                let calls = std::mem::take(&mut trace.lock().unwrap().calls);
                let mut expected = vec!["health pre"];
                if !cancel_health {
                    if replace {
                        expected.push("replacement");
                    } else if vanilla_eligible {
                        expected.push("hearts");
                    }
                    expected.push("health post");
                }
                expected.extend([
                    "armor pre",
                    "armor post",
                    "food pre",
                    "food post",
                    "vehicle pre",
                    "vehicle post",
                    "air pre",
                ]);
                if !cancel_air {
                    expected.extend(["air wrapper", "air post"]);
                }
                assert_eq!(calls, expected);
                let painted_replacement = !cancel_health && replace;
                assert_eq!(
                    batch.vertices.iter().any(|v| v.color == HEALTH_MARKER),
                    painted_replacement,
                );
                if painted_replacement {
                    assert_eq!(game.ui.room.bottom(), 100.0);
                } else if !vanilla_eligible {
                    assert_eq!(
                        game.ui.room.bottom(),
                        batch.height,
                        "no vanilla status paint"
                    );
                } else {
                    assert!(game.ui.room.bottom() > 100.0);
                }
                assert_eq!(
                    game.health_hud.last_health,
                    if cancel_health || replace || !vanilla_eligible {
                        17
                    } else {
                        12
                    }
                );
                let draws_air = !cancel_air && vanilla_eligible;
                let pops = draws_air && under_water;
                assert_eq!(
                    game.health_hud.last_bubble_pop_sound,
                    if !draws_air {
                        3
                    } else if pops {
                        7
                    } else {
                        0
                    }
                );
                // The sound is local-only; `heard` counts it even with the device disabled.
                assert_eq!(game.audio.heard.get(BUBBLE_POP).copied(), pops.then_some(1));
            }
        }
        // The last frame hid the vanilla HUD. The global veto still applies to its slots.
        *trace.lock().unwrap() = HealthTrace {
            cancel_gui: true,
            ..HealthTrace::default()
        };
        let batch = game.hud_layers([640, 480]);
        assert!(trace.lock().unwrap().calls.is_empty());
        assert!(!batch.vertices.iter().any(|v| v.color == HEALTH_MARKER));
    });
}

#[test]
#[ignore = "daily"]
fn mod_layers_draw_in_their_place() {
    // The layer order is kept once per process.
    if run_fresh_cases(
        "game::gui_layer_tests::mod_layers_draw_in_their_place",
        "SUNRISE_GUI_LAYER_FIXTURE",
        &["run"],
    ) {
        return;
    }
    with_client_game(|game| {
        mod_platform::__private::install(
            vec![
                ModEntry::new("compass", Box::new(Compass)),
                ModEntry::new("inner", Box::new(Inner)),
                ModEntry::new("editor", Box::new(Editor)),
            ]
            .into(),
        );
        mods::register_gui_layers(ExecutionRole::Client);
        let mut batch = HudBatch::new([320, 240], game.settings.gui_scale_inputs());

        // A wrapper around a no-op vanilla body still draws between its neighbors.
        game.hud_layer_next = Some(0);
        game.gui_layer(VanillaGuiLayer::Crosshair, &mut batch, &mut |_, _| {});
        for (layer, paint) in [
            (VanillaGuiLayer::Hotbar, "hotbar paint"),
            (VanillaGuiLayer::Effects, "effects paint"),
            (VanillaGuiLayer::PlayerHealth, "health paint"),
            (VanillaGuiLayer::ContextualInfoBar, "contextual paint"),
            (VanillaGuiLayer::Chat, "chat paint"),
        ] {
            game.gui_layer(layer, &mut batch, &mut |_, _| log(paint));
        }
        game.draw_mod_gui_layers(None, &mut batch);
        game.hud_layer_next = None;
        assert_eq!(current_native_mod(), None);
        assert_eq!(
            take(),
            [
                "pre hud_test:under_crosshair",
                "outer before",
                "inner before",
                "draw hud_test:under_crosshair",
                "inner after",
                "outer after",
                "post hud_test:under_crosshair",
                "pre minecraft:crosshair",
                "crosshair wrapper",
                "post minecraft:crosshair",
                "pre hud_test:over_crosshair",
                "draw hud_test:over_crosshair",
                "post hud_test:over_crosshair",
                "pre minecraft:hotbar",
                "hotbar wrapper before",
                "hotbar paint",
                "hotbar wrapper after",
                "post minecraft:hotbar",
                "pre hud_test:bar",
                "pre minecraft:effects",
                "pre minecraft:player_health",
                "health paint",
                "post minecraft:player_health",
                "pre hud_test:above_health",
                "draw hud_test:above_health",
                "post hud_test:above_health",
                "pre hud_test:below_contextual",
                "draw hud_test:below_contextual",
                "post hud_test:below_contextual",
                "pre minecraft:contextual_info_bar",
                "contextual replacement",
                "post minecraft:contextual_info_bar",
                "pre minecraft:chat",
                "chat paint",
                "post minecraft:chat",
                "pre hud_test:top",
                "draw hud_test:top",
                "post hud_test:top",
            ]
        );
        // The outer wrapper and the underlying painter append into the same batch in order.
        let mut colors: Vec<_> = batch
            .vertices
            .iter()
            .map(|v| v.color.to_le_bytes())
            .collect();
        colors.dedup();
        assert_eq!(
            colors,
            [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        );
    });
}

#[test]
#[ignore = "daily"]
fn skipped_vanilla_hud_bodies_still_dispatch_their_slots() {
    if run_fresh_cases(
        "game::gui_layer_tests::skipped_vanilla_hud_bodies_still_dispatch_their_slots",
        "SUNRISE_HUD_ELIGIBILITY_FIXTURE",
        &["run"],
    ) {
        return;
    }

    const CROSSHAIR: &str = "minecraft:crosshair";
    const CHAT: &str = "minecraft:chat";
    const AFTER_CROSSHAIR: &str = "hud_test:after_crosshair";
    const SLEEP: &str = "minecraft:sleep_overlay";
    const MARKER: u32 = 0xFF37_3737;

    struct Slots(Arc<AtomicBool>);

    #[mod_api::__subscriptions]
    impl Hooks for Slots {
        fn register_gui_layers(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterGuiLayersEvent<'_>,
        ) {
            assert!(event.layers.replace_layer(
                CROSSHAIR,
                Box::new(|_, draw| {
                    log("replacement crosshair");
                    draw.fill([20.0, 20.0, 8.0, 4.0], MARKER);
                }),
            ));
            assert!(event.layers.register(
                AFTER_CROSSHAIR.into(),
                GuiLayerPlacement::Above(CROSSHAIR.into()),
                Box::new(|_, _| log("inserted layer")),
            ));
        }

        fn render_gui_pre(&self, _: &dyn ClientHost, _: &mut RenderGuiPreEvent) -> Flow {
            if self.0.load(Ordering::Relaxed) {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }

        fn gui_layer_pre(&self, _: &dyn ClientHost, event: &mut GuiLayerPreEvent<'_>) -> Flow {
            if matches!(event.layer, CROSSHAIR | AFTER_CROSSHAIR | SLEEP | CHAT) {
                log(format!("pre {}", event.layer));
            }
            Flow::Continue
        }

        fn gui_layer_post(&self, _: &dyn ClientHost, event: &mut GuiLayerPostEvent<'_>) {
            if matches!(event.layer, CROSSHAIR | AFTER_CROSSHAIR | SLEEP | CHAT) {
                log(format!("post {}", event.layer));
            }
        }
    }

    let veto = Arc::new(AtomicBool::new(false));
    mod_platform::testing::with_mods(
        vec![ModEntry::new("test-slots", Box::new(Slots(veto.clone())))].into(),
        || {
            with_client_game(|game| {
                mods::register_gui_layers(ExecutionRole::Client);
                game.title = None;
                game.set_mode_id(0);
                game.player =
                    Some(PlayerMotion::new(game.motion, Vec3::new(2.0, 64.0, 2.0)).unwrap());
                game.camera_type = CameraType::ThirdPersonBack;
                for (hidden, screen) in [
                    (false, super::Screen::Playing),
                    (true, super::Screen::Playing),
                    (false, super::Screen::Chat),
                ] {
                    game.ui.hide_gui = hidden;
                    game.screen = screen;
                    let batch = game.hud_layers([640, 480]);
                    assert!(batch.vertices.iter().any(|v| v.color == MARKER));
                    assert_eq!(
                        take(),
                        [
                            "pre minecraft:crosshair",
                            "replacement crosshair",
                            "post minecraft:crosshair",
                            "pre hud_test:after_crosshair",
                            "inserted layer",
                            "post hud_test:after_crosshair",
                            "pre minecraft:sleep_overlay",
                            "post minecraft:sleep_overlay",
                            "pre minecraft:chat",
                            "post minecraft:chat",
                        ],
                    );
                }
                // A whole-HUD veto differs from F1: even the sleep and background chat slots stop.
                game.ui.hide_gui = true;
                veto.store(true, Ordering::Relaxed);
                let batch = game.hud_layers([640, 480]);
                assert!(!batch.vertices.iter().any(|v| v.color == MARKER));
                assert!(take().is_empty());
            })
        },
    );
}

#[test]
#[ignore = "daily"]
fn a_refused_gui_layer_does_not_leave_partial_paint() {
    crate::game::in_own_process(
        "game::gui_layer_tests::a_refused_gui_layer_does_not_leave_partial_paint",
        || {
            struct RefusedLayer;
            #[mod_api::__subscriptions]
            impl Hooks for RefusedLayer {
                fn register_gui_layers(
                    &self,
                    _: &RegistrationContext,
                    event: &mut RegisterGuiLayersEvent<'_>,
                ) {
                    assert!(event.layers.register(
                        "hud_test:refused".into(),
                        GuiLayerPlacement::AboveAll,
                        Box::new(|_, draw| {
                            draw.fill([0.0, 0.0, 4.0, 4.0], 0xFFFF_0000);
                            log("refused");
                            mod_api::__private::refuse(
                                "test layer refusal",
                                mod_api::__private::ModPhase::Setup,
                            );
                        }),
                    ));
                }
            }
            with_client_game(|game| {
                mod_platform::__private::install(
                    vec![ModEntry::new("refused_layer", Box::new(RefusedLayer))].into(),
                );
                mods::register_gui_layers(ExecutionRole::Client);
                for _ in 0..2 {
                    let mut batch = HudBatch::scratch([320, 240]);
                    game.hud_layer_next = Some(0);
                    game.draw_mod_gui_layers(None, &mut batch);
                    assert!(batch.vertices.is_empty());
                }
                assert_eq!(take(), ["refused", "refused"]);
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn a_gui_wrapper_panic_is_contained_but_delegated_engine_paint_is_fatal() {
    use sunrise_client::hud::{Blend, rgba};

    let test = "game::gui_layer_tests::a_gui_wrapper_panic_is_contained_but_delegated_engine_paint_is_fatal";
    crate::game::in_own_process(test, || {
        struct Wrapper(&'static str, bool);
        #[mod_api::__subscriptions]
        impl Hooks for Wrapper {
            fn register_gui_layers(
                &self,
                _: &RegistrationContext,
                event: &mut RegisterGuiLayersEvent<'_>,
            ) {
                let Self(id, broken) = *self;
                assert!(event.layers.wrap_layer(
                    "minecraft:hotbar",
                    Box::new(move |ctx| {
                        assert_owner(id);
                        log(id);
                        if broken {
                            ctx.with_draw(&mut |draw| draw.fill([0.0, 0.0, 4.0, 4.0], 0xFFFF_0000));
                            ctx.draw_previous();
                            ctx.with_draw(&mut |draw| draw.fill([0.0, 0.0, 4.0, 4.0], 0xFF00_00FF));
                            ctx.draw_previous();
                            ctx.with_draw(&mut |draw| draw.fill([0.0, 0.0, 4.0, 4.0], 0xFFFF_0000));
                            panic!("broken wrapper");
                        }
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            ctx.draw_previous();
                        }))
                        .is_err()
                        {
                            log(format!("caught by {id}"));
                        }
                        assert_owner(id);
                    }),
                ));
            }
        }
        with_client_game(|game| {
            mod_platform::__private::install(
                vec![
                    ModEntry::new("inner", Box::new(Wrapper("inner", false))),
                    ModEntry::new("broken", Box::new(Wrapper("broken", true))),
                    ModEntry::new("outer", Box::new(Wrapper("outer", false))),
                ]
                .into(),
            );
            mods::register_gui_layers(ExecutionRole::Client);
            let mut batch = HudBatch::scratch([320, 240]);
            let white = rgba(255, 255, 255, 255);
            let green = rgba(0, 255, 0, 255);
            let yellow = rgba(255, 255, 0, 255);
            let mut paint_calls = 0;
            batch.fill(&game.gui.atlas, 0.0, 0.0, 4.0, 4.0, white);
            game.gui_layer(VanillaGuiLayer::Hotbar, &mut batch, &mut |g, batch| {
                log("vanilla paint");
                let start = batch.vertices.len();
                let color = [green, yellow][paint_calls];
                paint_calls += 1;
                batch.fill(&g.gui.atlas, 0.0, 0.0, 4.0, 4.0, color);
                batch.invert_since(start);
            });
            assert_eq!(
                take(),
                [
                    "outer",
                    "broken",
                    "inner",
                    "vanilla paint",
                    "inner",
                    "vanilla paint"
                ]
            );
            assert_eq!(
                batch.vertices.iter().map(|v| v.color).collect::<Vec<_>>(),
                [[white; 6], [green; 6], [yellow; 6]].concat()
            );
            assert_eq!(
                batch.blends,
                [(6..12, Blend::Invert), (12..18, Blend::Invert)]
            );

            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                game.gui_layer(VanillaGuiLayer::Hotbar, &mut batch, &mut |_, _| {
                    assert_eq!(current_native_mod(), None);
                    assert!(!mod_api::__private::game::retained_callback_active());
                    panic!("vanilla paint failed");
                });
            }))
            .unwrap_err();
            assert_eq!(panic.downcast_ref::<&str>(), Some(&"vanilla paint failed"));
            assert_eq!(
                take(),
                ["outer", "inner", "caught by inner", "caught by outer"]
            );
            assert_eq!(current_native_mod(), None);

            game.gui_layer(VanillaGuiLayer::Hotbar, &mut batch, &mut |_, _| {
                log("vanilla paint");
            });
            assert_eq!(take(), ["outer", "inner", "vanilla paint"]);
        });
    });
}
