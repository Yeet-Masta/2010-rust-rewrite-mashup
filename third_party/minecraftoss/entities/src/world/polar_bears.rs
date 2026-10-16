//! Polar bears (26.3 `PolarBear`, `PolarBearMeleeAttackGoal`,
//! `PolarBearHurtByTargetGoal`, `PolarBearAttackPlayersGoal`).
//!
//! - A grown bear near a cub (within eight blocks across, four up and
//!   down) goes for players within half its follow range; it hunts foxes;
//!   what hurts it, or a cub near it, has the grown bears about after it.
//! - Closing in, it rears up a half second before it can swipe, with its
//!   warning roar (once in two seconds), and drops back down to strike.
//! - A cub flees any hurt; a grown bear flees only fire and the like.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Controls, Goal, GoalSelector};
use crate::monster_ai::{HurtByTargetGoal, MeleeAttackGoal, MonsterGoalContext, NearestTargetGoal, Prey};
use crate::species::Kind;

/// `STAND_ANIMATION_TICKS`.
const STAND_TICKS: f32 = 6.0;

/// A polar bear's rearing.
#[derive(Clone, Debug, Default)]
pub struct PolarState {
    /// `DATA_STANDING_ID`.
    pub standing: bool,
    /// `clientSideStandAnimation` and its last value, 0 to 6.
    pub stand: f32,
    pub stand_o: f32,
    pub warning_ticks: i32,
}

impl PolarState {
    /// `getStandingAnimationScale`.
    pub fn stand_scale(&self, partial: f32) -> f32 {
        (self.stand_o + (self.stand - self.stand_o) * partial) / STAND_TICKS
    }
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct PolarCtx {
    pub baby: bool,
    /// A cub within its box grown 8 across and 4 up and down.
    pub cub_near: bool,
    pub standing: Option<bool>,
    pub warn: bool,
}

/// `PolarBear.registerGoals`.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(1, PolarMeleeGoal(MeleeAttackGoal { hits: true, raises_arms: false, follow_unseen: true, gives_up_in_light: false }));
    selector.add(1, PolarPanicGoal(crate::mob_ai::PanicGoal::new(2.0)));
    selector.add(4, crate::mob_ai::FollowParentGoal::new(1.25));
    selector.add(5, crate::monster_ai::StrollGoal::new(1.0, 0.0));
    selector.add(6, crate::monster_ai::LookAtPlayerGoal { range: 6.0 });
    selector.add(7, crate::monster_ai::RandomLookGoal);
    selector
}

/// Its target goals.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, PolarHurtByGoal(HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: true }));
    selector.add(2, AttackPlayersGoal(NearestTargetGoal::every(Prey::Player, true, 10)));
    selector.add(4, Grown(NearestTargetGoal::every(Prey::Kinds(&["minecraft:fox"]), true, 5)));
    selector
}

/// Delegates every hook to the goal inside, but for those written out.
macro_rules! delegate {
    () => {
        fn controls(&self) -> Controls {
            self.0.controls()
        }
        fn every_tick(&self) -> bool {
            self.0.every_tick()
        }
        fn interruptible(&self) -> bool {
            self.0.interruptible()
        }
        fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
            self.0.start_world(ctx, world);
        }
        fn tick(&mut self, ctx: &mut MonsterGoalContext) {
            self.0.tick(ctx);
        }
    };
}

/// `PolarBearMeleeAttackGoal(1.25)`: the swipe when in reach and ready;
/// otherwise, within three blocks and its target's width, it rears for
/// the last half second of the cooldown and starts the cooldown over when
/// still out of reach; farther off it keeps the cooldown full.
#[derive(Clone)]
struct PolarMeleeGoal(MeleeAttackGoal);

impl Goal<MonsterGoalContext> for PolarMeleeGoal {
    delegate!();
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.polar.standing = Some(false);
        self.0.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let swings = ctx.melee.swings;
        self.0.tick_world(ctx, world);
        let Some(target) = ctx.target() else { return };
        if ctx.melee.swings != swings {
            ctx.polar.standing = Some(false);
            return;
        }
        let reach = f64::from(target.width + 3.0);
        if ctx.body.position.distance_squared(target.position) < reach * reach {
            if ctx.melee.ticks_until_attack <= 0 {
                ctx.polar.standing = Some(false);
                ctx.melee.ticks_until_attack = 20;
            }
            if ctx.melee.ticks_until_attack <= 10 {
                ctx.polar.standing = Some(true);
                ctx.polar.warn = true;
            }
        } else {
            ctx.melee.ticks_until_attack = 20;
            ctx.polar.standing = Some(false);
        }
    }
}

/// `PanicGoal(2.0, …)`: a cub's for any hurt, a grown bear's for fire.
#[derive(Clone)]
struct PolarPanicGoal(crate::mob_ai::PanicGoal);

impl Goal<MonsterGoalContext> for PolarPanicGoal {
    delegate!();
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        (ctx.polar.baby || ctx.on_fire) && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

/// `PolarBearHurtByTargetGoal`: a cub calls the grown bears and lets it be.
#[derive(Clone)]
struct PolarHurtByGoal(HurtByTargetGoal);

impl Goal<MonsterGoalContext> for PolarHurtByGoal {
    delegate!();
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.polar.baby && self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
        if ctx.polar.baby {
            self.0.stop(ctx);
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

/// `PolarBearAttackPlayersGoal`: a grown bear with a cub about, within half
/// its follow range.
#[derive(Clone)]
struct AttackPlayersGoal(NearestTargetGoal);

impl Goal<MonsterGoalContext> for AttackPlayersGoal {
    delegate!();
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.polar.baby {
            return false;
        }
        let range = ctx.follow_range;
        ctx.follow_range = range * 0.5;
        let found = self.0.can_start(ctx, world);
        ctx.follow_range = range;
        found && ctx.polar.cub_near
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let range = ctx.follow_range;
        ctx.follow_range = range * 0.5;
        let keep = self.0.can_continue(ctx, world);
        ctx.follow_range = range;
        keep
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

/// A target goal for grown bears only.
#[derive(Clone)]
struct Grown(NearestTargetGoal);

impl Goal<MonsterGoalContext> for Grown {
    delegate!();
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.polar.baby && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

impl MobEntity {
    /// Before its goals: its rearing's clocks (`PolarBear.tick`).
    pub(super) fn polar_prepare(&mut self, cub_near: bool) {
        if self.kind != Kind::PolarBear {
            return;
        }
        let p = &mut self.polar;
        p.stand_o = p.stand;
        p.stand = if p.standing { (p.stand + 1.0).min(STAND_TICKS) } else { (p.stand - 1.0).max(0.0) };
        if p.warning_ticks > 0 {
            p.warning_ticks -= 1;
        }
        self.ai.state.polar = PolarCtx { baby: self.baby(), cub_near, ..PolarCtx::default() };
    }

    /// After its goals: rearing and the warning roar (`playWarningSound`).
    pub(super) fn polar_settle(&mut self) {
        if self.kind != Kind::PolarBear {
            return;
        }
        let ctx = std::mem::take(&mut self.ai.state.polar);
        if let Some(standing) = ctx.standing {
            self.polar.standing = standing;
        }
        if ctx.warn && self.polar.warning_ticks <= 0 {
            let baby = self.baby();
            let pitch = voice_pitch(&mut self.random, baby);
            self.voices.push((Voice::Event("entity.polar_bear.warning", 1.0, pitch), self.body.position));
            self.polar.warning_ticks = 40;
        }
    }
}

impl EntityWorld {
    /// Whether a polar bear cub is within the box about `id` grown 8 across
    /// and 4 up and down.
    pub(super) fn polar_cub_near(&self, id: u64) -> bool {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return false };
        let (p, half, height) = (e.body.position, f64::from(e.body.width) / 2.0, f64::from(e.body.height));
        self.mobs.iter().any(|o| {
            let (q, oh) = (o.body.position, f64::from(o.body.width) / 2.0);
            o.kind == Kind::PolarBear && o.baby() && (q.x - p.x).abs() < half + 8.0 + oh && (q.z - p.z).abs() < half + 8.0 + oh && q.y < p.y + height + 4.0 && q.y + f64::from(o.body.height) > p.y - 4.0
        })
    }

    /// `HurtByTargetGoal.alertOthers`: the mob's kind within its follow
    /// range across and ten up and down, with no target of their own, turn
    /// on what hurt it (for polar bears, the grown ones only).
    pub(super) fn alert_others(&mut self, id: u64, attacker: crate::monster_ai::Target) {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        let (kind, p, range, owner) = (e.kind, e.body.position, e.ai.state.follow_range, e.owner);
        let corner = DVec3::new(p.x.floor(), p.y.floor(), p.z.floor());
        for other in self.mobs.iter_mut().filter(|o| o.id != id && o.kind == kind && o.health > 0.0) {
            let q = other.body.position;
            let inside = q.x > corner.x - range - 1.0 && q.x < corner.x + 1.0 + range + 1.0 && q.y > corner.y - 10.0 - f64::from(other.body.height) && q.y < corner.y + 11.0 && q.z > corner.z - range - 1.0 && q.z < corner.z + 1.0 + range + 1.0;
            if !inside || other.ai.state.target.is_some() || other.owner != owner || kind == Kind::PolarBear && other.baby() {
                continue;
            }
            other.ai.state.set_target(Some(attacker));
        }
    }
}
