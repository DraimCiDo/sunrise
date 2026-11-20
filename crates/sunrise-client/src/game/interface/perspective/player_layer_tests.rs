use super::*;
use crate::game::native_entity_refs::tests::with_client_game;
use mod_api::{
    ClientHost, DynamicTextureError, EntityUuid, Flow, Hooks, Identifier, ModEntry,
    PlayerLayerChoice, PlayerLayerRefusal, RenderPlayerPreEvent, ResolvePlayerLayerEvent,
};
use std::sync::atomic::{AtomicBool, Ordering};
use sunrise_client::{
    crash::{ModCall, enter_native_mod},
    hud::Rect,
    items::ItemStack,
    mesher::Vertex,
    motion::PlayerMotion,
};

const PROFILE: &str = "test:profile_cape";
const VANILLA_WINGS: &str = "entity/equipment/wings/elytra";
const CAMERA: glam::DVec3 = glam::DVec3::new(0.0, 64.0, -5.0);

struct Listener<F>(F);

#[mod_api::__subscriptions]
impl<F> Hooks for Listener<F>
where
    F: Fn(&mut ResolvePlayerLayerEvent<'_, '_>) + Send + Sync + 'static,
{
    fn resolve_player_layer(
        &self,
        _: &dyn ClientHost,
        event: &mut ResolvePlayerLayerEvent<'_, '_>,
    ) {
        self.0(event);
    }
}

fn listener(
    name: &'static str,
    pick: impl Fn(&mut ResolvePlayerLayerEvent<'_, '_>) + Send + Sync + 'static,
) -> ModEntry {
    ModEntry::new(name, Box::new(Listener(pick)))
}

fn in_scene_process(case: &str, check: impl FnOnce()) {
    let (_, module) = module_path!().split_once("::").unwrap();
    crate::game::in_own_process(&format!("{module}::{case}"), check);
}

fn with_scene(mods: Vec<ModEntry>, check: impl FnOnce(&mut Game<'_>)) {
    // These fixtures publish process-wide registry snapshots as well as loading resources.
    // Run this exact test alone, and install its listeners before building the texture pool.
    mod_platform::testing::with_mods(mods.into(), || {
        with_client_game(|game| {
            let at = Vec3::new(0.0, 64.0, 0.0);
            game.player = Some(PlayerMotion::new(game.motion, at).unwrap());
            game.player_model = Some(Mob::new(
                0,
                game.res.player_type().clone().unwrap(),
                at,
                0.0,
            ));
            game.camera_type = CameraType::ThirdPersonBack;
            game.level.player_uuid = Some([0, 0, 0, 42]);
            let sprite = game.gui.atlas.sprites["block/stone"];
            game.gui.atlas.sprites.insert(PROFILE.into(), sprite);
            check(game);
        });
    });
}

fn profile_cape(game: &mut Game<'_>) {
    game.player_model.as_mut().unwrap().cape = Some(PROFILE.into());
}

fn texture_edit(owner: &'static str) -> sunrise_client::crash::ModGuard {
    enter_native_mod(ModCall {
        id: owner,
        hook: "client_tick_post",
        part: None,
    })
}

fn upload(game: &mut Game<'_>, key: &'static str, width: u32, height: u32) -> Rect {
    let id = Identifier::parse(key).unwrap();
    let _edit = texture_edit(key.split_once(':').unwrap().0);
    game.create_dynamic_texture(
        &id,
        width,
        height,
        &vec![255; (width * height * 4) as usize],
    )
    .unwrap();
    game.gui.atlas.sprites[key]
}

fn inside(rect: Rect, [u, v]: [f32; 2]) -> bool {
    rect.u0 <= u && u < rect.u1 && rect.v0 <= v && v < rect.v1
}

fn world(game: &Game<'_>) -> Vec<Vertex> {
    crate::game::native_mob_draw::begin_entity_frame();
    let mut out = Vec::new();
    game.player_model_geometry(CAMERA, 1.0, &mut out);
    game.mob_geometry(CAMERA, 1.0, &mut out);
    out
}

fn uses(out: &[Vertex], rect: Rect) -> bool {
    out.iter().any(|v| inside(rect, v.uv))
}

#[test]
#[ignore = "daily"]
fn no_listener_keeps_the_profile_cape_and_does_not_add_one() {
    in_scene_process(
        "no_listener_keeps_the_profile_cape_and_does_not_add_one",
        || {
            with_scene(vec![], |game| {
                let sprite = game.gui.atlas.sprites[PROFILE];
                assert!(!uses(&world(game), sprite));
                profile_cape(game);
                assert!(uses(&world(game), sprite));
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn a_remote_players_capeless_draw_gets_only_its_own_replacement() {
    in_scene_process(
        "a_remote_players_capeless_draw_gets_only_its_own_replacement",
        || {
            let key = Identifier::parse("picker:cape").unwrap();
            let mod_ = listener("picker", move |event| {
                if event.player == EntityUuid(620)
                    && !event.local
                    && event.view == PlayerLayerView::World
                    && event.layer == PlayerLayer::Cape
                {
                    let _ = event.choose(PlayerLayerChoice::UseTexture(&key));
                }
            });
            with_scene(vec![mod_], |game| {
                use sunrise_protocol::{
                    connection::Pipe,
                    packets::play::{self, Clientbound},
                };
                let (client, _peer) = Pipe::pair();
                game.connect_local(client, "Local").unwrap();
                let builtin = &game.remote.as_ref().unwrap().builtin;
                let kind = builtin
                    .get("minecraft:entity_type")
                    .unwrap()
                    .id("minecraft:player")
                    .unwrap();
                let cape = upload(game, "picker:cape", 64, 32);
                for (id, uuid, x) in [(62, 620, 2.0), (63, 630, -2.0)] {
                    game.handle_packet(Clientbound::AddEntity(play::AddEntity {
                        id,
                        uuid,
                        kind,
                        position: [x, 64.0, 0.0],
                        velocity: [0.0; 3],
                        pitch: 0,
                        yaw: 0,
                        head_yaw: 0,
                        data: 0,
                    }))
                    .unwrap();
                }
                game.mobs.iter_mut().find(|m| m.id == 63).unwrap().cape = Some(PROFILE.into());
                let vertices = world(game);
                assert!(
                    vertices
                        .iter()
                        .any(|v| v.pos[0] > 1.0 && inside(cape, v.uv))
                );
                assert!(
                    vertices
                        .iter()
                        .all(|v| !inside(cape, v.uv) || v.pos[0] > 1.0)
                );
                assert!(
                    vertices
                        .iter()
                        .any(|v| v.pos[0] < -1.0 && inside(game.gui.atlas.sprites[PROFILE], v.uv))
                );
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn wings_choose_their_own_texture_or_fall_back_to_equipment() {
    in_scene_process(
        "wings_choose_their_own_texture_or_fall_back_to_equipment",
        || {
            #[derive(Clone, Copy)]
            enum WingsChoice {
                Keep,
                Replace,
                Equipment,
            }
            let mode = Arc::new(Mutex::new(WingsChoice::Keep));
            let wings = Identifier::parse("picker:wings").unwrap();
            let cape = Identifier::parse("picker:cape").unwrap();
            let choose = mode.clone();
            let mod_ = listener("picker", move |event| {
                let choice = if event.layer == PlayerLayer::Cape {
                    PlayerLayerChoice::UseTexture(&cape)
                } else {
                    match *choose.lock().unwrap() {
                        WingsChoice::Keep => PlayerLayerChoice::Keep,
                        WingsChoice::Replace => PlayerLayerChoice::UseTexture(&wings),
                        WingsChoice::Equipment => PlayerLayerChoice::UseEquipmentTexture,
                    }
                };
                let _ = event.choose(choice);
            });
            with_scene(vec![mod_], |game| {
                let cape = upload(game, "picker:cape", 64, 32);
                let wings = upload(game, "picker:wings", 128, 64);
                profile_cape(game);
                assert!(uses(&world(game), cape));
                game.armor[1] = Some(ItemStack::new("minecraft:elytra", 1));
                let profile = game.gui.atlas.sprites[PROFILE];
                let equipment = game.gui.atlas.sprites[VANILLA_WINGS];
                let normal = world(game);
                assert!(uses(&normal, profile));
                assert!(!uses(&normal, cape));
                assert!(!uses(&normal, wings));
                *mode.lock().unwrap() = WingsChoice::Replace;
                let changed = world(game);
                assert!(uses(&changed, wings));
                assert!(!uses(&changed, profile));
                let geometry = |out: &[Vertex], rect: Rect| {
                    out.iter()
                        .filter(|v| inside(rect, v.uv))
                        .map(|v| v.pos)
                        .collect::<Vec<_>>()
                };
                assert_eq!(geometry(&normal, profile), geometry(&changed, wings));
                *mode.lock().unwrap() = WingsChoice::Equipment;
                let fallback = world(game);
                assert!(uses(&fallback, equipment));
                assert!(!uses(&fallback, profile));
                assert!(!uses(&fallback, wings));
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn world_inventory_and_creative_use_separate_draws() {
    in_scene_process("world_inventory_and_creative_use_separate_draws", || {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let calls = seen.clone();
        let key = Identifier::parse("picker:world").unwrap();
        let mod_ = listener("picker", move |event| {
            calls
                .lock()
                .unwrap()
                .push((event.player, event.local, event.layer, event.view));
            if event.view == PlayerLayerView::World {
                let _ = event.choose(PlayerLayerChoice::UseTexture(&key));
            }
        });
        with_scene(vec![mod_], |game| {
            profile_cape(game);
            let texture = upload(game, "picker:world", 64, 32);
            let profile = game.gui.atlas.sprites[PROFILE];
            assert!(uses(&world(game), texture));
            let mut survival = HudBatch::scratch([320, 240]);
            game.screen = Screen::Inventory;
            game.draw_inventory_player(&mut survival, 0.0, 0.0);
            let mut creative = HudBatch::scratch([320, 240]);
            game.screen = Screen::Creative;
            let tab = game
                .creative_screen
                .tab_headers()
                .iter()
                .position(|tab| tab.inventory)
                .unwrap();
            game.creative_screen.select(tab);
            game.draw_creative(&mut creative);
            for batch in [survival, creative] {
                assert!(batch.vertices.iter().any(|v| inside(profile, v.uv)));
                assert!(batch.vertices.iter().all(|v| !inside(texture, v.uv)));
            }
            assert_eq!(
                *seen.lock().unwrap(),
                [
                    (
                        EntityUuid(42),
                        true,
                        PlayerLayer::Cape,
                        PlayerLayerView::World
                    ),
                    (
                        EntityUuid(42),
                        true,
                        PlayerLayer::Cape,
                        PlayerLayerView::InventoryPreview
                    ),
                    (
                        EntityUuid(42),
                        true,
                        PlayerLayer::Cape,
                        PlayerLayerView::InventoryPreview
                    ),
                ]
            );
        });
    });
}

struct Cancel(Arc<AtomicBool>);

#[mod_api::__subscriptions]
impl Hooks for Cancel {
    fn render_player_pre(&self, _: &dyn ClientHost, _: &mut RenderPlayerPreEvent<'_>) -> Flow {
        if self.0.load(Ordering::Relaxed) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

#[test]
#[ignore = "daily"]
fn only_eligible_layers_of_an_uncanceled_avatar_are_asked() {
    in_scene_process(
        "only_eligible_layers_of_an_uncanceled_avatar_are_asked",
        || {
            let cancel = Arc::new(AtomicBool::new(false));
            let seen = Arc::new(Mutex::new(Vec::new()));
            let calls = seen.clone();
            let picker = listener("picker", move |event| {
                calls.lock().unwrap().push(event.layer)
            });
            with_scene(
                vec![
                    ModEntry::new("cancel", Box::new(Cancel(cancel.clone()))),
                    picker,
                ],
                |game| {
                    let draw = |game: &Game<'_>| {
                        let _ = world(game);
                        std::mem::take(&mut *seen.lock().unwrap())
                    };
                    assert_eq!(
                        draw(game),
                        [PlayerLayer::Cape],
                        "no wings without worn equipment"
                    );
                    game.player_model.as_mut().unwrap().extra.hidden_model_parts = 1;
                    assert!(draw(game).is_empty(), "cape setting off");
                    let avatar = game.player_model.as_mut().unwrap();
                    avatar.extra.hidden_model_parts = 0;
                    avatar.invisible = true;
                    assert!(draw(game).is_empty(), "invisible avatars have no cape");
                    let avatar = game.player_model.as_mut().unwrap();
                    avatar.invisible = false;
                    avatar.extra.spectator = true;
                    assert!(draw(game).is_empty(), "spectators have no layers");
                    game.player_model.as_mut().unwrap().extra.spectator = false;
                    game.armor[1] = Some(ItemStack::new("minecraft:elytra", 1));
                    assert_eq!(draw(game), [PlayerLayer::Wings], "wings exclude the cape");
                    cancel.store(true, Ordering::Relaxed);
                    assert!(
                        draw(game).is_empty(),
                        "a canceled avatar has no layer submissions"
                    );
                },
            );
        },
    );
}

#[test]
#[ignore = "daily"]
fn refusal_and_listener_failure_keep_the_latest_accepted_choice() {
    in_scene_process(
        "refusal_and_listener_failure_keep_the_latest_accepted_choice",
        || {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let pick = |owner, id| {
                let key = Identifier::parse(id).unwrap();
                let seen = seen.clone();
                listener(owner, move |event| {
                    seen.lock()
                        .unwrap()
                        .push((owner, event.choose(PlayerLayerChoice::UseTexture(&key))))
                })
            };
            let refusals = seen.clone();
            let foreign = Identifier::parse("last:texture").unwrap();
            let freed = Identifier::parse("bad:freed").unwrap();
            let wrong_size = Identifier::parse("bad:layout").unwrap();
            let bad = listener("bad", move |event| {
                let mut row = refusals.lock().unwrap();
                row.push((
                    "wrong layer",
                    event.choose(PlayerLayerChoice::UseEquipmentTexture),
                ));
                for (label, id) in [
                    ("foreign", &foreign),
                    ("freed", &freed),
                    ("layout", &wrong_size),
                ] {
                    row.push((label, event.choose(PlayerLayerChoice::UseTexture(id))));
                }
            });
            let last_seen = seen.clone();
            let panics = listener("panics", move |event| {
                last_seen
                    .lock()
                    .unwrap()
                    .push(("panics", event.choose(PlayerLayerChoice::Keep)));
                panic!("listener failed after staging Keep");
            });
            let continued = seen.clone();
            let observer = listener("observer", move |_| {
                continued.lock().unwrap().push(("observer", Ok(())))
            });
            let mods = vec![
                pick("first", "first:texture"),
                pick("last", "last:texture"),
                bad,
                panics,
                observer,
            ];
            with_scene(mods, |game| {
                let first = upload(game, "first:texture", 64, 32);
                let last = upload(game, "last:texture", 64, 32);
                upload(game, "bad:layout", 64, 64);
                upload(game, "bad:freed", 64, 32);
                {
                    let _edit = texture_edit("bad");
                    game.free_dynamic_texture(&Identifier::parse("bad:freed").unwrap())
                        .unwrap();
                }
                let out = world(game);
                assert!(uses(&out, last));
                assert!(!uses(&out, first));
                assert_eq!(
                    *seen.lock().unwrap(),
                    [
                        ("first", Ok(())),
                        ("last", Ok(())),
                        ("wrong layer", Err(PlayerLayerRefusal::WrongLayer)),
                        (
                            "foreign",
                            Err(PlayerLayerRefusal::Texture(DynamicTextureError::NotOwned))
                        ),
                        (
                            "freed",
                            Err(PlayerLayerRefusal::Texture(DynamicTextureError::Unknown))
                        ),
                        ("layout", Err(PlayerLayerRefusal::IncompatibleLayout)),
                        ("panics", Ok(())),
                        ("observer", Ok(())),
                    ]
                );
            });
        },
    );
}
