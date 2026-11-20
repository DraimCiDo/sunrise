//! Frogs (`Frog`, `FrogAi`): the `Croak`, `ShootTongue` and long jump
//! behaviours as goals, the poses they set and the client's animation states.
//! AnimState states: jump, croak, tongue, swim idle.
//!
//! The brain itself (`FrogAi`: activities, sensors, gates) lives in
//! `animal_brain`; this file holds the frog's behaviours it runs and the
//! frog's memories.
use super::{
    DamageLevel, Mob, MobEvent, MobWorld, Species, Who,
    animal_brain::{Tracker, WalkTarget, mem},
    goals::{Cx, Goal, LOOK, MOVE, goal_clone},
    sync,
};
use crate::blocks::Fluid;
use mod_api::SoundSource;
use sunrise_core::{
    geometry::Vec3,
    random::{LegacyRandom, RandomSource},
};

const JUMP: usize = 0;
const CROAK: usize = 1;
const TONGUE: usize = 2;
const SWIM_IDLE: usize = 3;

/// `Croak.CROAK_TICKS`.
const CROAK_TICKS: i32 = 60;
/// `ShootTongue`'s duration and eating distance.
const TONGUE_TIME_OUT: i32 = 100;
const EATING_DISTANCE: f64 = 1.75;
/// `FrogAttackablesSensor.TARGET_DETECTION_DISTANCE`.
const DETECTION_DISTANCE: f64 = 10.0;

/// `FrogAi.TIME_BETWEEN_LONG_JUMPS` and the brain memory
/// `LONG_JUMP_COOLDOWN_TICKS` (saved with the others, as `finalize` sets it).
const TIME_BETWEEN_LONG_JUMPS: (i32, i32) = (100, 140);
/// `GoatAi.TIME_BETWEEN_LONG_JUMPS`.
const GOAT_TIME_BETWEEN_LONG_JUMPS: (i32, i32) = (600, 1200);
const LONG_JUMP_COOLDOWN: &str = "long_jump_cooling_down";
/// `FrogAi`'s `LongJumpToPreferredBlock`: `MAX_LONG_JUMP_WIDTH`,
/// `MAX_LONG_JUMP_HEIGHT`, `MAX_JUMP_VELOCITY_MULTIPLIER` and the chance to
/// look for a lily pad or big dripleaf first.
const JUMP_WIDTH: i32 = 4;
const JUMP_HEIGHT: i32 = 2;
/// The goat's `MAX_JUMP_VELOCITY_MULTIPLIER` is the same number.
const JUMP_VELOCITY_MULTIPLIER: f32 = 3.571_428_8;
const PREFERRED_BLOCKS_CHANCE: f32 = 0.5;
/// `GoatAi`'s `LongJumpToRandomPos`: `MAX_LONG_JUMP_WIDTH` and
/// `MAX_LONG_JUMP_HEIGHT`.
const GOAT_JUMP_WIDTH: i32 = 5;
const GOAT_JUMP_HEIGHT: i32 = 5;
/// `LongJumpToRandomPos.ALLOWED_ANGLES`, `FIND_JUMP_TRIES` and
/// `PREPARE_JUMP_DURATION`.
const ALLOWED_ANGLES: [i32; 4] = [65, 70, 75, 80];
const FIND_JUMP_TRIES: i32 = 20;
const PREPARE_JUMP_DURATION: i32 = 40;
/// `LongJumpMidJump`'s duration (`TIME_OUT_DURATION`).
const MID_JUMP_DURATION: i32 = 100;

/// `ShootTongue.State`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Phase {
    MoveToTarget,
    CatchAnimation,
    EatAnimation,
    #[default]
    Done,
}

/// The frog's brain memories and `DATA_TONGUE_TARGET_ID`.
#[derive(Clone, Debug, Default)]
pub struct State {
    croak: i32,
    /// The `IsInWaterSensor`'s `IS_IN_WATER` memory: the swim activity
    /// follows it, up to 20 ticks behind the water.
    pub(super) in_water: bool,
    /// `LONG_JUMP_MID_JUMP`.
    pub(super) mid_jump: bool,
    /// `NEAREST_ATTACKABLE`.
    nearest: Option<u32>,
    /// `UNREACHABLE_TONGUE_TARGETS`: the mob and the ticks it stays in mind.
    unreachable: Vec<(u32, i32)>,
    /// `ATTACK_TARGET` while `ShootTongue` runs, and the synched tongue target.
    pub(super) target: Option<u32>,
    pub tongue_target: Option<u32>,
    phase: Phase,
    eat_timer: i32,
    path_counter: i32,
    time: i32,
    /// `LivingEntity.discardFriction`, set through the long jump.
    pub discard_friction: bool,
}

/// The frog brain's attack target, while its tongue behavior is active.
pub fn attack_target(m: &Mob) -> Option<Who> {
    m.extra.frog.target.map(Who::Mob)
}

pub fn register(m: &mut Mob) {
    // `Frog()`: SmoothSwimmingMoveControl(85, 10, 0.02, 0.1, true). The
    // frog's movement speed is 1.0, so the controller's 0.1 outside-water
    // speed is what keeps a walking frog at vanilla's pace; the plain ground
    // controller walks ten times too fast (#2213).
    m.ai.mov.kind = super::control::MoveKind::SmoothSwimming {
        max_turn_x: 85.0,
        max_turn_y: 10.0,
        in_water: 0.02,
        outside: 0.1,
        gravity: true,
    };
    // `Frog.FrogPathNavigation` (`AmphibiousPathNavigation`, and
    // `prefersShallowSwimming` is the frog's own override): water deeper than
    // ten blocks under sea level costs more, so a frog paths the shallows.
    m.ai.nav.kind = super::nav::NavKind::Amphibious {
        prefers_shallow: true,
    };
    // `Frog.BRAIN_PROVIDER`: the brain and its sensors' start delays.
    super::animal_brain::init(m, i64::from(m.id));
}

/// `Entity.setPose` and `Frog.onSyncedDataUpdated(DATA_POSE)`.
pub fn set_pose(m: &mut Mob, id: u8) {
    let old = std::mem::replace(&mut m.extra.pose, id);
    if old != id {
        pose_changed(m);
    }
}
/// The animation states of the pose the frog has taken.
pub fn pose_changed(m: &mut Mob) {
    let age = m.age as f32;
    let pose = m.extra.pose;
    let states = &mut m.anim.states;
    for (state, on) in [
        (JUMP, pose == sync::LONG_JUMPING),
        (CROAK, pose == sync::CROAKING),
        (TONGUE, pose == sync::USING_TONGUE),
    ] {
        if on {
            states[state].start(age);
        } else {
            states[state].stop();
        }
    }
}
/// `Frog.tick` on a client: the idle swim while it floats still. The server
/// runs it too, for a window that shows its own mobs.
pub fn tick(m: &mut Mob) {
    let age = m.age as f32;
    let still = m.fluids.in_water && m.limb_speed <= 1.0e-5;
    m.anim.states[SWIM_IDLE].animate_when(still, age);
}
/// `Frog.travelInWater`: `moveRelative(getSpeed(), input)`, the move, then
/// the motion scaled by 0.9. `getSpeed` is the speed the move control stored.
/// Out of water the usual travel runs.
pub(super) fn travel(
    m: &mut Mob,
    w: &MobWorld,
    level: &mut DamageLevel<'_>,
    input: Vec3,
    events: &mut Vec<MobEvent>,
    pause: bool,
) -> Result<(), super::living::TravelTrample> {
    m.move_relative(m.ai.speed, input);
    m.travel_move(w, level, events, pause, |m, _, _, _| {
        m.velocity = m.velocity.scale(0.9);
    })
}

/// `Frog.canEat`: a slime or magma cube of size 1.
/// `Frog.updateWalkAnimation`: the frog's short steps walk the legs 25 times
/// the distance (the base is 4), and not at all while the jump plays.
pub fn walk_target(m: &Mob, distance: f32) -> f32 {
    if m.anim.states[JUMP].started.is_some() {
        0.0
    } else {
        (distance * 25.0).min(1.0)
    }
}

fn can_eat(species: Species, size: i32) -> bool {
    matches!(species, Species::Slime | Species::MagmaCube) && size == 1
}

/// The frog's memories that expire: `UNREACHABLE_TONGUE_TARGETS`.
pub(super) fn forget(m: &mut Mob) {
    m.extra.frog.unreachable.retain_mut(|(_, ttl)| {
        *ttl -= 1;
        *ttl > 0
    });
}

/// `IsInWaterSensor`.
pub(super) fn sense_water(m: &mut Mob) {
    m.extra.frog.in_water = m.fluids.in_water;
}

/// `FrogAttackablesSensor`.
pub(super) fn sense_attackables(m: &mut Mob, w: &MobWorld) {
    let unreachable: Vec<u32> = m.extra.frog.unreachable.iter().map(|u| u.0).collect();
    let eye = Vec3::new(m.position.x, m.eye_y(), m.position.z);
    let nearest = w
        .nearby(m.position, DETECTION_DISTANCE, |s| {
            can_eat(s.species, s.size)
                && !unreachable.contains(&s.id)
                && w.line_of_sight(eye, Vec3::new(s.position.x, s.eye_y, s.position.z))
        })
        .find(|s| s.position.distance_squared(m.position) < DETECTION_DISTANCE * DETECTION_DISTANCE)
        .map(|s| s.id);
    m.extra.frog.nearest = nearest;
}

/// `Croak`: the pose sync::CROAKING for 60 ticks (the croak is the frog's ambient
/// sound). Started by the idle `RunOne`; it is the behaviour of the idle and lay
/// spawn activities.
#[derive(Clone, Debug)]
pub(super) struct Croak;
impl Goal for Croak {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn can_use(&mut self, m: &mut Mob, _cx: &mut Cx) -> bool {
        m.extra.pose == sync::STANDING && !mem(m).walk_target.present()
    }
    fn can_continue(&mut self, m: &mut Mob, _cx: &mut Cx) -> bool {
        m.extra.frog.croak < CROAK_TICKS
    }
    fn start(&mut self, m: &mut Mob, _cx: &mut Cx) {
        // `start` does nothing in a liquid, and the counter stays as it was.
        if m.fluids.in_water || m.fluids.in_lava {
            return;
        }
        m.extra.frog.croak = 0;
        set_pose(m, sync::CROAKING);
    }
    fn stop(&mut self, m: &mut Mob, _cx: &mut Cx) {
        set_pose(m, sync::STANDING);
    }
    fn tick(&mut self, m: &mut Mob, _cx: &mut Cx) {
        m.extra.frog.croak += 1;
    }
    fn every_tick(&self) -> bool {
        true
    }
}

/// `IS_PREGNANT`, set by `Frog.spawnChildFromBreeding` and erased once the
/// frog has laid its frogspawn.
const PREGNANT: &str = "is_pregnant";
pub fn is_pregnant(m: &Mob) -> bool {
    m.extra.memories.iter().any(|x| x.0 == PREGNANT)
}
pub fn set_pregnant(m: &mut Mob, on: bool) {
    let at = m.extra.memories.iter().position(|x| x.0 == PREGNANT);
    match (at, on) {
        (None, true) => m.extra.memories.push((PREGNANT, 1, i64::MAX)),
        (Some(i), false) => {
            m.extra.memories.remove(i);
        }
        _ => {}
    }
}

fn block_at(m: &Mob) -> [i32; 3] {
    [
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    ]
}
fn is_air(w: &MobWorld, p: [i32; 3]) -> bool {
    w.facts.is_air(w.world.block(p[0], p[1], p[2]))
}
fn empty(w: &MobWorld, p: [i32; 3]) -> bool {
    w.collision_bounds(p).is_none()
}
/// `TryFindLand.canStandOn`.
fn can_stand_on(w: &MobWorld, p: [i32; 3]) -> bool {
    empty(w, p) && w.top_face_full([p[0], p[1] - 1, p[2]])
}
/// `TryFindLandNearLiquid`'s `findFirst` test.
fn land_near_water(w: &MobWorld, p: [i32; 3]) -> bool {
    if !empty(w, p) || empty(w, [p[0], p[1] - 1, p[2]]) {
        return false;
    }
    HORIZONTAL.iter().any(|d| {
        let n = [p[0] + d[0], p[1], p[2] + d[1]];
        is_air(w, n) && w.fluid(n[0], n[1] - 1, n[2]) == Fluid::Water
    })
}
/// The walk target and look target both `TryFindLand` behaviours set.
fn set_land_target(m: &mut Mob, p: [i32; 3], speed: f32, close: i32) {
    let mm = mem(m);
    mm.look_target.set(Tracker::Block(p));
    mm.walk_target.set(WalkTarget {
        target: Tracker::Block(p),
        speed,
        close,
    });
}

/// `TryFindLand.create(range, speed)`: needs no attack target and no walk
/// target; in water it looks for land (blocks by Manhattan distance, off the
/// frog's column, with no fluid and a block to stand on) every 60 ticks. A try
/// before then pushes the next search 60 ticks on. `next_ok` is
/// `nextOkStartTime`.
pub(super) fn try_find_land(
    m: &mut Mob,
    w: &MobWorld,
    next_ok: &mut i64,
    range: i32,
    speed: f32,
) -> bool {
    if m.extra.frog.target.is_some() || mem(m).walk_target.present() {
        return false;
    }
    let at = block_at(m);
    if w.fluid(at[0], at[1], at[2]) != Fluid::Water {
        return false;
    }
    let now = w.game_time;
    if now < *next_ok {
        *next_ok = now + 60;
        return true;
    }
    let found = manhattan_box(at, range).into_iter().find(|p| {
        (p[0] != at[0] || p[2] != at[2])
            && w.fluid(p[0], p[1], p[2]) == Fluid::None
            && can_stand_on(w, *p)
    });
    if let Some(p) = found {
        set_land_target(m, p, speed, 1);
    }
    *next_ok = now + 60;
    true
}

/// `TryFindLandNearLiquid.create(8, 1.0, FROG_TRIES_TO_FIND_LAND_NEAR)`: the
/// same out of water, for land beside water, every 40 ticks.
pub(super) fn try_find_land_near_liquid(m: &mut Mob, w: &MobWorld, next_ok: &mut i64) -> bool {
    if m.extra.frog.target.is_some() || mem(m).walk_target.present() {
        return false;
    }
    let at = block_at(m);
    if w.fluid(at[0], at[1], at[2]) == Fluid::Water {
        return false;
    }
    let now = w.game_time;
    if now < *next_ok {
        *next_ok = now + 40;
        return true;
    }
    let found = manhattan_box(at, 8)
        .into_iter()
        .find(|p| (p[0] != at[0] || p[2] != at[2]) && land_near_water(w, *p));
    if let Some(p) = found {
        set_land_target(m, p, 1.0, 0);
    }
    *next_ok = now + 40;
    true
}

/// `TryLaySpawnOnFluidNearLand.create(FROGSPAWN)`: with a walk target and
/// `IS_PREGNANT`, on the ground out of water, frogspawn on the first
/// neighbouring water or `supports_frogspawn` block with air above it.
pub(super) fn try_lay_spawn(m: &mut Mob, cx: &mut Cx) -> bool {
    if m.extra.frog.target.is_some() || !mem(m).walk_target.present() || !is_pregnant(m) {
        return false;
    }
    if m.fluids.in_water || !m.on_ground {
        return false;
    }
    let below = [
        m.position.x.floor() as i32,
        m.position.y.floor() as i32 - 1,
        m.position.z.floor() as i32,
    ];
    for d in HORIZONTAL {
        let rel = [below[0] + d[0], below[1], below[2] + d[1]];
        // `getFaceShape(UP).isEmpty()`: nothing reaches the top of the block.
        let open_top =
            cx.w.collision_bounds(rel)
                .is_none_or(|b| b.max_position().y < 1.0);
        let supports = cx.w.fluid(rel[0], rel[1], rel[2]) == Fluid::Water
            || cx.w.has_tag(
                "minecraft:supports_frogspawn",
                cx.w.block_name(rel[0], rel[1], rel[2]),
            );
        let spawn = [rel[0], rel[1] + 1, rel[2]];
        if open_top && supports && is_air(cx.w, spawn) {
            if let Some(state) = cx.w.registry.default_state("minecraft:frogspawn") {
                cx.events.push(MobEvent::SetBlock { pos: spawn, state });
                cx.events.push(MobEvent::GameEvent {
                    event: "minecraft:block_place",
                    at: [
                        f64::from(spawn[0]) + 0.5,
                        f64::from(spawn[1]) + 0.5,
                        f64::from(spawn[2]) + 0.5,
                    ],
                    source: m.id,
                    state,
                });
            }
            // level.playSound(null, body, FROG_LAY_SPAWN, BLOCKS, 1, 1).
            cx.events.push(MobEvent::bound_sound(
                m,
                "entity.frog.lay_spawn",
                SoundSource::Blocks,
                1.0,
                1.0,
            ));
            set_pregnant(m, false);
            return true;
        }
    }
    true
}
/// `Direction.Plane.HORIZONTAL` as x and z steps: north, east, south, west.
pub(super) const HORIZONTAL: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
/// `BlockPos.withinManhattan`: nearest first, then x, y, and +z before -z.
pub(super) fn manhattan_box(o: [i32; 3], range: i32) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    for depth in 0..=range * 3 {
        let max_x = range.min(depth);
        for x in -max_x..=max_x {
            let max_y = range.min(depth - x.abs());
            for y in -max_y..=max_y {
                let z = depth - x.abs() - y.abs();
                if z <= range {
                    out.push([o[0] + x, o[1] + y, o[2] + z]);
                    if z != 0 {
                        out.push([o[0] + x, o[1] + y, o[2] - z]);
                    }
                }
            }
        }
    }
    out
}

/// `StartAttacking.create(canAttack, NEAREST_ATTACKABLE)` of the idle, swim
/// and lay spawn activities: `ATTACK_TARGET` is the nearest attackable, once
/// the brain's target hooks let the frog take it, and the walk target goes.
pub(super) fn start_attacking(m: &mut Mob, cx: &mut Cx) -> bool {
    // `canAttack`: not breeding; and no attack target already.
    if mem(m).breed_target.present() || m.extra.frog.target.is_some() {
        return false;
    }
    let Some(id) = m.extra.frog.nearest else {
        return false;
    };
    let Some(t) = cx.w.seen(id).filter(|t| t.alive) else {
        m.extra.frog.nearest = None;
        return false;
    };
    // A frog can never accept a player. Hide the mutable player
    // selection from this proposal so a rejected player redirect cannot
    // change who the remainder of the mob tick sees.
    let change = if cx.w.mob_hooks.is_none() {
        super::TargetChange::Set(Some(Who::Mob(t.id)))
    } else {
        let mut target_world = cx.w.clone();
        target_world.player = cx.w.selected_player().cloned();
        target_world.player_identity = cx.w.selected_player_identity();
        target_world.player_selection = None;
        target_world.change_target(m, true, Some(Who::Mob(t.id)))
    };
    let super::TargetChange::Set(Some(Who::Mob(id))) = change else {
        return false;
    };
    m.extra.frog.target = Some(id);
    mem(m).walk_target.clear();
    true
}

/// `StopAttackingIfTargetInvalid.create()`: a dead or gone target is
/// forgotten.
pub(super) fn stop_attacking_if_invalid(m: &mut Mob, cx: &mut Cx) -> bool {
    let Some(id) = m.extra.frog.target else {
        return false;
    };
    if cx.w.seen(id).is_some_and(|t| t.alive) {
        return false;
    }
    m.extra.frog.target = None;
    true
}

/// `ShootTongue`: walk up to the attack target (a small slime or magma cube),
/// shoot the tongue, pull it in and eat it.
#[derive(Clone, Debug)]
pub(super) struct ShootTongue;
impl Goal for ShootTongue {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        // `IS_PANICKING` must be absent.
        let Some(id) = m.extra.frog.target else {
            return false;
        };
        if m.ai.panicking {
            return false;
        }
        let Some(t) = cx
            .w
            .seen(id)
            .filter(|t| t.alive && can_eat(t.species, t.size))
        else {
            return false;
        };
        // canPathfindToTarget: a path that ends at the target.
        let at = [t.position.x, t.position.y, t.position.z];
        let ends = m
            .create_path_to_entity(cx.w, at, 0)
            .is_some_and(|p| f64::from(p.dist_to_target) < EATING_DISTANCE);
        if !ends {
            // The target is forgotten and not taken for 100 ticks.
            let f = &mut m.extra.frog;
            f.nearest = None;
            f.target = None;
            if !f.unreachable.iter().any(|u| u.0 == id) {
                if f.unreachable.len() == 5 {
                    f.unreachable.remove(0);
                }
                f.unreachable.push((id, 100));
            }
            return false;
        }
        true
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let f = &m.extra.frog;
        f.phase != Phase::Done
            && f.time <= TONGUE_TIME_OUT
            && !m.ai.panicking
            && f.target
                .is_some_and(|id| cx.w.seen(id).is_some_and(|t| t.alive))
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(t) = m.extra.frog.target.and_then(|id| cx.w.seen(id)) else {
            return;
        };
        let at = [t.position.x, t.position.y, t.position.z];
        let eye = [t.position.x, t.eye_y, t.position.z];
        m.look_at(eye);
        m.extra.frog.tongue_target = m.extra.frog.target;
        m.nav_move_to_entity(cx.w, at, 0, 2.0);
        let f = &mut m.extra.frog;
        f.path_counter = 10;
        f.time = 0;
        f.phase = Phase::MoveToTarget;
    }
    fn stop(&mut self, m: &mut Mob, _cx: &mut Cx) {
        let f = &mut m.extra.frog;
        // `ShootTongue.stop` erases brain memory directly, independently
        // of `StartAttacking` / `StopAttackingIfTargetInvalid` hooks.
        f.target = None;
        f.tongue_target = None;
        f.phase = Phase::Done;
        set_pose(m, sync::STANDING);
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(id) = m.extra.frog.target else {
            return;
        };
        let Some(t) = cx.w.seen(id) else {
            return;
        };
        let (at, pos) = ([t.position.x, t.position.y, t.position.z], t.position);
        m.look_at([t.position.x, t.eye_y, t.position.z]);
        m.extra.frog.tongue_target = Some(id);
        m.extra.frog.time += 1;
        match m.extra.frog.phase {
            Phase::MoveToTarget => {
                if pos.distance_squared(m.position) < EATING_DISTANCE * EATING_DISTANCE {
                    // level.playSound(null, body, tongueSound, NEUTRAL, 2, 1).
                    cx.events.push(MobEvent::bound_sound(
                        m,
                        "entity.frog.tongue",
                        SoundSource::Neutral,
                        2.0,
                        1.0,
                    ));
                    set_pose(m, sync::USING_TONGUE);
                    // `target.setDeltaMovement(vectorTo(frog).normalize().scale(0.75))`: the
                    // target's own motion is replaced, and it is drawn in.
                    let pull = m.position.subtract(pos).normalize().scale(0.75);
                    cx.events.push(MobEvent::SetVelocity {
                        target: Who::Mob(id),
                        velocity: [pull.x, pull.y, pull.z],
                    });
                    let f = &mut m.extra.frog;
                    f.eat_timer = 0;
                    f.phase = Phase::CatchAnimation;
                } else if m.extra.frog.path_counter <= 0 {
                    m.nav_move_to_entity(cx.w, at, 0, 2.0);
                    m.extra.frog.path_counter = 10;
                } else {
                    m.extra.frog.path_counter -= 1;
                }
            }
            Phase::CatchAnimation => {
                let f = &mut m.extra.frog;
                f.eat_timer += 1;
                if f.eat_timer > 6 {
                    f.phase = Phase::EatAnimation;
                    // eatEntity.
                    cx.events.push(MobEvent::bound_sound(
                        m,
                        "entity.frog.eat",
                        SoundSource::Neutral,
                        2.0,
                        1.0,
                    ));
                    super::ai::do_hurt_target(m, Who::Mob(id), cx);
                    // `if (!target.isAlive()) target.remove(KILLED)`: no death animation.
                    cx.events.push(MobEvent::RemoveKilled { target: id });
                }
            }
            Phase::EatAnimation => {
                let f = &mut m.extra.frog;
                if f.eat_timer >= 10 {
                    f.phase = Phase::Done;
                } else {
                    f.eat_timer += 1;
                }
            }
            Phase::Done => {}
        }
    }
}

/// `CountDownCooldownTicks.tick` of `LONG_JUMP_COOLDOWN_TICKS`: one less each
/// tick, erased the tick after it reaches 0 (the core activity of a frog and
/// a goat).
pub(super) fn count_down_cooldown(m: &mut Mob) {
    match jump_cooldown(m) {
        Some(t) if t > 0 => set_jump_cooldown(m, Some(t - 1)),
        Some(_) => set_jump_cooldown(m, None),
        None => {}
    }
}
/// `LONG_JUMP_COOLDOWN_TICKS`.
pub(crate) fn jump_cooldown(m: &Mob) -> Option<i64> {
    m.extra
        .memories
        .iter()
        .find(|x| x.0 == LONG_JUMP_COOLDOWN)
        .map(|x| x.1)
}
pub(crate) fn set_jump_cooldown(m: &mut Mob, ticks: Option<i64>) {
    let memories = &mut m.extra.memories;
    let at = memories.iter().position(|x| x.0 == LONG_JUMP_COOLDOWN);
    match (at, ticks) {
        (Some(i), Some(t)) => memories[i].1 = t,
        (Some(i), None) => {
            memories.remove(i);
        }
        (None, Some(t)) => memories.push((LONG_JUMP_COOLDOWN, t, i64::MAX)),
        (None, None) => {}
    }
}
/// `TIME_BETWEEN_LONG_JUMPS.sample` from the level's random, divided as the
/// caller asks.
fn sample_cooldown(m: &mut Mob, lr: &mut LegacyRandom, div: i32) {
    let (lo, hi) = if m.species() == Species::Goat {
        GOAT_TIME_BETWEEN_LONG_JUMPS
    } else {
        TIME_BETWEEN_LONG_JUMPS
    };
    let t = lr.next_int_between_inclusive(lo, hi).unwrap_or(lo) / div;
    set_jump_cooldown(m, Some(i64::from(t)));
}

/// The jump sound each activity hands to `LongJumpToRandomPos`: the frog's, or
/// the goat's, which has a screaming one.
fn jump_sound(m: &Mob) -> &'static str {
    match (m.species(), m.extra.screaming) {
        (Species::Goat, true) => "entity.goat.screaming.long_jump",
        (Species::Goat, false) => "entity.goat.long_jump",
        _ => "entity.frog.long_jump",
    }
}

/// `LivingEntity.setDiscardFriction`: the flag lives in the frog's or the
/// goat's state.
fn set_discard_friction(m: &mut Mob, on: bool) {
    if m.species() == Species::Goat {
        m.extra.goat.discard_friction = on;
    } else {
        m.extra.frog.discard_friction = on;
    }
}
/// `Entity.setPose` to or from a long jump: a goat's box changes with it.
fn set_jump_pose(m: &mut Mob, w: &MobWorld, id: u8) {
    if m.species() == Species::Goat {
        super::goat::set_pose(m, w, id);
    } else {
        set_pose(m, id);
    }
}

/// `FrogAi.isAcceptableLandingSpot`: no fluid in the target, below or above
/// it; a lily pad or big dripleaf (`frog_prefer_jump_to`) or a trapdoor; else
/// `LongJumpToRandomPos.defaultAcceptableLandingSpot`: a solid block below
/// and a path type the frog walks without a malus.
fn acceptable_landing(m: &Mob, w: &MobWorld, [x, y, z]: [i32; 3]) -> bool {
    use super::path::PathType;
    if (y - 1..=y + 1).any(|y| w.fluid(x, y, z) != crate::blocks::Fluid::None) {
        return false;
    }
    let preferred = |y| w.has_tag("minecraft:frog_prefer_jump_to", w.block_name(x, y, z));
    if preferred(y) || preferred(y - 1) {
        return true;
    }
    let here = w.path_type_static(x, y, z);
    if here == PathType::Trapdoor
        || w.facts.is_air(w.world.block(x, y, z))
            && w.path_type_static(x, y - 1, z) == PathType::Trapdoor
    {
        return true;
    }
    default_acceptable_landing(m, w, [x, y, z])
}
/// `LongJumpToRandomPos.defaultAcceptableLandingSpot`: a solid block below and
/// a path type the mob walks without a malus.
fn default_acceptable_landing(m: &Mob, w: &MobWorld, [x, y, z]: [i32; 3]) -> bool {
    w.facts.get(w.world.block(x, y - 1, z)).solid_render
        && m.malus(w.path_type_static(x, y, z)) == 0.0
}

/// `LongJumpToRandomPos.PossibleJump`: a block and its weight.
type Candidate = ([i32; 3], i32);

/// `FrogAi`'s long jump activity: `LongJumpToPreferredBlock` looks for a spot
/// it cannot walk to, faces it for 40 ticks and leaps; `LongJumpMidJump`
/// holds the pose until it lands. One goal here, like the brain activity
/// that keeps the idle croak and stroll from starting meanwhile.
#[derive(Clone, Debug, Default)]
pub(super) struct LongJump {
    candidates: Vec<Candidate>,
    not_preferred: Vec<Candidate>,
    wanting_preferred: bool,
    initial: Option<Vec3>,
    chosen: Option<Vec3>,
    /// The chosen spot, looked at while preparing.
    look: [f64; 3],
    tries: i32,
    /// Ticks since the jump was chosen.
    prepare: i32,
    /// `LONG_JUMP_MID_JUMP`, and the ticks `LongJumpMidJump` has run.
    mid_jump: bool,
    in_air: Option<i32>,
}
impl LongJump {
    /// `WeightedRandom.getRandomItem` with the level's random over the
    /// candidates, removed once drawn.
    fn draw(&mut self, lr: &mut LegacyRandom) -> Option<Candidate> {
        let total: i32 = self.candidates.iter().map(|c| c.1).sum();
        if total == 0 {
            return None;
        }
        let mut pick = lr.next_int_bound(total).unwrap_or(0);
        let i = self.candidates.iter().position(|c| {
            pick -= c.1;
            pick < 0
        })?;
        Some(self.candidates.remove(i))
    }
    /// `LongJumpToPreferredBlock.getJumpCandidate`: while it wants them, the
    /// first drawn spot over a preferred block, else the first one passed over.
    fn candidate(&mut self, w: &MobWorld, lr: &mut LegacyRandom) -> Option<Candidate> {
        if !self.wanting_preferred {
            return self.draw(lr);
        }
        while !self.candidates.is_empty() {
            if let Some(c) = self.draw(lr) {
                let [x, y, z] = c.0;
                if w.has_tag("minecraft:frog_prefer_jump_to", w.block_name(x, y - 1, z)) {
                    return Some(c);
                }
                self.not_preferred.push(c);
            }
        }
        (!self.not_preferred.is_empty()).then(|| self.not_preferred.remove(0))
    }
    /// `LongJumpToRandomPos.pickCandidate`: the first acceptable spot it can
    /// reach in an arc but not by an 8 block path.
    fn pick(&mut self, m: &mut Mob, w: &MobWorld, lr: &mut LegacyRandom) {
        while !self.candidates.is_empty() {
            let Some((target, _)) = self.candidate(w, lr) else {
                continue;
            };
            let here = [m.position.x.floor() as i32, m.position.z.floor() as i32];
            // GoatAi's landing spot is LongJumpToRandomPos's default; the
            // frog's is FrogAi.isAcceptableLandingSpot.
            let acceptable = if m.species() == Species::Goat {
                default_acceptable_landing(m, w, target)
            } else {
                acceptable_landing(m, w, target)
            };
            if here == [target[0], target[2]] || !acceptable {
                continue;
            }
            let centre = Vec3::new(
                f64::from(target[0]) + 0.5,
                f64::from(target[1]) + 0.5,
                f64::from(target[2]) + 0.5,
            );
            let Some(v) = optimal_jump_vector(m, w, centre) else {
                continue;
            };
            self.look = [centre.x, centre.y, centre.z];
            let walkable = m
                .create_path_raw(w, &[target], 0, 8.0)
                .is_some_and(|p| p.reached);
            if !walkable {
                self.chosen = Some(v);
                self.prepare = 0;
                return;
            }
        }
    }
    /// `LongJumpMidJump.stop` (a goat lands with `entity.goat.step`, a frog
    /// with `entity.frog.step`), or the end of a search.
    fn end(&mut self, m: &mut Mob, cx: &mut Cx) {
        if self.mid_jump {
            if m.on_ground {
                // multiply(0.1F, 1.0, 0.1F): the float widened.
                let slow = f64::from(0.1_f32);
                m.velocity = m.velocity.multiply(Vec3::new(slow, 1.0, slow));
                let landing = if m.species() == Species::Goat {
                    "entity.goat.step"
                } else {
                    "entity.frog.step"
                };
                cx.events.push(MobEvent::bound_sound(
                    m,
                    landing,
                    mod_api::SoundSource::Neutral,
                    2.0,
                    1.0,
                ));
            }
            set_jump_pose(m, cx.w, sync::STANDING);
            sample_cooldown(m, cx.level.random(), 1);
        }
        set_discard_friction(m, false);
        m.extra.frog.mid_jump = false;
        *self = Self::default();
    }
}
/// `LongJumpToRandomPos.start`'s candidates: every block of the box but the
/// mob's own, weighted by the squared distance, in `betweenClosed` order.
fn candidates([x, y, z]: [i32; 3], width: i32, height: i32) -> Vec<Candidate> {
    let mut out = Vec::new();
    for dz in -width..=width {
        for dy in -height..=height {
            for dx in -width..=width {
                if (dx, dy, dz) != (0, 0, 0) {
                    out.push(([x + dx, y + dy, z + dz], dx * dx + dy * dy + dz * dz));
                }
            }
        }
    }
    out
}
/// `LongJumpToRandomPos.calculateOptimalJumpVector`: the angles shuffled, the
/// first arc that reaches the target clear of blocks. `Collections.shuffle`
/// without a random draws from an unseeded one, not the mob's or the level's.
fn optimal_jump_vector(m: &Mob, w: &MobWorld, target: Vec3) -> Option<Vec3> {
    let mut random = crate::random_source::create();
    let mut angles = ALLOWED_ANGLES;
    for i in (2..=angles.len()).rev() {
        let j = random.next_int_bound(i as i32).unwrap_or(0) as usize;
        angles.swap(i - 1, j);
    }
    let max = (m.attrs.jump_strength * f64::from(JUMP_VELOCITY_MULTIPLIER)) as f32;
    angles
        .iter()
        .find_map(|&a| super::breeze::jump_vector(m, target, max, a, Some(w)))
}
impl Goal for LongJump {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn interruptable(&self) -> bool {
        !self.mid_jump
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        // A frog's activity requirements (no cooldown, out of the water memory,
        // no tempting player or breed target) are the brain's. A goat has none
        // here: its check below sets the cooldown instead.
        if m.species() == Species::Goat && jump_cooldown(m).is_some() {
            return false;
        }
        // GoatAi.initLongJumpActivity's other memories, and the activity
        // order of GoatAi.updateActivity.
        if m.species() == Species::Goat && !super::goat::long_jump_activity(m, cx.w) {
            return false;
        }
        // checkExtraStartConditions: on the ground, not in water or lava or
        // on honey.
        let (x, y, z) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        let can = m.on_ground
            && !m.fluids.in_water
            && !m.fluids.in_lava
            && cx.w.block_name(x, y, z) != "minecraft:honey_block";
        if !can {
            sample_cooldown(m, cx.level.random(), 2);
        }
        can
    }
    fn start(&mut self, m: &mut Mob, _cx: &mut Cx) {
        // LongJumpToRandomPos.start.
        let (x, y, z) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        let (width, height) = if m.species() == Species::Goat {
            (GOAT_JUMP_WIDTH, GOAT_JUMP_HEIGHT)
        } else {
            (JUMP_WIDTH, JUMP_HEIGHT)
        };
        *self = Self {
            candidates: candidates([x, y, z], width, height),
            tries: FIND_JUMP_TRIES,
            initial: Some(m.position),
            ..Self::default()
        };
        // LongJumpToPreferredBlock.start: the frog's; a goat's activity is a
        // plain LongJumpToRandomPos, which draws nothing from the mob's random.
        if m.species() != Species::Goat {
            self.wanting_preferred = m.random.next_float() < PREFERRED_BLOCKS_CHANCE;
        }
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if self.mid_jump {
            // LongJumpMidJump starts the tick after the leap and runs until
            // the mob is on the ground again, for 100 ticks at most.
            match self.in_air {
                None => {
                    self.in_air = Some(0);
                    set_discard_friction(m, true);
                    set_jump_pose(m, cx.w, sync::LONG_JUMPING);
                }
                Some(t) => self.in_air = Some(t + 1),
            }
            return !m.on_ground && self.in_air.is_some_and(|t| t <= MID_JUMP_DURATION);
        }
        let valid = self.initial == Some(m.position)
            && self.tries > 0
            && !m.fluids.in_water
            && !(self.chosen.is_none() && self.candidates.is_empty());
        if !valid {
            sample_cooldown(m, cx.level.random(), 2);
        }
        valid
    }
    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.end(m, cx);
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if self.mid_jump {
            return;
        }
        let Some(jump) = self.chosen else {
            self.tries -= 1;
            self.pick(m, cx.w, cx.level.random());
            return;
        };
        m.look_at(self.look);
        self.prepare += 1;
        if self.prepare < PREPARE_JUMP_DURATION {
            return;
        }
        // The leap: the body's facing, no friction, the arc's velocity
        // lengthened by jump boost.
        m.yaw = m.body_yaw;
        set_discard_friction(m, true);
        let len = jump.length();
        let boost = m
            .extra
            .effects
            .iter()
            .find(|e| e.id == "minecraft:jump_boost")
            .map_or(0.0, |e| f64::from(0.1_f32 * (e.amplifier + 1) as f32));
        m.velocity = jump.scale((len + boost) / len);
        self.mid_jump = true;
        if m.species() == Species::Frog {
            m.extra.frog.mid_jump = true;
        }
        // level.playSound(null, body, getJumpSound(body), NEUTRAL, 1, 1).
        let sound = MobEvent::bound_sound(m, jump_sound(m), SoundSource::Neutral, 1.0, 1.0);
        cx.events.push(sound);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sunrise_core::random::LegacyRandom;

    fn frog() -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let k = std::sync::Arc::make_mut(&mut kind);
        k.id = "minecraft:frog".into();
        k.species = Species::Frog;
        Mob::new(1, kind, Vec3::new(0.5, 0.0, 0.5), 0.0)
    }

    #[test]
    fn swim_activity_replaces_float() {
        let mut m = frog();
        register(&mut m);
        // `FrogAi.initCoreActivity` has no `Swim`: the swim activity is a brain one.
        assert!(!m.ai.goals.has("Float"));
        assert!(m.extra.animal_brain.is_some());
        assert!(!m.ai.nav.can_float, "no FloatGoal, so the navigation cannot float");
    }

    fn activity(m: &mut Mob) -> super::super::animal_brain::Act {
        super::super::animal_brain::update_frog_activity(m);
        m.extra.animal_brain.as_ref().unwrap().activity
    }

    #[test]
    fn the_activity_follows_the_memories_in_frog_ais_order() {
        use super::super::animal_brain::Act;
        let mut m = frog();
        register(&mut m);
        // `IS_IN_WATER` absent: no cooldown, so the long jump comes before idle.
        assert_eq!(activity(&mut m), Act::LongJump);
        set_jump_cooldown(&mut m, Some(5));
        assert_eq!(activity(&mut m), Act::Idle);
        m.extra.frog.in_water = true;
        assert_eq!(activity(&mut m), Act::Swim);
        // `updateActivity` tries TONGUE and LAY_SPAWN first.
        set_pregnant(&mut m, true);
        assert_eq!(activity(&mut m), Act::LaySpawn);
        m.extra.frog.target = Some(2);
        assert_eq!(activity(&mut m), Act::Tongue);
        // Idle, swim and lay spawn wait for the landing.
        m.extra.frog.mid_jump = true;
        m.extra.frog.target = None;
        set_pregnant(&mut m, false);
        assert_eq!(activity(&mut m), Act::Tongue, "no activity is valid: it stays");
    }

    #[test]
    fn sensor_delays_draw_five_times() {
        let mut m = frog();
        m.random.set_seed(179);
        let mut twin = LegacyRandom::new(179);
        super::super::animal_brain::init(&mut m, 1);
        for _ in 0..5 {
            twin.next_int_bound(20).unwrap();
        }
        assert_eq!(m.random.state(), twin.state());
    }

    #[test]
    #[ignore = "daily"]
    fn tongue_acquisition_honors_brain_hooks_and_edible_redirects() {
        use crate::mobs::{MobHooks, PlayerView, TargetChange};
        use std::cell::Cell;
        struct Hook(TargetChange, Cell<usize>);
        impl MobHooks for Hook {
            fn enderman_anger_suppressed(&self, _: u32, _: &PlayerView) -> bool {
                false
            }
            fn can_grief(&self, _: u32, rule: bool) -> bool {
                rule
            }
            fn grief_hooked(&self) -> bool {
                false
            }
            fn living_visibility(&self, _: &PlayerView, _: u32, v: f64) -> f64 {
                v
            }
            fn living_change_target(
                &self,
                _: &mut Mob,
                world: &MobWorld<'_>,
                brain: bool,
                proposed: Option<Who>,
            ) -> TargetChange {
                assert!(brain);
                assert_eq!(proposed, Some(Who::Mob(2)));
                self.1.set(self.1.get() + 1);
                // Mirror the native host selecting a redirected player.
                if self.0 == TargetChange::Set(Some(Who::Player))
                    && let Some(selection) = world.player_selection
                {
                    assert!(selection.select(100));
                }
                assert_eq!(world.selected_player_identity().map(|p| p.1), Some(99));
                self.0
            }
        }
        let mut others = Vec::new();
        assert!(crate::mobs::world::with_test_world(&[], |_, types| {
            for (id, name, size) in [
                (2, "slime", 1),
                (3, "magma_cube", 1),
                (4, "slime", 2),
                (5, "zombie", 1),
            ] {
                let kind = std::sync::Arc::new(types.types[&format!("minecraft:{name}")].clone());
                let mut seen = Mob::new(id, kind, Vec3::new(1.5, 64.0, 0.5), 0.0)
                    .seen(None, &crate::items::Items::empty());
                seen.size = size;
                others.push(seen);
            }
        }));
        assert!(crate::mobs::world::with_built_test_world(
            &others,
            |world, registry| {
                for x in -1..=0 {
                    for z in -1..=0 {
                        world.chunks.insert(
                            (x, z),
                            std::sync::Arc::new(crate::chunk::Chunk::empty(x, z)),
                        );
                    }
                }
                let stone = registry.default_state("minecraft:stone").unwrap();
                for x in -4..=4 {
                    for z in -4..=4 {
                        world.set(x, 63, z, stone).unwrap();
                    }
                }
            },
            |world, types| {
                for (with_hook, outcome, expected) in [
                    (false, TargetChange::Keep, Some(2)),
                    (true, TargetChange::Keep, None),
                    (true, TargetChange::Set(None), None),
                    (true, TargetChange::Set(Some(Who::Mob(2))), Some(2)),
                    (true, TargetChange::Set(Some(Who::Mob(3))), Some(3)),
                    (true, TargetChange::Set(Some(Who::Mob(4))), None),
                    (true, TargetChange::Set(Some(Who::Mob(5))), None),
                    (true, TargetChange::Set(Some(Who::Mob(999))), None),
                    (true, TargetChange::Set(Some(Who::Player)), None),
                ] {
                    let hook = Hook(outcome, Cell::new(0));
                    let selection = crate::mobs::player_selection::PlayerSelection::new(
                        [99, 100]
                            .into_iter()
                            .map(|entity_id| crate::mobs::player_selection::TickPlayer {
                                entity_id,
                                connection: Some(entity_id),
                                view: crate::mobs::world::test_player(Vec3::new(3.5, 64.0, 0.5)),
                            })
                            .collect(),
                        Some(99),
                    )
                    .unwrap();
                    let mut world = world.clone();
                    world.player_selection = Some(&selection);
                    world.mob_hooks = with_hook.then_some(&hook as &dyn MobHooks);
                    let kind = std::sync::Arc::new(types.types["minecraft:frog"].clone());
                    let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
                    register(&mut m);
                    m.on_ground = true;
                    m.extra.frog.nearest = Some(2);
                    let mut events = Vec::new();
                    let mut fixture = crate::health::TestLevel::new(0);
                    let mut cx = Cx {
                        w: &world,
                        level: &mut fixture.level(),
                        events: &mut events,
                    };
                    assert_eq!(
                        start_attacking(&mut m, &mut cx) && ShootTongue.can_use(&mut m, &mut cx),
                        expected.is_some(),
                        "{outcome:?}"
                    );
                    assert_eq!(attack_target(&m), expected.map(Who::Mob));
                    assert_eq!(hook.1.get(), usize::from(with_hook));
                    assert_eq!(selection.selected().unwrap().entity_id, 99);
                    assert!(!selection.has_switched());
                    if expected.is_some() {
                        ShootTongue.start(&mut m, &mut cx);
                        assert_eq!(m.extra.frog.tongue_target, expected);
                        ShootTongue.stop(&mut m, &mut cx);
                        assert_eq!(attack_target(&m), None);
                        assert_eq!(m.extra.frog.tongue_target, None);
                        assert_eq!(
                            hook.1.get(),
                            usize::from(with_hook),
                            "ShootTongue.stop erases memory directly"
                        );
                    }
                }
            }
        ));
    }

    /// `Frog.travelInWater`: the move uses `getSpeed` (the speed the move
    /// control stored, here 0.25, not the movement attribute 1.0 and not the
    /// turtle's 0.1), then the whole motion is scaled by 0.9. Out of water
    /// this path does not run.
    #[test]
    #[ignore = "daily"]
    fn water_travel_uses_get_speed_and_scales_by_nine_tenths() {
        let ran = crate::mobs::world::with_test_world(&[], |w, _| {
            let mut m = frog();
            m.attrs.speed = 1.0;
            m.ai.speed = 0.25;
            m.yaw = 0.0;
            m.velocity = Vec3::new(1.0, -0.5, 0.125);
            let mut test_level = crate::health::TestLevel::new(0);
            m.fluids.in_water = true;
            m.position = Vec3::new(0.5, 80.0, 0.5);
            m.travel(
                w,
                &mut test_level.level(),
                Vec3::new(0.0, 0.0, 1.0),
                &mut Vec::new(),
            );
            // (1, -0.5, 0.125) + (0, 0, getSpeed 0.25) = (1, -0.5, 0.375),
            // then * 0.9 = (0.9, -0.45, 0.3375).
            assert!((m.velocity.x - 0.9).abs() < 1.0e-9, "{}", m.velocity.x);
            assert!((m.velocity.y - -0.45).abs() < 1.0e-9, "{}", m.velocity.y);
            assert!((m.velocity.z - 0.3375).abs() < 1.0e-9, "{}", m.velocity.z);
        });
        // Without a test world the closure does not run; there is nothing else to check.
        assert!(ran, "vanilla data is missing");
    }

    /// `Frog.updateWalkAnimation`: target `min(distance * 25, 1)`, 0 while
    /// the jump animation is started, eased by 0.4 (`WalkAnimationState.update`).
    #[test]
    fn a_frog_walks_its_legs_at_the_vanilla_rate() {
        let mut m = frog();
        m.previous = m.position;
        m.position.x += 0.02;
        m.update_walk_animation();
        assert!((m.limb_speed - 0.5 * 0.4).abs() < 1e-6, "{}", m.limb_speed);
        assert!((m.limb_position - m.limb_speed).abs() < 1e-6);

        let mut m = frog();
        m.previous = m.position;
        m.position.x += 0.1;
        m.update_walk_animation();
        assert!((m.limb_speed - 0.4).abs() < 1e-6, "{}", m.limb_speed);

        let mut m = frog();
        m.anim.states[JUMP].start(0.0);
        m.previous = m.position;
        m.position.x += 0.02;
        m.update_walk_animation();
        assert_eq!(m.limb_speed, 0.0);
    }

    /// `LongJumpToRandomPos.start`: the 9 x 5 x 9 box without the frog's own
    /// block, x fastest, then y, then z, each weighted by its squared distance.
    #[test]
    fn the_candidates_cover_the_jump_box_in_vanilla_order() {
        let c = candidates([10, 64, -3], JUMP_WIDTH, JUMP_HEIGHT);
        assert_eq!(c.len(), 9 * 5 * 9 - 1);
        assert_eq!(c[0], ([6, 62, -7], 16 + 4 + 16));
        assert_eq!(c[1], ([7, 62, -7], 9 + 4 + 16));
        assert_eq!(c[9], ([6, 63, -7], 16 + 1 + 16));
        assert_eq!(c[45], ([6, 62, -6], 16 + 4 + 9));
        assert!(!c.iter().any(|(p, _)| *p == [10, 64, -3]));
        assert_eq!(c.last().unwrap().0, [14, 66, 1]);
    }

    /// `WeightedRandom.getRandomItem`: `nextInt(total)` walked down the
    /// list, and the drawn candidate is removed.
    #[test]
    fn a_draw_takes_the_weighted_candidate_out() {
        let mut j = LongJump {
            candidates: vec![([0, 0, 0], 1), ([1, 0, 0], 4), ([2, 0, 0], 9)],
            ..LongJump::default()
        };
        let mut r = LegacyRandom::new(7);
        let mut lr = r.clone();
        let pick = r.next_int_bound(14).unwrap();
        let want = if pick < 1 {
            0
        } else if pick < 5 {
            1
        } else {
            2
        };
        assert_eq!(j.draw(&mut lr).unwrap().0, [want, 0, 0]);
        assert_eq!(j.candidates.len(), 2);
        assert!(!j.candidates.iter().any(|c| c.0 == [want, 0, 0]));
    }

    /// `CountDownCooldownTicks`: the memory falls by one a tick and is gone
    /// the tick after it reached 0; the sample is 100..=140, halved after a
    /// failed search, drawn from the level's random and not the frog's.
    #[test]
    fn pregnancy_is_a_memory_that_the_laying_erases() {
        let mut m = frog();
        assert!(!is_pregnant(&m));
        set_pregnant(&mut m, true);
        set_pregnant(&mut m, true);
        assert!(is_pregnant(&m));
        assert_eq!(
            m.extra.memories.iter().filter(|x| x.0 == PREGNANT).count(),
            1
        );
        set_pregnant(&mut m, false);
        assert!(!is_pregnant(&m));
    }

    #[test]
    fn the_land_search_goes_out_by_manhattan_distance() {
        let all = manhattan_box([0, 0, 0], 8);
        assert_eq!(all[0], [0, 0, 0]);
        assert_eq!(all.len(), 17 * 17 * 17);
        // x, then y, and +z before -z, at distance 1.
        assert_eq!(
            all[1..7],
            [
                [-1, 0, 0],
                [0, -1, 0],
                [0, 0, 1],
                [0, 0, -1],
                [0, 1, 0],
                [1, 0, 0]
            ]
        );
    }

    #[test]
    fn the_long_jump_cooldown_counts_down_and_is_erased() {
        let mut m = frog();
        let own = m.random.state();
        sample_cooldown(&mut m, &mut LegacyRandom::new(3), 2);
        let t = jump_cooldown(&m).unwrap();
        assert!((50..=70).contains(&t));
        assert_eq!(m.random.state(), own);
        set_jump_cooldown(&mut m, Some(1));
        set_jump_cooldown(&mut m, Some(0));
        assert_eq!(jump_cooldown(&m), Some(0));
        set_jump_cooldown(&mut m, None);
        assert_eq!(jump_cooldown(&m), None);
        assert!(!m.extra.memories.iter().any(|x| x.0 == LONG_JUMP_COOLDOWN));
    }

    /// `FrogAi.initCoreActivity`: `AnimalPanic(2.0)`. A hurt frog starts the
    /// panic goal with vanilla's 2.0 speed multiplier, which `PathNavigation`
    /// stores for `SmoothSwimmingMoveControl` (`speedModifier * movement
    /// speed`, so 0.2 on land and 0.04 in water). The brain's temptation and
    /// long jump run at 1.25 and 1.0; 1.25 here would make a fleeing frog 60%
    /// slower than vanilla's.
    #[test]
    #[ignore = "daily"]
    fn a_hurt_frog_panics_at_vanillas_two_times_speed() {
        let ran = crate::mobs::world::with_built_test_world(
            &[],
            |world, registry| {
                for x in -2..=1 {
                    for z in -2..=1 {
                        world.chunks.insert(
                            (x, z),
                            std::sync::Arc::new(crate::chunk::Chunk::empty(x, z)),
                        );
                    }
                }
                let stone = registry.default_state("minecraft:stone").unwrap();
                for x in -8..=8 {
                    for z in -8..=8 {
                        world.set(x, 63, z, stone).unwrap();
                    }
                }
            },
            |w, types| {
                let kind = std::sync::Arc::new(types.types["minecraft:frog"].clone());
                let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
                register(&mut m);
                m.on_ground = true;
                // An environmental hit inside the 40 tick panic window
                // (`DamageTypeTags.PANIC_CAUSES`).
                m.last_damage = Some((
                    super::super::Damage::of(super::super::DamageKind::Freeze),
                    i64::from(m.age),
                ));
                let mut events = Vec::new();
                let mut fixture = crate::health::TestLevel::new(0);
                let mut cx = Cx {
                    w,
                    level: &mut fixture.level(),
                    events: &mut events,
                };
                let mut goals = std::mem::take(&mut m.ai.goals);
                goals.tick(&mut m, &mut cx);
                assert!(goals.is_running("Panic"), "the hurt frog should flee");
                m.ai.goals = goals;
                assert!(m.ai.panicking);
                assert_eq!(m.ai.nav.speed_modifier, 2.0);
            },
        );
        assert!(ran, "vanilla data is missing");
    }

    /// `Frog` keeps its pregnancy as the `IS_PREGNANT` brain memory, and
    /// `Brain.Packed` saves every memory: `mob_to_nbt`/`mob_from_nbt` are the
    /// pair `Game::mob_tree_nbt` and `Game::load_mob` go through. Sunrise read
    /// the memory back but wrote no `Brain` for a frog, so a frog made
    /// pregnant in play that unloaded with its chunk came back unpregnant and
    /// never laid its frogspawn.
    #[test]
    fn a_pregnant_frog_keeps_its_pregnancy_through_a_save() {
        use crate::entity_nbt::{mob_from_nbt, mob_to_nbt};
        use sunrise_core::nbt::Tag;
        let memory = |tag: &Tag| {
            tag.get("Brain")
                .and_then(|b| b.get("memories"))
                .and_then(|b| b.get("minecraft:is_pregnant"))
                .cloned()
        };
        let mut m = frog();
        set_pregnant(&mut m, true);
        let saved = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None, &Default::default());
        assert_eq!(
            memory(&saved),
            Some(Tag::compound([(
                "value",
                Tag::Compound(Default::default())
            )])),
            "the save names the pregnancy"
        );
        let types = std::collections::HashMap::from([(m.kind.id.clone(), m.kind.clone())]);
        let back = mob_from_nbt(&saved, &types, 2, None).unwrap();
        assert!(is_pregnant(&back), "the reloaded frog is pregnant");
        // A frog that laid its spawn saves no memory, even over a compound
        // that still names one.
        set_pregnant(&mut m, false);
        let saved = mob_to_nbt(
            &m,
            [1, 2, 3, 4],
            Some(&saved),
            None,
            |_| None,
            &Default::default(),
        );
        assert_eq!(memory(&saved), None, "a laid frog saves no pregnancy");
        let back = mob_from_nbt(&saved, &types, 3, None).unwrap();
        assert!(!is_pregnant(&back), "and does not come back pregnant");
    }

    /// `TryLaySpawnOnFluidNearLand`: the frogspawn is set, then
    /// `block_place` is posted from the frog at the new block, then the lay
    /// sound, and the pregnancy is erased.
    #[test]
    #[ignore = "daily"]
    fn laying_frogspawn_posts_block_place_from_the_frog() {
        let ran = crate::mobs::world::with_built_test_world(
            &[],
            |world, registry| {
                world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
                );
                let water = registry.default_state("minecraft:water").unwrap();
                world.set(8, 63, 7, water).unwrap();
            },
            |w, _| {
                let mut m = frog();
                m.position = Vec3::new(8.5, 64.0, 8.5);
                m.on_ground = true;
                set_pregnant(&mut m, true);
                super::super::animal_brain::init(&mut m, 1);
                mem(&mut m).walk_target.set(WalkTarget {
                    target: Tracker::Block([8, 64, 7]),
                    speed: 1.0,
                    close: 0,
                });
                let mut lr = crate::health::TestLevel::new(3);
                let mut events = Vec::new();
                let mut cx = Cx {
                    w,
                    level: &mut lr.level(),
                    events: &mut events,
                };
                assert!(try_lay_spawn(&mut m, &mut cx));
                let spawn = w.registry.default_state("minecraft:frogspawn").unwrap();
                assert!(
                    events
                        .iter()
                        .any(|e| matches!(e, MobEvent::SetBlock { pos: [8, 64, 7], state } if *state == spawn)),
                    "the frogspawn is set: {events:?}"
                );
                assert!(
                    events.iter().any(|e| matches!(
                        e,
                        MobEvent::GameEvent {
                            event: "minecraft:block_place",
                            at: [8.5, 64.5, 7.5],
                            source: 1,
                            state,
                        } if *state == spawn
                    )),
                    "the new frogspawn posts block_place from the frog: {events:?}"
                );
                assert!(!is_pregnant(&m), "laying erases the pregnancy");
            },
        );
        assert!(ran, "vanilla data is missing");
    }
}
