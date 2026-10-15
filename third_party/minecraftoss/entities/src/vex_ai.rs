//! The vex's goals on the monster goal framework (26.3 `Vex.registerGoals`):
//! charging at its target through anything (`VexChargeAttackGoal`), drifting
//! about the spot it was summoned to (`VexRandomMoveGoal`), and looking at
//! players and mobs; its targets are whoever hurt it (raiders join in), its
//! owner's target (`VexCopyOwnerTargetGoal`) and the nearest player. Its
//! move control's wanted position is the context's `fly_target`, and
//! `charging` is `Vex.isCharging`.
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{continue_target, FloatGoal, HurtByTargetGoal, LookAtPlayerGoal, MonsterGoalContext, NearestTargetGoal, Prey, Target};
use glam::DVec3;
use minecraftoss_player::World;

/// `Goal.reducedTickDelay(7)`: the one-in-four roll both moves start on.
const MOVE_ROLL: u32 = 4;

/// `Vex.registerGoals`' goal selector.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, FloatGoal);
    selector.add(4, VexChargeAttackGoal);
    selector.add(8, VexRandomMoveGoal);
    selector.add(9, LookAtPlayerGoal { range: 3.0 });
    selector.add(10, LookAtPlayerGoal { range: 8.0 });
    selector
}

/// `Vex.registerGoals`' target selector.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: true });
    selector.add(2, VexCopyOwnerTargetGoal { target_mob: None, unseen_ticks: 0 });
    selector.add(3, NearestTargetGoal::every(Prey::Player, true, 10));
    selector
}

/// `VexChargeAttackGoal`: now and then, when its move control rests and its
/// target is more than two blocks off, it charges at the target's eyes
/// (`VEX_CHARGE`), steering on while within three blocks, and strikes when
/// their boxes meet.
#[derive(Clone)]
pub(crate) struct VexChargeAttackGoal;
impl Goal<MonsterGoalContext> for VexChargeAttackGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some(target) = ctx.target().filter(|t| t.alive) else { return false };
        if ctx.fly_target.is_some() || ctx.random.next_int(MOVE_ROLL) != 0 {
            return false;
        }
        ctx.body.position.distance_squared(target.position) > 4.0
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fly_target.is_some() && ctx.charging && ctx.target().is_some_and(|t| t.alive)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        if let Some(target) = ctx.target() {
            ctx.fly_target = Some((target.position + DVec3::new(0.0, f64::from(target.eye_height), 0.0), 1.0));
        }
        ctx.charging = true;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.charging = false;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(target) = ctx.target() else { return };
        let body = &ctx.body;
        let (half, other) = (f64::from(body.width) / 2.0, f64::from(target.width) / 2.0);
        let (p, q) = (body.position, target.position);
        let meets = p.x - half < q.x + other && p.x + half > q.x - other && p.y < q.y + f64::from(target.height) && p.y + f64::from(body.height) > q.y && p.z - half < q.z + other && p.z + half > q.z - other;
        if meets {
            // `doHurtTarget`, then the charge is spent.
            ctx.attack = Some(target.target);
            ctx.melee.swings = ctx.melee.swings.wrapping_add(1);
            ctx.charging = false;
        } else if p.distance_squared(q) < 9.0 {
            ctx.fly_target = Some((q + DVec3::new(0.0, f64::from(target.eye_height), 0.0), 1.0));
        }
    }
}

/// `VexRandomMoveGoal`: now and then, with its move control at rest, it
/// picks one of three random spots up to 7 blocks across and 5 up or down
/// of where it was summoned (or stands), the first that is empty, and
/// drifts to its middle at a quarter speed, looking there when it has no
/// target.
#[derive(Clone)]
pub(crate) struct VexRandomMoveGoal;
impl Goal<MonsterGoalContext> for VexRandomMoveGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fly_target.is_none() && ctx.random.next_int(MOVE_ROLL) == 0
    }
    fn can_continue(&mut self, _ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        false
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let p = ctx.body.position;
        let origin = ctx.bound_origin.unwrap_or((p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
        for _ in 0..3 {
            let (dx, dy, dz) = (ctx.random.next_int(15) as i32 - 7, ctx.random.next_int(11) as i32 - 5, ctx.random.next_int(15) as i32 - 7);
            let cell = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
            if world.block(cell).is_none_or(|b| b.id == "minecraft:air" || b.id == "minecraft:cave_air" || b.id == "minecraft:void_air") {
                let middle = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
                ctx.fly_target = Some((middle, 0.25));
                if ctx.target.is_none() {
                    ctx.look_control.set_look_at_with_limits(middle, 180.0, 20.0);
                }
                break;
            }
        }
    }
}

/// `VexCopyOwnerTargetGoal`: its owner's target becomes its own, whatever
/// it can see (`forNonCombat().ignoreLineOfSight()`).
#[derive(Clone)]
pub(crate) struct VexCopyOwnerTargetGoal {
    target_mob: Option<Target>,
    unseen_ticks: i32,
}
impl Goal<MonsterGoalContext> for VexCopyOwnerTargetGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Target])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.owner_target.and_then(|t| ctx.info(t)).is_some_and(|info| ctx.can_attack(info))
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        continue_target(ctx, world, self.target_mob, &mut self.unseen_ticks, None)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.set_target(ctx.owner_target);
        self.target_mob = ctx.target;
        self.unseen_ticks = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.clear_target();
        self.target_mob = None;
    }
}
