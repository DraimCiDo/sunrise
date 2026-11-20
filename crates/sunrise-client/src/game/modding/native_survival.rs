//! The shared-host `BlockState.canSurvive` read. Only the destination has to be resident;
//! the rules themselves read neighbours as gameplay does.
use super::api_values::{TryIntoApi, block_state::block_listens};
use super::mod_api_host::level_side;
use super::native_level_signals::ReadLevel;
use super::{Game, LevelDimension};
use mod_api::{HookId, LevelReadError, LevelRef};
use std::cell::RefCell;
use sunrise_client::world::World;
use sunrise_core::{LevelKey, geometry::Direction, position::BlockPos, random::LegacyRandom};
use sunrise_world::{
    level::{BlockEntityNeighbors, LevelAccess, PreviewBlocks, Ticks, signal_access::SignalAccess},
    registry::BlockStateId,
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Query(*const World, BlockPos, BlockStateId);

thread_local! {
    static ACTIVE: RefCell<[Option<Query>; 16]> = const { RefCell::new([None; 16]) };
}

struct QueryGuard(usize);
impl QueryGuard {
    fn enter(query: Query) -> Result<Self, LevelReadError> {
        ACTIVE.with_borrow_mut(|active| {
            let slot = active
                .iter()
                .position(Option::is_none)
                .ok_or(LevelReadError::Unavailable)?;
            if active[..slot].contains(&Some(query)) {
                return Err(LevelReadError::Unavailable);
            }
            active[slot] = Some(query);
            Ok(Self(slot))
        })
    }
}
impl Drop for QueryGuard {
    fn drop(&mut self) {
        ACTIVE.with_borrow_mut(|active| active[self.0] = None);
    }
}

impl Game<'_> {
    pub(super) fn level_can_survive(
        &self,
        level: LevelRef,
        pos: [i32; 3],
        state: BlockStateId,
    ) -> Result<bool, LevelReadError> {
        if !self.level_handle_current(level) {
            return Err(LevelReadError::Stale);
        }
        let (world, ticks, lid_openers) = if self.is_logical_server() {
            if level.dimension() == self.dim.key() {
                (&self.world, &self.block_ticks, &self.lid_openers)
            } else {
                let parked = self
                    .levels
                    .parked_of(level.dimension_id())
                    .ok_or(LevelReadError::Stale)?;
                (&parked.world, &parked.block_ticks, &parked.lid_openers)
            }
        } else {
            // A separately viewed level has chunk data but no local rule/tick context.
            let remote = self
                .remote
                .as_ref()
                .filter(|remote| remote.has_level())
                .ok_or(LevelReadError::Unavailable)?;
            if remote.level_key != level.dimension() {
                return Err(if remote.viewed.world_of(level.dimension()).is_some() {
                    LevelReadError::Unavailable
                } else {
                    LevelReadError::Stale
                });
            }
            (&self.world, &self.block_ticks, &self.lid_openers)
        };
        let [x, y, z] = pos;
        if !super::native_level_refs::horizontal_in_bounds(pos) || !world.is_loaded_at(x, y, z) {
            return Err(LevelReadError::Unavailable);
        }
        let dimension =
            LevelKey::registered(level.dimension_id()).ok_or(LevelReadError::Unavailable)?;
        let pos = BlockPos::new(x, y, z);
        let _guard = QueryGuard::enter(Query(std::ptr::from_ref(world), pos, state))?;
        let read = ReadLevel {
            game: self,
            level,
            world,
            dimension: Some(LevelDimension::of_level(dimension)),
            lid_openers,
            shulkers: &world.shulkers,
            rule_ticks: Some(ticks),
        };
        // All storage lent to this adapter is shared, including the caller's staged writes.
        let mut read = SurvivalLevel {
            read,
            scratch_random: LegacyRandom::new(0),
        };
        Ok(self
            .res
            .behaviours()
            .can_survive_read(&mut read, ticks, pos, state))
    }
}

/// Adapt shared inputs to the same evaluator used by placement and shape updates. The
/// evaluator takes `&self`; there is no write path from here into a world or tick journal.
struct SurvivalLevel<'g, 'a> {
    read: ReadLevel<'g, 'a>,
    scratch_random: LegacyRandom,
}

impl BlockEntityNeighbors for SurvivalLevel<'_, '_> {}
impl LevelAccess for SurvivalLevel<'_, '_> {
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        SignalAccess::block_state(&self.read, pos)
    }
    fn set_raw(&mut self, _: BlockPos, _: BlockStateId) -> Option<BlockStateId> {
        None
    }
    fn is_loaded(&self, pos: BlockPos) -> bool {
        self.read.world.loaded(pos.x, pos.z)
    }
    fn min_y(&self) -> i32 {
        self.read.world.shape.min_y
    }
    fn max_y(&self) -> i32 {
        self.read.world.shape.max_y()
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.scratch_random
    }
    fn game_time(&self) -> i64 {
        self.read.world.game_time
    }
    fn is_client_side(&self) -> bool {
        !self.read.game.is_logical_server()
    }
    fn block_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.read.world.light(pos.x, pos.y, pos.z) & 15)
    }
    fn sky_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.read.world.light(pos.x, pos.y, pos.z) >> 4)
    }
    fn shulker_is_closed(&self, pos: BlockPos) -> Option<bool> {
        self.read.shulker_is_closed(pos)
    }
    fn published_collision_shape(
        &self,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<sunrise_core::geometry::VoxelShape> {
        self.read.published_collision_shape(pos, state)
    }
    fn before_can_survive(
        &self,
        _: &Ticks,
        _: Option<&PreviewBlocks>,
        pos: BlockPos,
        state: BlockStateId,
        answer: &mut Option<bool>,
    ) -> bool {
        let mut inert = false;
        if block_listens(HookId::CanSurvive, state)
            && let Ok(state) = state.try_into_api()
        {
            mod_api::dispatch_can_survive(
                level_side(&self.read),
                self.read.dimension(),
                pos,
                state,
                answer,
                &mut inert,
            );
        }
        inert
    }
    fn before_can_sustain_plant(
        &self,
        _: &Ticks,
        _: Option<&PreviewBlocks>,
        state: BlockStateId,
        ground: BlockStateId,
        pos: BlockPos,
        answer: &mut Option<bool>,
    ) -> bool {
        let mut inert = false;
        if block_listens(HookId::CanSustainPlant, ground)
            && let (Ok(state), Ok(ground)) = (state.try_into_api(), ground.try_into_api())
        {
            mod_api::dispatch_can_sustain_plant(
                level_side(&self.read),
                self.read.dimension(),
                state,
                ground,
                pos,
                answer,
                &mut inert,
            );
        }
        inert
    }
    fn before_may_place_on(
        &self,
        _: &Ticks,
        _: Option<&PreviewBlocks>,
        state: BlockStateId,
        ground: BlockStateId,
        pos: BlockPos,
        answer: &mut Option<bool>,
    ) -> bool {
        let mut inert = false;
        if block_listens(HookId::MayPlaceOn, state)
            && let (Ok(state), Ok(ground)) = (state.try_into_api(), ground.try_into_api())
        {
            mod_api::dispatch_may_place_on(
                level_side(&self.read),
                self.read.dimension(),
                state,
                ground,
                pos,
                answer,
                &mut inert,
            );
        }
        inert
    }
    fn before_fire_odds(
        &self,
        state: BlockStateId,
        pos: BlockPos,
        face: Direction,
        odds: &mut Option<(i32, i32)>,
    ) {
        if block_listens(HookId::FireOdds, state)
            && let Ok(state) = state.try_into_api()
        {
            mod_api::dispatch_fire_odds(
                level_side(&self.read),
                self.read.dimension(),
                state,
                pos,
                face,
                odds,
            );
        }
    }
}

#[cfg(test)]
#[path = "native_survival/tests.rs"]
mod tests;
