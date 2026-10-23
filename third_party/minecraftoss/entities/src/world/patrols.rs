//! Illager patrols (26.3 `PatrollingMonster`, `LongDistancePatrolGoal`).
//!
//! A patrol walks toward its far-off target ten blocks at a time, weaving a
//! little to the side, its leader (with the ominous banner) a touch slower
//! and pointing the others at each next step; at its target the leader
//! picks another up to five hundred blocks off. A patroller with nobody
//! about stops patrolling; one whose path fails wanders for ten seconds.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// A patroller's errand (`patrolTarget`, `PatrolLeader`, `Patrolling`).
#[derive(Clone, Debug, Default)]
pub struct PatrolState {
    pub target: Option<Cell>,
    pub leader: bool,
    pub patrolling: bool,
}

/// What the patrol goal shares.
#[derive(Clone, Debug, Default)]
pub struct PatrolCtx {
    pub target: Option<Cell>,
    pub leader: bool,
    pub patrolling: bool,
    /// The other patrollers within sixteen blocks.
    pub companions: Vec<u64>,
    /// Out: a new far target (the leader's), the target handed to the
    /// others, and whether it gave up patrolling.
    pub new_target: Option<Cell>,
    pub lead_to: Option<Cell>,
    pub stopped: bool,
}

/// The kinds that patrol (`PatrollingMonster`s here: the illagers and the
/// ravager).
pub fn patrols(kind: Kind) -> bool {
    matches!(kind, Kind::Pillager | Kind::Vindicator | Kind::Evoker | Kind::Illusioner | Kind::Ravager)
}

/// The top of the ground at a column (`MOTION_BLOCKING_NO_LEAVES`), looked
/// for from sixteen above down to sixty-four below.
fn ground_top(world: &dyn World, x: i32, y: i32, z: i32) -> i32 {
    for top in (y - 64..=y + 16).rev() {
        let c = (x, top, z);
        let fluid = world.block(c).is_some_and(|b| matches!(b.id.as_str(), "minecraft:water" | "minecraft:lava"));
        if (world.solid(c) && !world.block_in_tag(c, "minecraft:leaves")) || fluid {
            return top + 1;
        }
    }
    y
}

/// `LongDistancePatrolGoal(mob, 0.7, 0.595)`.
#[derive(Clone)]
pub(crate) struct PatrolGoal {
    cooldown_until: i64,
}

impl PatrolGoal {
    pub(crate) fn new() -> Self {
        Self { cooldown_until: -1 }
    }
}

impl Goal<MonsterGoalContext> for PatrolGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.patrol.patrolling && ctx.target.is_none() && ctx.patrol.target.is_some() && ctx.game_time >= self.cooldown_until
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_start(ctx, world)
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if !ctx.navigation.is_done() {
            return;
        }
        let Some(target) = ctx.patrol.target else { return };
        let p = ctx.body.position;
        let leader = ctx.patrol.leader;
        let centre = DVec3::new(f64::from(target.0) + 0.5, f64::from(target.1) + 0.5, f64::from(target.2) + 0.5);
        if ctx.patrol.companions.is_empty() {
            ctx.patrol.patrolling = false;
            ctx.patrol.stopped = true;
            return;
        }
        if leader && centre.distance_squared(p) < 100.0 {
            // `findPatrolTarget`.
            let t = (p.x.floor() as i32 - 500 + ctx.random.next_int(1000) as i32, p.y.floor() as i32, p.z.floor() as i32 - 500 + ctx.random.next_int(1000) as i32);
            ctx.patrol.target = Some(t);
            ctx.patrol.new_target = Some(t);
            return;
        }
        // Ten blocks on toward a point swung a little aside of the target.
        let goal = DVec3::new(f64::from(target.0) + 0.5, f64::from(target.1), f64::from(target.2) + 0.5);
        let d = p - goal;
        let (s, c) = (90.0_f32.to_radians().sin() as f64, 90.0_f32.to_radians().cos() as f64);
        let swung = goal + DVec3::new(d.x * c + d.z * s, d.y, d.z * c - d.x * s) * 0.4;
        let step = p + (swung - p).normalize_or_zero() * 10.0;
        let (x, z) = (step.x.floor() as i32, step.z.floor() as i32);
        let y = ground_top(world, x, step.y.floor() as i32, z);
        let to = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
        let profile = ctx.profile();
        let speed = if leader { 0.595 } else { 0.7 };
        let moved = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, speed, 0).unwrap_or(false);
        if !moved {
            // `moveRandomly`.
            let (rx, rz) = (p.x.floor() as i32 - 8 + ctx.random.next_int(16) as i32, p.z.floor() as i32 - 8 + ctx.random.next_int(16) as i32);
            let ry = ground_top(world, rx, p.y.floor() as i32, rz);
            let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, DVec3::new(f64::from(rx), f64::from(ry), f64::from(rz)), 0.7, 0);
            self.cooldown_until = ctx.game_time + 200;
        } else if leader {
            ctx.patrol.lead_to = Some((x, y, z));
        }
    }
}

impl MobEntity {
    /// Before its goals: its errand for the patrol goal.
    pub(super) fn patrol_prepare(&mut self, companions: Vec<u64>) {
        if !patrols(self.kind) {
            return;
        }
        let p = &self.patrol;
        self.ai.state.patrol = PatrolCtx { target: p.target, leader: p.leader, patrolling: p.patrolling, companions, ..PatrolCtx::default() };
    }
}

impl EntityWorld {
    /// The patrollers within the box about `id` grown sixteen.
    pub(super) fn patrol_companions(&self, id: u64) -> Vec<u64> {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return Vec::new() };
        let p = e.body.position;
        self.mobs.iter().filter(|o| o.id != id && patrols(o.kind) && o.health > 0.0 && (o.body.position - p).abs().max_element() <= 16.0 + f64::from(o.body.width)).map(|o| o.id).collect()
    }

    /// After its tick: the patrol goal's word to it and to its companions
    /// (`setPatrolTarget`).
    pub(super) fn patrol_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        let ctx = std::mem::take(&mut e.ai.state.patrol);
        if ctx.stopped {
            e.patrol.patrolling = false;
        }
        if let Some(t) = ctx.new_target {
            e.patrol.target = Some(t);
            e.patrol.patrolling = true;
        }
        if let Some(t) = ctx.lead_to {
            for other in ctx.companions {
                if let Some(o) = self.mob_mut(other) {
                    o.patrol.target = Some(t);
                    o.patrol.patrolling = true;
                }
            }
        }
    }
}
