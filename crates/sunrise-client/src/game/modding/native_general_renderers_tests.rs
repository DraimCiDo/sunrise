//! A mod's general entity types on the client (#792, stage 5): a renderer registered for one
//! resolves to its catalog slot, a type with none is named; each row shown on the window is drawn
//! by its type's renderer at the frame's interpolated pose, with its synced attachments; a renderer that fails is dropped until resources reload; a type
//! with no renderer draws nothing.
use super::Game;
use super::native_entity_renderers::EntityRendererTable;
use crate::game::general::GeneralTypeCatalog;
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, BlockEntityDrawContext, EntityRenderState,
    EntityRenderer,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};
use sunrise_client::mesher::Vertex;
use sunrise_core::nbt::Tag;
use sunrise_protocol::packets::play::{self, Clientbound as C};

fn declaration(id: &str) -> mod_api::__private::GeneralEntityTypeDeclaration {
    mod_api::__private::GeneralEntityTypeDeclaration {
        mod_id: "test".into(),
        id: id.into(),
        category: mod_api::GeneralEntityCategory::Misc,
        width: 0.5,
        height: 0.5,
        save_policy: mod_api::GeneralSavePolicy::Save,
        summonable: true,
        tracking_chunks: 8,
        update_interval: 1,
        payload_version: 1,
        required_features: Vec::new(),
    }
}

/// Draws nothing.
struct Blank;
impl EntityRenderer for Blank {
    fn submit(
        &self,
        _: &EntityRenderState,
        _: &mut dyn BlockEntityDrawContext,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn a_general_types_renderer_resolves_to_its_slot_and_a_type_without_one_is_named() {
    let catalog =
        GeneralTypeCatalog::freeze(&[declaration("test:missile"), declaration("test:plain")])
            .unwrap();
    let registered: BTreeMap<String, Arc<dyn EntityRenderer>> = [(
        "test:missile".to_owned(),
        Arc::new(Blank) as Arc<dyn EntityRenderer>,
    )]
    .into();
    let (table, unknown) =
        EntityRendererTable::resolve(&registered, &[], &Default::default(), &catalog, |_| false);
    assert!(
        unknown.is_empty(),
        "a general key is an entity type: {unknown:?}"
    );
    let missile = catalog.slot("test:missile").unwrap();
    assert!(table.general(missile).is_some());
    assert!(table.general(catalog.slot("test:plain").unwrap()).is_none());
    assert_eq!(table.unrendered_general(&catalog), ["test:plain"]);
}

/// The type's synced field the renderer reads while drawing.
static FUEL: AttachmentType<i32> = AttachmentType {
    namespace: "test",
    name: "fuel",
    default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
        decode: |c| match c.get(&"n".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: true,
};

/// `test:missile`'s renderer: draws one stone quad from its pose's origin, and keeps the last
/// state it was handed and the fuel it read from the snapshot.
#[derive(Default)]
struct Missile {
    seen: Mutex<Option<EntityRenderState>>,
    fuel: Mutex<Option<i32>>,
}
impl EntityRenderer for Missile {
    fn submit(
        &self,
        entity: &EntityRenderState,
        draw: &mut dyn BlockEntityDrawContext,
    ) -> Result<(), String> {
        *self.seen.lock().unwrap() = Some(entity.clone());
        let fuel = entity
            .attachments
            .as_ref()
            .ok_or("no snapshot")?
            .get(&FUEL)
            .map(|value| value.copied())
            .map_err(|e| format!("{e:?}"))?;
        *self.fuel.lock().unwrap() = fuel;
        let corners = [
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        draw.quad("block/stone", corners, [0.0, 0.0, 16.0, 16.0], 0xFF_FFFF);
        Ok(())
    }
}

/// `test:bomb`'s renderer: panics, and counts its calls.
#[derive(Default)]
struct Bomb {
    calls: AtomicU32,
}
impl EntityRenderer for Bomb {
    fn submit(
        &self,
        _: &EntityRenderState,
        _: &mut dyn BlockEntityDrawContext,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        panic!("the bomb's renderer");
    }
}

/// The scene's mod: `test:missile`, `test:bomb` and `test:plain` declared, `test:fuel` synced,
/// renderers for the first two.
struct Scene {
    missile: Arc<Missile>,
    bomb: Arc<Bomb>,
}
#[mod_api::__subscriptions]
impl mod_api::Hooks for Scene {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        r.register_attachment_type("test:fuel", true).unwrap();
        for path in ["missile", "bomb", "plain"] {
            r.declare_general_entity(
                path,
                mod_api::GeneralEntityCategory::Misc,
                0.5,
                0.5,
                mod_api::GeneralSavePolicy::Save,
                true,
                8,
                1,
                1,
                &[],
            )
            .unwrap();
        }
    }
    fn register_entity_renderers(
        &self,
        _: &mod_api::RegistrationContext,
        event: &mut mod_api::RegisterEntityRenderersEvent<'_>,
    ) {
        event
            .renderers
            .register("test:missile".into(), self.missile.clone());
        event
            .renderers
            .register("test:bomb".into(), self.bomb.clone());
    }
}

/// The headless camera just above build height, where vanilla needs no compiled section.
const CAMERA: glam::DVec3 = glam::DVec3::new(0.0, 321.0, -5.0);

/// The server's spawn of row `id` of `key` at `at`.
fn spawn(id: i32, key: &str, at: [f64; 3]) -> C {
    use sunrise_protocol::general_entity::{CHANNEL, Message, encode};
    C::CustomPayload(sunrise_protocol::packets::configuration::CustomPayload {
        channel: CHANNEL.into(),
        data: encode(&Message::Spawn {
            id,
            uuid: 0x100 + id as u128,
            type_key: key.into(),
            position: at,
            rotation: [0.0, 0.0],
            velocity: [0.0; 3],
        })
        .unwrap(),
    })
}

/// The server's first snapshot of row `id`'s synced fields: `fuel`, or nothing synced.
fn snapshot(id: u32, fuel: Option<i32>) -> C {
    use sunrise_mod_runtime::attachments::{AttachmentSync, Holder};
    let changes = fuel.map_or_else(Vec::new, |n| {
        vec![AttachmentSync {
            namespace: "test".into(),
            name: "fuel".into(),
            value: Some([("n".into(), Tag::Int(n))].into()),
        }]
    });
    C::CustomPayload(sunrise_protocol::packets::configuration::CustomPayload {
        channel: sunrise_protocol::attachment_sync::CHANNEL.into(),
        data: sunrise_protocol::attachment_sync::encode(
            &sunrise_protocol::attachment_sync::Message {
                holder: Holder::Entity(id),
                replace: true,
                changes,
            },
        )
        .unwrap(),
    })
}

/// One frame of the entity pass at `alpha`.
fn frame(game: &mut Game<'_>, alpha: f64) -> Vec<Vertex> {
    let mut out = Vec::new();
    game.mob_geometry(CAMERA, alpha, &mut out);
    out
}

/// The scene on a client (its block atlas loaded) joined to a server through memory, with its
/// rows shown: `test:missile`
/// (id 5) going from (0, 321, 0) to (4, 321, 0) with fuel 7, `test:bomb` (6) and `test:plain` (7).
/// In a child process running only `test`: the registrations are kept once per process.
fn in_scene(test: &str, check: impl FnOnce(&mut Game<'_>, &Scene)) {
    if std::env::var_os("SUNRISE_GENERAL_RENDERERS_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("game::native_general_renderers_tests::{test}"),
                "--include-ignored",
                "--nocapture",
            ])
            .env("SUNRISE_GENERAL_RENDERERS_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let held = Scene {
        missile: Arc::default(),
        bomb: Arc::default(),
    };
    let (missile, bomb) = (held.missile.clone(), held.bomb.clone());
    let mods = move || {
        vec![mod_api::ModEntry::new(
            "test",
            Box::new(Scene {
                missile: missile.clone(),
                bomb: bomb.clone(),
            }),
        )]
    };
    mod_platform::__private::initialize().unwrap();
    // A general type is declared only while the registration phase is current.
    let registration =
        mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    mod_platform::testing::with_mods(mods().into(), || {
        mod_platform::__private::part_tests::install_parts_fixture(&mods()).unwrap();
        super::mods::register_entity_renderers(super::execution_role::ExecutionRole::Client);
        // A frame draws with a world open, as the window does.
        drop(registration);
        let _world = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        super::native_entity_refs::tests::with_client_game(|game| {
            let (pipe, _server) = sunrise_protocol::connection::Pipe::pair();
            game.connect_local(pipe, "Steve").unwrap();
            game.resolve_entity_renderers();
            // Logged in to the overworld: the client's level, whose rows its host names.
            game.handle_packet(C::Login(play::Login {
                player_id: 1,
                hardcore: false,
                levels: vec!["minecraft:overworld".into()],
                max_players: 20,
                chunk_radius: 8,
                simulation_distance: 8,
                reduced_debug_info: false,
                show_death_screen: true,
                limited_crafting: false,
                spawn: play::SpawnInfo {
                    dimension_type: 0,
                    dimension: "minecraft:overworld".into(),
                    seed: 1,
                    game_type: 0,
                    previous_game_type: None,
                    debug: false,
                    flat: false,
                    last_death: None,
                    portal_cooldown: 0,
                    sea_level: 63,
                },
                online_mode: false,
                enforces_secure_chat: false,
            }))
            .unwrap();
            for (id, key) in [(5, "test:missile"), (6, "test:bomb"), (7, "test:plain")] {
                game.handle_packet(spawn(id, key, [0.0, 321.0, 0.0]))
                    .unwrap();
                game.handle_packet(snapshot(id as u32, (id == 5).then_some(7)))
                    .unwrap();
            }
            let row = game
                .remote
                .as_mut()
                .unwrap()
                .general
                .rows
                .get_mut(&5)
                .unwrap();
            (row.previous, row.position) = ([0.0, 321.0, 0.0], [4.0, 321.0, 0.0]);
            (row.previous_rotation, row.rotation) = ([0.0, -20.0], [90.0, 20.0]);
            row.age = 40;
            check(game, &held);
        });
    });
}

/// A quarter of the way between ticks: the pose stack starts at x = 1, and the state is
/// interpolated the same way; the renderer reads the row's synced fuel through its host.
#[test]
#[ignore = "daily"]
fn a_row_is_drawn_by_its_types_renderer_at_the_frames_pose_reading_its_synced_fields() {
    in_scene(
        "a_row_is_drawn_by_its_types_renderer_at_the_frames_pose_reading_its_synced_fields",
        |game, scene| {
            // A one-plane test frustum: +Z in camera space is in front.
            game.view_frustum = Some(sunrise_client::gfx::test_frustum([glam::Vec4::Z; 6]));
            for (id, at) in [
                (8, [0.0, 321.0, -8.0]),  // behind the camera
                (9, [0.0, 321.0, 300.0]), // beyond the draw range
                (10, [0.0, 319.0, 0.0]),  // in build height, no compiled section
            ] {
                game.handle_packet(spawn(id, "test:missile", at)).unwrap();
                game.handle_packet(snapshot(id as u32, Some(99))).unwrap();
            }
            let out = frame(game, 0.25);
            assert_eq!(
                out.len(),
                4,
                "the missile's one quad, nothing for the others: {out:?}"
            );
            let xs: Vec<f32> = out.iter().map(|v| v.pos[0]).collect();
            let min = xs.iter().copied().fold(f32::INFINITY, f32::min);
            assert_eq!(min, 1.0 - CAMERA.x as f32);
            let seen = scene.missile.seen.lock().unwrap().clone().unwrap();
            assert_eq!((seen.id, seen.partial, seen.age), (5, 0.25, 40.25));
            assert_eq!((seen.yaw, seen.pitch), (22.5, -10.0));
            assert!(seen.living.is_none());
            assert_eq!(*scene.missile.fuel.lock().unwrap(), Some(7));
        },
    );
}

#[test]
#[ignore = "daily"]
fn general_submission_keeps_lookup_order_across_removal_and_reentry() {
    in_scene(
        "general_submission_keeps_lookup_order_across_removal_and_reentry",
        |game, _| {
            for (id, x) in [(12, 6.0), (8, 8.0)] {
                game.handle_packet(spawn(id, "test:missile", [x, 321.0, 0.0]))
                    .unwrap();
                game.handle_packet(snapshot(id as u32, Some(1))).unwrap();
            }
            let submitted = |game: &mut Game<'_>| {
                frame(game, 1.0)
                    .chunks_exact(4)
                    .map(|quad| quad.iter().map(|v| v.pos[0]).fold(f32::INFINITY, f32::min))
                    .collect::<Vec<_>>()
            };
            assert_eq!(submitted(game), [4.0, 6.0, 8.0]);
            game.handle_packet(C::RemoveEntities(play::RemoveEntities { ids: vec![12] }))
                .unwrap();
            game.handle_packet(spawn(12, "test:missile", [6.0, 321.0, 0.0]))
                .unwrap();
            game.handle_packet(snapshot(12, Some(1))).unwrap();
            assert_eq!(submitted(game), [4.0, 8.0, 6.0]);
        },
    );
}

/// A panicking renderer draws nothing and is asked no more until a reload resolves it again; a
/// type with no renderer draws nothing, and neither stops the others.
#[test]
#[ignore = "daily"]
fn a_failing_renderer_is_dropped_until_reload_and_a_type_without_one_draws_nothing() {
    in_scene(
        "a_failing_renderer_is_dropped_until_reload_and_a_type_without_one_draws_nothing",
        |game, scene| {
            for _ in 0..2 {
                assert_eq!(frame(game, 1.0).len(), 4, "the missile alone");
                assert_eq!(scene.bomb.calls.load(Ordering::Relaxed), 1);
            }
            super::model_baking_completed_tests::reload(game);
            assert_eq!(frame(game, 1.0).len(), 4);
            assert_eq!(
                scene.bomb.calls.load(Ordering::Relaxed),
                2,
                "asked again after it"
            );
            // The reload kept the rows.
            assert_eq!(game.remote.as_ref().unwrap().general.rows.len(), 3);
        },
    );
}
