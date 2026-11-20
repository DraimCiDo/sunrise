//! Exercise the resident tick through the same registered Part path as a native mod.
use super::{
    Game,
    native_entity_refs::tests::{fixture_root, run_fresh_cases, with_game_at},
};
use mod_api::{
    BlockEntityPart, BlockEntityTickSubject, BlockEntityTickViewEvent, Hooks, ModEntry, Registrar,
    RegistrationContext,
};
use std::sync::Arc;
use sunrise_client::{
    chunk::Chunk,
    containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields},
};

const POS: (i32, i32, i32) = (1, 64, 1);
const ID: &str = "block_entity_view_fixture:press";

struct Fixture;

#[mod_api::__subscriptions]
impl Hooks for Fixture {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        let kind = registrar.block_entity_type("press").build();
        registrar.block_entity_part(&kind, Fixture).unwrap();
    }
}

#[mod_api::sunrise_part]
impl mod_api::BlockPart for Fixture {}

#[mod_api::sunrise_part]
impl BlockEntityPart for Fixture {
    fn block_entity_tick_view(&self, event: &mut BlockEntityTickViewEvent<'_>) {
        let BlockEntityTickSubject::View(view) = &mut event.subject else {
            panic!("the tick did not lend a resident view");
        };
        assert_eq!(view.position(), [1, 64, 1]);
        assert_eq!(view.state().ordinal(), 0); // stone has one state
        assert_eq!(view.level().dimension_id(), "minecraft:overworld");
        assert_eq!(view.data_slots(), [13, 7]);
        assert!(!view.set_data_slot(2, 900));
        assert!(!view.set_data_slot(0, 13));
        assert!(view.set_data_slot(0, 41));
        assert_eq!(view.data_slots(), [41, 7]);
    }
}

fn with_fixture(name: &str, test: impl FnOnce(&mut Game<'_>, mod_api::BlockEntityRef)) {
    if run_fresh_cases(name, "SUNRISE_BLOCK_ENTITY_VIEW_CASE", &["server"]) {
        return;
    }
    sunrise_guest_stack::run_on_guest_stack(|| {
        let mods: mod_api::Mods = vec![ModEntry::new(
            "block_entity_view_fixture",
            Box::new(Fixture),
        )]
        .into();
        mod_platform::testing::with_mods(mods.clone(), || {
            super::api_values::install_parts(&mods).unwrap();
            mod_api::__private::mark_game_thread();
            let root = super::mods::content_root(&fixture_root(), &Default::default()).unwrap();
            with_game_at(root.prepared().to_owned(), |game| {
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.set(POS.0, POS.1, POS.2, stone).unwrap();
                game.world.block_entities.insert(
                    POS,
                    BlockEntity::Mod(Box::new(ModBlockEntity::new(
                        ID.into(),
                        ModBlockEntityFields {
                            data: vec![13, 7],
                            ..Default::default()
                        },
                    ))),
                );
                let level = game.level(game.dim.id()).unwrap();
                let handle = game.block_entity(level, [1, 64, 1]).unwrap();
                game.world.modified.clear();
                test(game, handle);
            });
        });
    });
}

#[test]
#[ignore = "daily"]
fn resident_tick_reads_and_updates_the_same_entity() {
    const NAME: &str =
        "game::block_entity_view_tests::resident_tick_reads_and_updates_the_same_entity";
    with_fixture(NAME, |game, entity| {
        game.tick_mod_block_entity(POS);
        assert_eq!(entity.data_slots(game).unwrap(), [41, 7]);
        assert_eq!(
            game.block_entity(game.level(game.dim.id()).unwrap(), [1, 64, 1]),
            Some(entity)
        );
        assert!(!game.world.modified.contains(&(0, 0)));
    });
}

#[test]
#[ignore = "daily"]
fn nested_commit_is_undone_by_outer_abort() {
    const NAME: &str = "game::block_entity_view_tests::nested_commit_is_undone_by_outer_abort";
    with_fixture(NAME, |game, entity| {
        let result = mod_api::run_transaction(game, |game, _| {
            mod_api::run_transaction(game, |game, _| {
                game.tick_mod_block_entity(POS);
                assert_eq!(entity.data_slots(game).unwrap(), [41, 7]);
                Ok::<_, &'static str>(())
            })?;
            assert_eq!(entity.data_slots(game).unwrap(), [41, 7]);
            Err::<(), _>("abort")
        });
        assert_eq!(result, Err("abort"));
        assert_eq!(entity.data_slots(game).unwrap(), [13, 7]);
        assert!(!game.world.modified.contains(&(0, 0)));
    });
}

#[test]
#[ignore = "daily"]
fn removed_holder_is_not_ticked_or_revived() {
    const NAME: &str = "game::block_entity_view_tests::removed_holder_is_not_ticked_or_revived";
    with_fixture(NAME, |game, entity| {
        game.world.block_entities.remove(&POS);
        game.tick_mod_block_entity(POS);
        assert_eq!(entity.data_slots(game), None);
        assert!(game.world.block_entities.get(&POS).is_none());
    });
}
