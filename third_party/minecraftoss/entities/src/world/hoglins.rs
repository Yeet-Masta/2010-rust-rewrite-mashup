//! Hoglins (26.3 `Hoglin`, `HoglinAi`, `HoglinSpecificSensor`), their brain's
//! activities as goals.
//!
//! - Warped fungus, a respawn anchor or a nether portal within eight
//!   blocks across and four up and down is a repellent: a hoglin walks
//!   away from it and stays calm for ten seconds after.
//! - Fight: it goes for the nearest player it can see, striking every two
//!   seconds (a piglet's three quarters of a second), tossing its target.
//!   Hurt, a grown hoglin turns on its attacker and the grown hoglins about
//!   it join in; a piglet runs.
//! - Avoid: striking a piglin while piglins outnumber the grown hoglins
//!   about sends them all running for five to twenty seconds, until the
//!   numbers turn.
//! - Idle: grown hoglins keep their distance from grown piglins, piglets
//!   follow the grown, and they stroll. Each change of activity has its
//!   sound: angry into a fight, retreat into flight.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{MonsterGoalContext, NearestTargetGoal, Prey, Target};
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// The brain's activity, for its sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HoglinActivity {
    #[default]
    Idle,
    Fight,
    Avoid,
}

/// A hoglin's memories.
#[derive(Clone, Debug, Default)]
pub struct HoglinState {
    /// `NEAREST_REPELLENT`, as its sensor last found it.
    pub repellent: Option<Cell>,
    /// `PACIFIED`'s ticks left.
    pub pacified: i32,
    /// `AVOID_TARGET` and its ticks left.
    pub avoid: Option<(Target, i32)>,
    /// `ATTACK_COOLING_DOWN`.
    pub cooldown: i32,
    pub activity: HoglinActivity,
    /// The hurt it last answered, and whom it struck this tick.
    pub hurt_seen: Option<i32>,
    pub hit: Option<Target>,
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct HoglinCtx {
    pub baby: bool,
    pub repellent: Option<DVec3>,
    pub pacified: bool,
    pub avoid: Option<Target>,
    pub cooldown: i32,
    /// The nearest grown piglin it sees, and the nearest grown hoglin (for
    /// a piglet to follow).
    pub piglin: Option<DVec3>,
    pub adult: Option<DVec3>,
    /// Out: a strike made.
    pub struck: bool,
}

/// The activities as goals: repellent and flight first, then the fight,
/// then idling.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, AwayFromGoal { from: AwayFrom::Repellent, speed: 1.0, within: 8.0, wanted: None });
    selector.add(1, AwayFromGoal { from: AwayFrom::AvoidTarget, speed: 1.3, within: 15.0, wanted: None });
    selector.add(2, FightGoal { recalculate: 0 });
    selector.add(3, AwayFromGoal { from: AwayFrom::Piglin, speed: 0.4, within: 8.0, wanted: None });
    selector.add(4, FollowAdultGoal);
    selector.add(5, crate::mob_ai::BreedGoal::new());
    selector.add(6, crate::monster_ai::StrollGoal::new(0.4, 0.0));
    selector.add(7, crate::monster_ai::LookAtPlayerGoal { range: 8.0 });
    selector
}

/// `StartAttacking(findNearestValidAttackTarget)`: the nearest visible
/// player, unless calm or fleeing.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, StartAttackingGoal(NearestTargetGoal::every(Prey::Player, true, 1)));
    selector
}

#[derive(Clone)]
struct StartAttackingGoal(NearestTargetGoal);

impl Goal<MonsterGoalContext> for StartAttackingGoal {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.hoglin.pacified && ctx.hoglin.avoid.is_none() && !ctx.in_love && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.hoglin.pacified && ctx.hoglin.avoid.is_none() && self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AwayFrom {
    Repellent,
    AvoidTarget,
    Piglin,
}

/// `SetWalkTargetAwayFrom`: within `within` blocks of what it shuns, to a
/// spot up to 16 off away from it.
#[derive(Clone)]
struct AwayFromGoal {
    from: AwayFrom,
    speed: f64,
    within: f64,
    wanted: Option<DVec3>,
}

impl AwayFromGoal {
    fn shunned(&self, ctx: &MonsterGoalContext) -> Option<DVec3> {
        match self.from {
            AwayFrom::Repellent => ctx.hoglin.repellent,
            AwayFrom::AvoidTarget => ctx.hoglin.avoid.and_then(|t| ctx.info(t)).map(|i| i.position),
            AwayFrom::Piglin => ctx.hoglin.piglin.filter(|_| !ctx.hoglin.baby && ctx.target.is_none() && ctx.hoglin.avoid.is_none()),
        }
    }
}

impl Goal<MonsterGoalContext> for AwayFromGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let Some(from) = self.shunned(ctx) else { return false };
        if from.distance_squared(ctx.body.position) > self.within * self.within {
            return false;
        }
        let profile = ctx.profile();
        self.wanted = crate::stroll::land_random_position_away(world, &profile, ctx.body.position, 16, 7, from, &mut ctx.random);
        self.wanted.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        self.shunned(ctx).is_some() && !ctx.navigation.is_done()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if let Some(to) = self.wanted.take() {
            let profile = ctx.profile();
            let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, self.speed, 0);
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.navigation.stop();
    }
}

/// The fight: `SetWalkTargetFromAttackTargetIfTargetOutOfReach(1.0)` and
/// `MeleeAttack(40)` (a piglet's 15).
#[derive(Clone)]
struct FightGoal {
    recalculate: i32,
}

impl Goal<MonsterGoalContext> for FightGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some() && ctx.hoglin.avoid.is_none()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_start(ctx, world)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.navigation.stop();
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(target) = ctx.target() else { return };
        let eye = target.position + DVec3::new(0.0, f64::from(target.eye_height), 0.0);
        ctx.look_control.set_look_at_with_limits(eye, 45.0, 90.0);
        let in_reach = ctx.within_melee_range(target);
        if in_reach {
            ctx.navigation.stop();
        } else {
            self.recalculate -= 1;
            if self.recalculate <= 0 || ctx.navigation.is_done() {
                self.recalculate = 10;
                let profile = ctx.profile();
                let _ = crate::navigation::navigate_walk_to_entity(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, target.position, 1.0, 0);
            }
        }
        if ctx.hoglin.cooldown <= 0 && in_reach && ctx.sees(world, target) {
            ctx.hoglin.cooldown = if ctx.hoglin.baby { 15 } else { 40 };
            ctx.hoglin.struck = true;
            ctx.melee.swings = ctx.melee.swings.wrapping_add(1);
            ctx.attack = Some(target.target);
        }
    }
}

/// `BabyFollowAdult(5..16, 0.6)`: a piglet to the nearest grown hoglin.
#[derive(Clone)]
struct FollowAdultGoal;

impl Goal<MonsterGoalContext> for FollowAdultGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.hoglin.baby && ctx.target.is_none() && ctx.hoglin.adult.is_some_and(|a| {
            let d = a.distance_squared(ctx.body.position);
            d > 25.0 && d < 256.0
        })
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.hoglin.adult.is_some_and(|a| a.distance_squared(ctx.body.position) > 25.0) && !ctx.navigation.is_done()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if let Some(to) = ctx.hoglin.adult {
            let profile = ctx.profile();
            let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 0.6, 0);
        }
    }
}

impl MobEntity {
    /// Before its goals: its memories' clocks and what its sensor sees.
    pub(super) fn hoglin_prepare(&mut self, world: &dyn World, piglin: Option<DVec3>, adult: Option<DVec3>) {
        if self.kind != Kind::Hoglin {
            return;
        }
        let h = &mut self.hoglin;
        h.pacified = (h.pacified - 1).max(0);
        h.cooldown = (h.cooldown - 1).max(0);
        if let Some((_, left)) = h.avoid.as_mut() {
            *left -= 1;
            if *left <= 0 {
                h.avoid = None;
            }
        }
        // `HoglinSpecificSensor` once a second: the nearest repellent.
        if self.tick_count % 20 == 0 {
            let p = self.body.position;
            let base = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let mut best: Option<(i32, Cell)> = None;
            for dy in -4..=4 {
                for dx in -8..=8 {
                    for dz in -8..=8 {
                        let c = (base.0 + dx, base.1 + dy, base.2 + dz);
                        let d = dx.abs() + dy.abs() + dz.abs();
                        if best.is_none_or(|(b, _)| d < b) && world.block_in_tag(c, "minecraft:hoglin_repellents") {
                            best = Some((d, c));
                        }
                    }
                }
            }
            h.repellent = best.map(|(_, c)| c);
        }
        // `BecomePassiveIfMemoryPresent(NEAREST_REPELLENT, 200)`.
        if h.repellent.is_some() {
            h.pacified = 200;
            self.ai.state.target = None;
        }
        let h = &self.hoglin;
        self.ai.state.hoglin = HoglinCtx {
            baby: self.age.baby(),
            repellent: h.repellent.map(|c| DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1), f64::from(c.2) + 0.5)),
            pacified: h.pacified > 0,
            avoid: h.avoid.map(|(t, _)| t),
            cooldown: h.cooldown,
            piglin,
            adult,
            struck: false,
        };
    }

    /// After its goals: its strike, and its activity's sound on a change
    /// (`updateActivity`).
    pub(super) fn hoglin_settle(&mut self) {
        if self.kind != Kind::Hoglin {
            return;
        }
        let ctx = std::mem::take(&mut self.ai.state.hoglin);
        let h = &mut self.hoglin;
        h.cooldown = ctx.cooldown;
        h.hit = if ctx.struck { self.ai.state.attack } else { None };
        let activity = if self.ai.state.target.is_some() && h.avoid.is_none() {
            HoglinActivity::Fight
        } else if h.avoid.is_some() {
            HoglinActivity::Avoid
        } else {
            HoglinActivity::Idle
        };
        if activity != h.activity {
            h.activity = activity;
            let sound = match activity {
                HoglinActivity::Avoid => Some("entity.hoglin.retreat"),
                _ if self.time_in_overworld > 0 && !self.zombify_immune => Some("entity.hoglin.retreat"),
                HoglinActivity::Fight => Some("entity.hoglin.angry"),
                HoglinActivity::Idle if h.repellent.is_some() => Some("entity.hoglin.retreat"),
                HoglinActivity::Idle => None,
            };
            if let Some(sound) = sound {
                let baby = self.age.baby();
                let pitch = voice_pitch(&mut self.random, baby);
                self.voices.push((Voice::Event(sound, 1.0, pitch), self.body.position));
            }
        }
    }
}

impl EntityWorld {
    /// What its sensor sees about `id`: the nearest grown piglin and the
    /// nearest grown hoglin within its follow range.
    pub(super) fn hoglin_sights(&self, id: u64) -> (Option<DVec3>, Option<DVec3>) {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return (None, None) };
        let p = e.body.position;
        let nearest = |kind: Kind| {
            self.mobs
                .iter()
                .filter(|o| o.id != id && o.kind == kind && !o.baby() && o.health > 0.0 && o.body.position.distance_squared(p) <= 256.0)
                .map(|o| o.body.position)
                .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
        };
        (nearest(Kind::Piglin), nearest(Kind::Hoglin))
    }

    /// The grown hoglins within sixteen blocks of `id` (`NEAREST_VISIBLE_ADULT_HOGLINS`).
    fn adult_hoglins_near(&self, id: u64) -> Vec<u64> {
        let Some(p) = self.mobs.iter().find(|e| e.id == id).map(|e| e.body.position) else { return Vec::new() };
        self.mobs.iter().filter(|o| o.id != id && o.kind == Kind::Hoglin && !o.baby() && o.health > 0.0 && o.body.position.distance_squared(p) <= 256.0).map(|o| o.id).collect()
    }

    /// `piglinsOutnumberHoglins`.
    fn piglins_outnumber(&self, id: u64) -> bool {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return false };
        if e.baby() {
            return false;
        }
        let p = e.body.position;
        let piglins = self.mobs.iter().filter(|o| o.kind == Kind::Piglin && !o.baby() && o.health > 0.0 && o.body.position.distance_squared(p) <= 256.0).count();
        piglins > self.adult_hoglins_near(id).len() + 1
    }

    fn target_position(&self, target: Target) -> Option<DVec3> {
        match target {
            Target::Player(p) => self.pushing_players.iter().find(|b| b.id == p).map(|b| b.position),
            Target::Mob(m) | Target::Villager(m) => self.mob_body(m).map(|(b, _)| b.position),
        }
    }

    /// `setAttackTargetIfCloserThanCurrent` for `hoglin`.
    fn hoglin_join(&mut self, hoglin: u64, target: Target) {
        let to = self.target_position(target);
        let current = self.mobs.iter().find(|e| e.id == hoglin).and_then(|e| e.ai.state.target);
        let current_at = current.and_then(|t| self.target_position(t));
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == hoglin) else { return };
        if e.hoglin.pacified > 0 {
            return;
        }
        let p = e.body.position;
        let nearer = match (current_at, to) {
            (Some(c), Some(t)) if c.distance_squared(p) < t.distance_squared(p) => current.unwrap(),
            _ => target,
        };
        e.ai.state.set_target(Some(nearer));
    }

    /// `setAvoidTarget`: five to twenty seconds of flight.
    fn hoglin_flee(&mut self, hoglin: u64, from: Target) {
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == hoglin) else { return };
        e.ai.state.target = None;
        e.ai.state.navigation.stop();
        let ticks = 100 + e.random.next_int(301) as i32;
        e.hoglin.avoid = Some((from, ticks));
    }

    /// After its tick: its strike's call to the others (`onHitTarget`), its
    /// answer to a hurt (`wasHurtBy`), and a flight that may end
    /// (`wantsToStopFleeing`).
    pub(super) fn hoglin_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        let (hit, baby) = (e.hoglin.hit.take(), e.baby());
        let hurt = e.ai.state.hurt_by;
        let fresh = hurt.map(|(_, w)| w) != e.hoglin.hurt_seen && hurt.is_some();
        e.hoglin.hurt_seen = hurt.map(|(_, w)| w);
        if let Some(target) = hit.filter(|_| !baby) {
            let piglin = matches!(target, Target::Mob(m) if self.mobs.iter().any(|o| o.id == m && o.kind == Kind::Piglin));
            if piglin && self.piglins_outnumber(id) {
                self.hoglin_flee(id, target);
                for other in self.adult_hoglins_near(id) {
                    self.hoglin_flee(other, target);
                }
            } else {
                for other in self.adult_hoglins_near(id) {
                    self.hoglin_join(other, target);
                }
            }
        }
        if let Some((attacker, _)) = hurt.filter(|_| fresh) {
            if let Some(e) = self.mob_mut(id) {
                e.hoglin.pacified = 0;
                e.in_love = 0;
            }
            if baby {
                self.hoglin_flee(id, attacker);
            } else {
                let fleeing_piglins = self.mobs.iter().find(|e| e.id == id).is_some_and(|e| e.hoglin.avoid.is_some())
                    && matches!(attacker, Target::Mob(m) if self.mobs.iter().any(|o| o.id == m && o.kind == Kind::Piglin));
                let hoglin = matches!(attacker, Target::Mob(m) if self.mobs.iter().any(|o| o.id == m && o.kind == Kind::Hoglin));
                if !fleeing_piglins && !hoglin {
                    if let Some(e) = self.mob_mut(id) {
                        e.ai.state.set_target(Some(attacker));
                    }
                    for other in self.adult_hoglins_near(id) {
                        self.hoglin_join(other, attacker);
                    }
                }
            }
        }
        // `EraseMemoryIf(wantsToStopFleeing, AVOID_TARGET)`.
        let flee_over = !baby && !self.piglins_outnumber(id);
        if let Some(e) = self.mob_mut(id) {
            if flee_over {
                e.hoglin.avoid = None;
            }
        }
    }
}
