#![cfg(test)]

use super::{Dim, Game, api_values::TryIntoApi, native_entity_refs::tests::with_game};
use mod_api::{
    BlockChangedEvent, BlockEntityError, BlockState, BlockStateError, BlockUpdateFlags, Compound,
    Hooks, HostTransaction as _, LevelDimension, LevelHost, LevelRef, LevelWriteError, ServerHost,
};
use std::sync::{Arc, Mutex};
use sunrise_client::chunk::Chunk;
use sunrise_core::nbt::Tag;
use sunrise_world::chunk::BlockState as SavedState;

const POS: [i32; 3] = [2, 64, 2];
const KEY: (i32, i32, i32) = (2, 64, 2);
const OVERWORLD: &str = "minecraft:overworld";
const NETHER: &str = "minecraft:the_nether";

type Heard = (LevelDimension, BlockState, BlockState, BlockUpdateFlags);
#[derive(Clone, Default)]
struct Changes(Arc<Mutex<Vec<Heard>>>);

#[mod_api::__subscriptions]
impl Hooks for Changes {
    fn block_changed(&self, host: &dyn LevelHost, event: &mut BlockChangedEvent) {
        if [event.pos.x, event.pos.y, event.pos.z] == POS {
            assert_eq!(host.block_state(POS), Some(event.new));
            self.0
                .lock()
                .unwrap()
                .push((event.dimension, event.old, event.new, event.flags));
        }
    }
}

fn chest_states(game: &Game<'_>) -> [BlockState; 3] {
    let registry = game.pack.assets.registry;
    ["north", "east", "south"].map(|facing| {
        registry
            .resolve(&SavedState {
                name: "minecraft:chest".into(),
                properties: [
                    ("facing", facing),
                    ("type", "single"),
                    ("waterlogged", "false"),
                ]
                .map(|(key, value)| (key.into(), value.into()))
                .into(),
            })
            .unwrap()
            .try_into_api()
            .unwrap()
    })
}

fn with_two_levels(
    test: impl FnOnce(&mut Game<'_>, LevelRef, LevelRef, [BlockState; 3], &Mutex<Vec<Heard>>),
) {
    let changes = Changes::default();
    let mods = vec![mod_api::ModEntry::new(
        "parked_level_test",
        Box::new(changes.clone()),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            let states = chest_states(game);
            for dim in [Dim::Nether, Dim::Overworld] {
                game.enter_level(dim).unwrap();
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                let level = game.level(dim.id()).unwrap();
                assert_eq!(
                    level.set_block(game, POS, states[0], BlockUpdateFlags::KNOWN_SHAPE),
                    Ok(true)
                );
                game.world
                    .block_entities
                    .set_attachments(KEY, Some(Default::default()));
                game.world.block_log = Some(Vec::new());
                game.world.modified.clear();
            }
            let current = game.level(OVERWORLD).unwrap();
            let parked = game.level(NETHER).unwrap();
            changes.0.lock().unwrap().clear();
            test(game, current, parked, states, &changes.0);
            assert_eq!(game.dim, Dim::Overworld);
        });
    });
}

fn payload(count: u8) -> Compound {
    let snbt = format!(
        r#"{{id:"minecraft:furnace",x:100,y:3,z:99,Items:[{{Slot:0b,id:"minecraft:stick",count:{count}}}]}}"#
    );
    let Tag::Compound(data) = sunrise_client::command::snbt::parse_fully(&snbt).unwrap() else {
        unreachable!()
    };
    data
}

fn sticks(game: &Game<'_>, level: LevelRef) -> Option<u32> {
    let world = game.native_level_world(level).unwrap();
    let entity = world.block_entities.get(&KEY).unwrap();
    entity.slots()[0].as_ref().map(|stack| {
        assert_eq!(stack.item.as_ref(), "minecraft:stick");
        stack.count
    })
}

#[test]
#[ignore = "daily"]
fn parked_property_and_payload_writes_reach_only_the_addressed_entity() {
    with_two_levels(|game, current, parked, [north, east, _], heard| {
        let entity = game.block_entity(parked, POS).unwrap();
        let flags = BlockUpdateFlags::KNOWN_SHAPE | BlockUpdateFlags::CLIENTS;
        assert_eq!(parked.set_block(game, POS, east, flags), Ok(true));
        assert_eq!(
            *heard.lock().unwrap(),
            [(LevelDimension::Nether, north, east, flags)]
        );
        assert_eq!(current.block_state(game, POS), Some(north));
        assert_eq!(parked.block_state(game, POS), Some(east));
        assert_eq!(game.block_entity(parked, POS), Some(entity));
        assert_eq!(entity.merge_data(game, payload(3)), Ok(true));
        assert_eq!(entity.position(game), Some(POS));
        assert_eq!(game.block_entity(parked, POS), Some(entity));
        assert_eq!(sticks(game, parked), Some(3));
        assert_eq!(sticks(game, current), None);
        let world = game.native_level_world(parked).unwrap();
        assert!(world.block_entities.attachments(KEY).is_some());
        assert!(world.block_log.as_ref().unwrap().contains(&KEY));
        assert!(game.world.block_log.as_ref().unwrap().is_empty());
    });
}

#[test]
#[ignore = "daily"]
fn root_commit_uses_final_flags_and_keeps_same_position_levels_separate() {
    with_two_levels(|game, current, parked, [north, east, south], heard| {
        let entity = game.block_entity(parked, POS).unwrap();
        let first = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
        let last = BlockUpdateFlags::KNOWN_SHAPE;
        (game as &mut dyn ServerHost)
            .transaction(|host, _| {
                assert_eq!(parked.set_block(host, POS, east, first), Ok(true));
                assert_eq!(current.set_block(host, POS, east, last), Ok(true));
                host.transaction(|host, _| {
                    assert_eq!(parked.set_block(host, POS, south, last), Ok(true));
                    assert_eq!(entity.replace_data(host, payload(7)), Ok(true));
                    Ok::<_, ()>(())
                })?;
                assert_eq!(parked.block_state(host, POS), Some(south));
                assert_eq!(current.block_state(host, POS), Some(east));
                assert!(entity.data(host).unwrap().contains_key(&"Items".into()));
                assert!(heard.lock().unwrap().is_empty());
                Ok(())
            })
            .unwrap();
        assert_eq!(
            *heard.lock().unwrap(),
            [
                (LevelDimension::Nether, north, south, last),
                (LevelDimension::Overworld, north, east, last)
            ]
        );
        assert_eq!(game.block_entity(parked, POS), Some(entity));
        assert_eq!(sticks(game, parked), Some(7));
        assert_eq!(sticks(game, current), None);
        // Only the saved-data load asks to send a client update (the last block flags don't).
        assert_eq!(
            game.native_level_world(parked)
                .unwrap()
                .block_log
                .as_deref(),
            Some(&[KEY][..])
        );
        assert_eq!(game.world.block_log.as_deref(), Some(&[][..]));
    });
}

#[test]
#[ignore = "daily"]
fn outer_abort_restores_state_and_payload_without_notices_or_dirtying_chunks() {
    with_two_levels(|game, current, parked, [north, east, south], heard| {
        let entity = game.block_entity(parked, POS).unwrap();
        assert_eq!(entity.merge_data(game, payload(2)), Ok(true));
        game.with_resident_block_level(Dim::Nether, |game| {
            game.world.block_log.as_mut().unwrap().clear();
            game.world.modified.clear();
        })
        .unwrap();
        let flags = BlockUpdateFlags::CLIENTS | BlockUpdateFlags::KNOWN_SHAPE;
        let result = (game as &mut dyn ServerHost).transaction(|host, _| {
            assert_eq!(parked.set_block(host, POS, east, flags), Ok(true));
            host.transaction(|host, _| {
                assert_eq!(parked.set_block(host, POS, south, flags), Ok(true));
                assert_eq!(entity.replace_data(host, payload(9)), Ok(true));
                Ok::<_, ()>(())
            })?;
            Err::<(), ()>(())
        });
        assert_eq!(result, Err(()));
        assert!(heard.lock().unwrap().is_empty());
        assert_eq!(parked.block_state(game, POS), Some(north));
        assert_eq!(current.block_state(game, POS), Some(north));
        assert_eq!(game.block_entity(parked, POS), Some(entity));
        assert_eq!(sticks(game, parked), Some(2));
        let world = game.native_level_world(parked).unwrap();
        assert_eq!(world.block_log.as_deref(), Some(&[][..]));
        assert!(!world.modified.contains(&(0, 0)));
    });
}

#[test]
#[ignore = "daily"]
fn stale_or_unloaded_targets_never_write_or_load() {
    with_two_levels(|game, current, parked, [north, east, _], heard| {
        let entity = game.block_entity(parked, POS).unwrap();
        let stale =
            LevelRef::with_generation(parked.epoch(), parked.dimension(), parked.generation() + 1);
        let flags = BlockUpdateFlags::CLIENTS;
        assert_eq!(
            stale.set_block(game, POS, east, flags),
            Err(LevelWriteError::Stale)
        );
        assert_eq!(
            parked.set_block(game, POS, BlockState::new(east.block(), u16::MAX), flags),
            Err(LevelWriteError::InvalidState(BlockStateError::UnknownState))
        );
        game.with_resident_block_level(Dim::Nether, |game| game.world.chunks.remove(&(0, 0)))
            .unwrap();
        assert_eq!(parked.set_block(game, POS, east, flags), Ok(false));
        assert_eq!(
            entity.merge_data(game, payload(4)),
            Err(BlockEntityError::Stale)
        );
        assert!(game.native_level_world(parked).unwrap().chunks.is_empty());
        assert!(game.level("minecraft:the_end").is_none());
        assert_eq!(current.block_state(game, POS), Some(north));
        assert!(heard.lock().unwrap().is_empty());
    });
}
