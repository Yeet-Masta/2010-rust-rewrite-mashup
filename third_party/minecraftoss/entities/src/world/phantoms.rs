//! Phantoms (26.3 `Phantom` and its goals).
//!
//! - A phantom flies to its `moveTargetPoint` under `PhantomMoveControl`:
//!   it turns at most 4 degrees a tick toward it, gathering speed up to 1.8
//!   while flying straight and slowing to 0.2 while turning, and turns
//!   about when it hits a wall.
//! - Without a target, or between dives, it circles an anchor 5 to 15
//!   blocks out (`PhantomCircleAroundAnchorGoal`), changing its height,
//!   reach and heading now and then and pulling up from the ground.
//! - With a target, every 8 to 11 seconds it re-anchors 20 to 39 blocks
//!   above the target and dives (`PhantomAttackStrategyGoal`, with the
//!   swoop sound), sweeping at the target's middle until it bites, hits a
//!   wall, is hurt or sees a cat (`PhantomSweepAttackGoal`).
//! - It looks for the highest player within 64 blocks every three seconds
//!   (`PhantomAttackPlayerTargetGoal`).
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{MonsterGoalContext, Target};

/// `Level.getSeaLevel` in the overworld.
const SEA_LEVEL: i32 = 63;

/// `Phantom.registerGoals`' goal selector.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, PhantomAttackStrategyGoal { next_sweep: 0 });
    selector.add(2, PhantomSweepAttackGoal { scared: false, cat_search_tick: 0 });
    selector.add(3, PhantomCircleAroundAnchorGoal { angle: 0.0, distance: 0.0, height: 0.0, clockwise: 1.0 });
    selector
}

/// `Phantom.registerGoals`' target selector.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, PhantomAttackPlayerTargetGoal { next_scan: 10 });
    selector
}

/// A cell `blockPosition` of a point.
fn cell(p: DVec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

/// `setAnchorAboveTarget`: 20 to 39 above the target, never below the sea.
fn anchor_above_target(ctx: &mut MonsterGoalContext) {
    if ctx.phantom.anchor.is_none() {
        return;
    }
    let Some(target) = ctx.target() else { return };
    let (x, y, z) = cell(target.position);
    let y = (y + 20 + ctx.random.next_int(20) as i32).max(SEA_LEVEL + 1);
    ctx.phantom.anchor = Some((x, y, z));
}

/// `PhantomAttackStrategyGoal`: while it has a target it may attack, it
/// circles, then dives after its interval.
#[derive(Clone)]
pub(crate) struct PhantomAttackStrategyGoal {
    next_sweep: i32,
}
impl Goal<MonsterGoalContext> for PhantomAttackStrategyGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some_and(|t| ctx.can_attack(t))
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        // `adjustedTickDelay(10)`, in goal ticks.
        self.next_sweep = 5;
        ctx.phantom.swoop = false;
        anchor_above_target(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        // `getHeightmapPos(...).above(10 + nextInt(20))`: back up above the
        // ground under its anchor (the anchor's own height stands in for
        // the ground's).
        if let Some((x, y, z)) = ctx.phantom.anchor {
            ctx.phantom.anchor = Some((x, y + 10 + ctx.random.next_int(20) as i32, z));
        }
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if !ctx.phantom.swoop {
            self.next_sweep -= 1;
            if self.next_sweep <= 0 {
                ctx.phantom.swoop = true;
                anchor_above_target(ctx);
                self.next_sweep = ((8 + ctx.random.next_int(4) as i32) * 20 + 1) / 2;
                ctx.phantom.swooped = true;
            }
        }
    }
}

/// `PhantomSweepAttackGoal`: dive at the target's middle; a bite, a wall or
/// a hurt ends the dive, as does a cat within 16 blocks.
#[derive(Clone)]
pub(crate) struct PhantomSweepAttackGoal {
    scared: bool,
    cat_search_tick: i32,
}
impl Goal<MonsterGoalContext> for PhantomSweepAttackGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target.is_some() && ctx.phantom.swoop
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let Some(target) = ctx.target() else { return false };
        if !target.alive || !ctx.can_attack(target) || !self.can_start(ctx, world) {
            return false;
        }
        if ctx.tick_count > self.cat_search_tick {
            self.cat_search_tick = ctx.tick_count + 20;
            let p = ctx.body.position;
            self.scared = ctx.mobs.iter().any(|m| m.kind == "minecraft:cat" && m.alive && (m.position - p).abs().max_element() <= 16.5);
        }
        !self.scared
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.clear_target();
        ctx.phantom.swoop = false;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(target) = ctx.target() else { return };
        ctx.phantom.move_target = target.position + DVec3::new(0.0, f64::from(target.height) * 0.5, 0.0);
        let body = &ctx.body;
        let (half, other) = (f64::from(body.width) / 2.0 + 0.2, f64::from(target.width) / 2.0);
        let (p, q) = (body.position, target.position);
        let meets = p.x - half < q.x + other && p.x + half > q.x - other && p.y - 0.2 < q.y + f64::from(target.height) && p.y + f64::from(body.height) + 0.2 > q.y && p.z - half < q.z + other && p.z + half > q.z - other;
        if meets {
            ctx.attack = Some(target.target);
            ctx.melee.swings = ctx.melee.swings.wrapping_add(1);
            ctx.phantom.swoop = false;
        } else if body.horizontal_collision || ctx.hurt_by.is_some_and(|(_, when)| ctx.tick_count - when < 10) {
            ctx.phantom.swoop = false;
        }
    }
}

/// `PhantomCircleAroundAnchorGoal`.
#[derive(Clone)]
pub(crate) struct PhantomCircleAroundAnchorGoal {
    angle: f32,
    distance: f32,
    height: f32,
    clockwise: f32,
}
impl PhantomCircleAroundAnchorGoal {
    /// `selectNext`: fifteen degrees on round the anchor.
    fn select_next(&mut self, ctx: &mut MonsterGoalContext) {
        let anchor = *ctx.phantom.anchor.get_or_insert_with(|| cell(ctx.body.position));
        self.angle += self.clockwise * 15.0_f32.to_radians();
        let corner = DVec3::new(f64::from(anchor.0), f64::from(anchor.1), f64::from(anchor.2));
        ctx.phantom.move_target = corner + DVec3::new(f64::from(self.distance * self.angle.cos()), f64::from(-4.0 + self.height), f64::from(self.distance * self.angle.sin()));
    }
}
impl Goal<MonsterGoalContext> for PhantomCircleAroundAnchorGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target.is_none() || !ctx.phantom.swoop
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.distance = 5.0 + ctx.random.next_float() * 10.0;
        self.height = -4.0 + ctx.random.next_float() * 9.0;
        self.clockwise = if ctx.random.next_boolean() { 1.0 } else { -1.0 };
        self.select_next(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        // The rolls are each `adjustedTickDelay`'s half.
        if ctx.random.next_int(175) == 0 {
            self.height = -4.0 + ctx.random.next_float() * 9.0;
        }
        if ctx.random.next_int(125) == 0 {
            self.distance += 1.0;
            if self.distance > 15.0 {
                self.distance = 5.0;
                self.clockwise = -self.clockwise;
            }
        }
        if ctx.random.next_int(225) == 0 {
            self.angle = ctx.random.next_float() * 2.0 * std::f32::consts::PI;
            self.select_next(ctx);
        }
        let p = ctx.body.position;
        if ctx.phantom.move_target.distance_squared(p) < 4.0 {
            self.select_next(ctx);
        }
        let empty = |pos: (i32, i32, i32)| world.block(pos).is_none_or(|b| b.id == "minecraft:air" || b.id == "minecraft:cave_air" || b.id == "minecraft:void_air");
        let (x, y, z) = cell(p);
        if ctx.phantom.move_target.y < p.y && !empty((x, y - 1, z)) {
            self.height = self.height.max(1.0);
            self.select_next(ctx);
        }
        if ctx.phantom.move_target.y > p.y && !empty((x, y + 1, z)) {
            self.height = self.height.min(-1.0);
            self.select_next(ctx);
        }
    }
}

/// `PhantomAttackPlayerTargetGoal`: every three seconds, the highest player
/// within 64 blocks (in its box grown 16 across and 64 up and down) it may
/// attack.
#[derive(Clone)]
pub(crate) struct PhantomAttackPlayerTargetGoal {
    next_scan: i32,
}
impl Goal<MonsterGoalContext> for PhantomAttackPlayerTargetGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Target])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if self.next_scan > 0 {
            self.next_scan -= 1;
            return false;
        }
        self.next_scan = 30;
        let p = ctx.body.position;
        let mut found: Vec<(f64, Target)> = ctx
            .players
            .iter()
            .filter(|q| (q.position.x - p.x).abs() <= 16.5 && (q.position.z - p.z).abs() <= 16.5 && (q.position.y - p.y).abs() <= 64.5 && q.position.distance(p) <= 64.0)
            .map(|q| (q.position.y, Target::Player(q.id)))
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, target) in found {
            if ctx.info(target).is_some_and(|info| ctx.can_attack(info)) {
                ctx.set_target(Some(target));
                return true;
            }
        }
        false
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some_and(|t| ctx.can_attack(t))
    }
}

/// `Mth.approachDegrees`.
fn approach_degrees(from: f32, to: f32, step: f32) -> f32 {
    let difference = wrap_degrees(to - from);
    from + difference.clamp(-step, step)
}

/// `Mth.wrapDegrees`.
fn wrap_degrees(angle: f32) -> f32 {
    let mut a = angle % 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// `Mth.approach`.
fn approach(from: f32, to: f32, step: f32) -> f32 {
    if from < to {
        (from + step).min(to)
    } else {
        (from - step).max(to)
    }
}

impl MobEntity {
    /// A phantom's living tick: its goals, `PhantomMoveControl`, a flying
    /// mob's travel (`FlyingMob.travel`: 0.91 drag in air, 0.8 in water,
    /// 0.5 in lava, no gravity) and its body taking its heading.
    pub(super) fn tick_phantom(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[crate::monster_ai::MobCandidate], game_time: i64, difficulty: i32) {
        let position = self.body.position;
        self.body.trim_small_velocity();
        self.no_action_time += 1;
        let fluid = crate::fluid::FluidFrame::sample(world, position, self.body.width, self.body.height);
        let ai = &mut self.ai;
        ai.state.body = self.body.clone();
        ai.state.health = self.health;
        ai.state.game_time = game_time;
        ai.state.difficulty = difficulty;
        ai.state.players = players.to_vec();
        ai.state.villagers = villagers.to_vec();
        ai.state.fluid = fluid;
        ai.state.no_action_time = self.no_action_time;
        ai.state.tick_count = self.tick_count;
        ai.state.random = std::mem::take(&mut self.random);
        if ai.state.phantom.move_target == DVec3::ZERO {
            ai.state.phantom.move_target = position;
        }
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut self.ai.state.random);
        if std::mem::take(&mut self.ai.state.phantom.swooped) {
            let pitch = 0.95 + self.random.next_float() * 0.1;
            self.voices.push((Voice::Event("entity.phantom.swoop", 10.0, pitch), position));
        }
        // `PhantomMoveControl.tick`.
        let state = &mut self.ai.state.phantom;
        if self.body.horizontal_collision {
            self.yaw += 180.0;
            state.speed = 0.1;
        }
        let d = state.move_target - self.body.position;
        let (mut tdx, tdy, mut tdz) = (d.x, d.y, d.z);
        let mut sd = (tdx * tdx + tdz * tdz).sqrt();
        if sd.abs() > 1.0e-5 {
            let scale = 1.0 - (tdy * 0.7).abs() / sd;
            tdx *= scale;
            tdz *= scale;
            sd = (tdx * tdx + tdz * tdz).sqrt();
            let sd2 = (tdx * tdx + tdz * tdz + tdy * tdy).sqrt();
            let previous = self.yaw;
            let angle = tdz.atan2(tdx) as f32;
            let a = wrap_degrees(self.yaw + 90.0);
            let b = wrap_degrees(angle.to_degrees());
            self.yaw = approach_degrees(a, b, 4.0) - 90.0;
            if wrap_degrees(previous - self.yaw).abs() < 3.0 {
                state.speed = approach(state.speed, 1.8, 0.005 * (1.8 / state.speed));
            } else {
                state.speed = approach(state.speed, 0.2, 0.025);
            }
            let x_rot = -((-tdy).atan2(sd).to_degrees() as f32);
            self.pitch = x_rot;
            let move_angle = (self.yaw + 90.0).to_radians();
            let speed = f64::from(state.speed);
            let wanted = DVec3::new(speed * f64::from(move_angle.cos()) * (tdx / sd2).abs(), speed * f64::from(x_rot.to_radians().sin()) * (tdy / sd2).abs(), speed * f64::from(move_angle.sin()) * (tdz / sd2).abs());
            self.body.velocity += (wanted - self.body.velocity) * 0.2;
        }
        let drag = if fluid.in_water() { 0.8 } else if fluid.in_lava() { 0.5 } else { 0.91 };
        let gravity = self.body.gravity;
        self.body.gravity = 0.0;
        let _ = self.body.travel_air_jumping(world, DVec3::ZERO, 0.0, self.yaw, false);
        self.body.gravity = gravity;
        // `travel_air_jumping`'s own air drag is 0.91 across and 0.98 up; a
        // flier's is the same both ways.
        self.body.velocity = DVec3::new(self.body.velocity.x / 0.91 * drag, self.body.velocity.y / 0.98 * drag, self.body.velocity.z / 0.91 * drag);
        self.body.fall_distance = 0.0;
        // `PhantomBodyRotationControl`: the head and body take its heading.
        let ai = &mut self.ai;
        ai.yaw = self.yaw;
        ai.body_rotation.body_yaw = self.yaw;
        ai.state.look_control.head_yaw = self.yaw;
    }

    /// `Phantom.getPhantomSize` (`size`).
    pub fn phantom_size(&self) -> i32 {
        if self.kind == crate::species::Kind::Phantom {
            self.variant.parse().unwrap_or(0)
        } else {
            0
        }
    }
}
