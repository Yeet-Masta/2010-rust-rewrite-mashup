//! Creakings (26.3 `Creaking`, `CreakingAi`).
//!
//! - A creaking only moves unwatched. While a player who could be its prey
//!   looks its way it stands frozen: no steps, no turns, no pushes. The
//!   first player to look at it from within twelve blocks wakes it, and it
//!   hunts that player from then on. A carved pumpkin hides a player from
//!   a creaking that is already awake. With no player about it goes back
//!   to sleep.
//! - Awake and free to move, it walks at its prey and strikes once every
//!   two seconds.
//! - One its creaking heart keeps (`home_pos`) can't be hurt. A blow from
//!   a mob, a projectile or a player only sways it, and a player's blow
//!   makes the heart cry out. It dies when its heart stops keeping it, and
//!   it crumbles away when the heart is broken.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{MonsterGoalContext, Target};
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// `ACTIVATION_RANGE_SQ`.
const ACTIVATION_RANGE_SQ: f64 = 144.0;
/// `ATTACK_INTERVAL`.
const ATTACK_INTERVAL: i32 = 40;
/// `TWITCH_DEATH_DURATION`.
const TWITCH_DEATH: i32 = 45;

/// A creaking's state (`CAN_MOVE`, `IS_ACTIVE`, `IS_TEARING_DOWN`, `HOME_POS`).
#[derive(Clone, Debug, Default)]
pub struct CreakingState {
    pub home: Option<Cell>,
    /// Watched, it cannot move (`!canMove`).
    pub frozen: bool,
    pub active: bool,
    pub tearing_down: bool,
    /// `attackAnimationRemainingTicks` and `invulnerabilityAnimationRemainingTicks`.
    pub attack_anim: i32,
    pub invulnerable_anim: i32,
    /// The ticks of its twitching death.
    pub death: i32,
    /// `ATTACK_COOLING_DOWN`.
    pub cooldown: i32,
    /// Who the blow about to land is blamed on: nobody (the world), a mob
    /// or projectile, or a player.
    pub blame: Option<bool>,
    /// A blow that passes its heart's guard (`BYPASSES_INVULNERABILITY`).
    pub bypass: bool,
    /// Out: a player struck it, for its heart to hear; it swayed (entity
    /// event 66); it struck (entity event 4).
    pub heart_hurt: bool,
    pub swayed: bool,
    pub struck: bool,
}

/// What the creaking's goals share.
#[derive(Clone, Debug, Default)]
pub struct CreakingCtx {
    pub frozen: bool,
    pub active: bool,
    pub cooldown: i32,
    /// Out: a strike made.
    pub struck: bool,
}

/// `CreakingAi`'s core, idle and fight activities.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, FrozenGate(crate::monster_ai::FloatGoal));
    selector.add(1, FightGoal { recalculate: 0 });
    selector.add(2, FrozenGate(crate::monster_ai::StrollGoal::new(0.3, 0.0)));
    selector.add(3, FrozenGate(crate::monster_ai::LookAtPlayerGoal { range: 8.0 }));
    selector
}

/// `StartAttacking` while awake: the nearest player it sees.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, AwakeGate(crate::monster_ai::NearestTargetGoal::every(crate::monster_ai::Prey::Player, true, 1)));
    selector
}

/// A goal a frozen creaking can't run.
#[derive(Clone)]
struct FrozenGate<G>(G);

impl<G: Goal<MonsterGoalContext> + Clone + 'static> Goal<MonsterGoalContext> for FrozenGate<G> {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.creaking.frozen && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.creaking.frozen && self.0.can_continue(ctx, world)
    }
    fn every_tick(&self) -> bool {
        self.0.every_tick()
    }
    fn interruptible(&self) -> bool {
        self.0.interruptible()
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.start_world(ctx, world);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.tick(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

/// A target goal that only an awake creaking runs.
#[derive(Clone)]
struct AwakeGate<G>(G);

impl<G: Goal<MonsterGoalContext> + Clone + 'static> Goal<MonsterGoalContext> for AwakeGate<G> {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.creaking.active && ctx.target.is_none() && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.creaking.active && self.0.can_continue(ctx, world)
    }
    fn every_tick(&self) -> bool {
        self.0.every_tick()
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.start_world(ctx, world);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.tick(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

/// `SetWalkTargetFromAttackTargetIfTargetOutOfReach(1.0)` with
/// `MeleeAttack(canMove, 40)`.
#[derive(Clone)]
struct FightGoal {
    recalculate: i32,
}

impl Goal<MonsterGoalContext> for FightGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.creaking.frozen && ctx.target().is_some()
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
        if in_reach && ctx.sees(world, target) {
            ctx.navigation.stop();
        } else {
            self.recalculate -= 1;
            if self.recalculate <= 0 || ctx.navigation.is_done() {
                self.recalculate = 10;
                let profile = ctx.profile();
                let _ = crate::navigation::navigate_walk_to_entity(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, target.position, 1.0, 0);
            }
        }
        if ctx.creaking.cooldown <= 0 && in_reach && ctx.sees(world, target) {
            ctx.creaking.cooldown = ATTACK_INTERVAL;
            ctx.creaking.struck = true;
            ctx.melee.swings = ctx.melee.swings.wrapping_add(1);
            ctx.attack = Some(target.target);
        }
    }
}

impl MobEntity {
    /// Whether its heart keeps it.
    pub fn heart_bound(&self) -> bool {
        self.kind == Kind::Creaking && self.creaking.home.is_some()
    }

    /// `Creaking.hurtServer` with a heart: no harm, only a sway, and a
    /// player's blow cries out to the heart. `None` lets the blow land.
    pub(super) fn creaking_guard(&mut self) -> Option<DamageResult> {
        if !self.heart_bound() || std::mem::take(&mut self.creaking.bypass) {
            return None;
        }
        let blame = self.creaking.blame.take();
        if self.creaking.invulnerable_anim > 0 || self.health <= 0.0 {
            return Some(super::withers::UNHURT);
        }
        let Some(player) = blame else { return Some(super::withers::UNHURT) };
        self.creaking.invulnerable_anim = 8;
        self.creaking.swayed = true;
        self.creaking.heart_hurt |= player;
        let at = self.position();
        self.voices.push((Voice::Event("entity.creaking.sway", 1.0, voice_pitch(&mut self.random, false)), at));
        Some(DamageResult { applied: true, dealt: 0.0, died: false, full: true })
    }

    /// `Creaking.aiStep` on the server, before its goals: the animations'
    /// clocks, its heart's hold on it, and `checkCanMove`.
    pub(super) fn creaking_prepare(&mut self, world: &dyn World, players: &[PlayerCandidate], views: &[(u64, crate::enderman::PlayerView)], kept: bool) {
        if self.kind != Kind::Creaking {
            return;
        }
        let c = &mut self.creaking;
        c.invulnerable_anim = (c.invulnerable_anim - 1).max(0);
        c.attack_anim = (c.attack_anim - 1).max(0);
        c.cooldown = (c.cooldown - 1).max(0);
        // `Creaking.tick`: a heart no longer keeping it ends it.
        if self.creaking.home.is_some() && !kept && self.health > 0.0 {
            self.health = 0.0;
            self.damage.dead = true;
        }
        if self.health <= 0.0 {
            return;
        }
        let was_frozen = self.creaking.frozen;
        let can_move = self.creaking_can_move(world, players, views);
        if can_move == was_frozen {
            let at = self.position();
            let sound = if can_move { "entity.creaking.unfreeze" } else { "entity.creaking.freeze" };
            self.voices.push((Voice::Event(sound, 1.0, voice_pitch(&mut self.random, false)), at));
            if !can_move {
                // `stopInPlace`.
                self.ai.state.navigation.stop();
                self.body.velocity.x = 0.0;
                self.body.velocity.z = 0.0;
            }
        }
        self.creaking.frozen = !can_move;
        // `StopAttackingIfTargetInvalid`: a target it no longer sees.
        if let Some(Target::Player(id)) = self.ai.state.target {
            let eye = self.body.position + DVec3::new(0.0, f64::from(self.species().eye_height(false)), 0.0);
            let seen = players.iter().any(|p| p.id == id && p.alive && !p.spectator && p.attackable && p.position.distance_squared(self.body.position) <= 32.0 * 32.0 && crate::sight::line_of_sight(world, eye, p.position + DVec3::new(0.0, f64::from(p.eye_height), 0.0)));
            if !seen {
                self.ai.state.target = None;
            }
        }
        let c = &self.creaking;
        self.ai.state.creaking = CreakingCtx { frozen: c.frozen, active: c.active, cooldown: c.cooldown, struck: false };
    }

    /// `checkCanMove`: whether no player it could hunt watches it, waking
    /// for the first who looks from close by.
    fn creaking_can_move(&mut self, world: &dyn World, players: &[PlayerCandidate], views: &[(u64, crate::enderman::PlayerView)]) -> bool {
        let p = self.body.position;
        let mut near: Vec<&PlayerCandidate> = players.iter().filter(|q| q.alive && !q.spectator && q.position.distance_squared(p) <= 32.0 * 32.0).collect();
        near.sort_by(|a, b| a.position.distance_squared(p).total_cmp(&b.position.distance_squared(p)));
        let active = self.creaking.active;
        if near.is_empty() {
            if active {
                self.creaking_deactivate();
            }
            return true;
        }
        let eye_y = p.y + f64::from(self.species().eye_height(false));
        let heights = [eye_y, p.y + 0.5, (eye_y + p.y) / 2.0];
        let mut potential = false;
        for q in near {
            if !q.attackable {
                continue;
            }
            potential = true;
            let view = views.iter().find(|(id, _)| *id == q.id).map(|(_, v)| *v).unwrap_or_default();
            if active && view.disguised {
                continue;
            }
            let eye = q.position + DVec3::new(0.0, f64::from(q.eye_height), 0.0);
            let look = crate::enderman::normalize(crate::enderman::view_vector(view.pitch, view.head_yaw));
            let watched = heights.iter().any(|&y| {
                let dir = crate::enderman::normalize(DVec3::new(p.x - eye.x, y - eye.y, p.z - eye.z));
                look.dot(dir) > 0.5 && crate::sight::line_of_sight(world, eye, DVec3::new(p.x, y, p.z))
            });
            if watched {
                if active {
                    return false;
                }
                if q.position.distance_squared(p) < ACTIVATION_RANGE_SQ {
                    // `activate`.
                    self.ai.state.target = Some(Target::Player(q.id));
                    self.creaking.active = true;
                    let at = self.position();
                    self.voices.push((Voice::Event("entity.creaking.activate", 1.0, voice_pitch(&mut self.random, false)), at));
                    return false;
                }
            }
        }
        if !potential && active {
            self.creaking_deactivate();
        }
        true
    }

    fn creaking_deactivate(&mut self) {
        self.ai.state.target = None;
        self.creaking.active = false;
        let at = self.position();
        self.voices.push((Voice::Event("entity.creaking.deactivate", 1.0, voice_pitch(&mut self.random, false)), at));
    }

    /// After its goals: the strike's animation, and a frozen creaking keeps
    /// its facing.
    pub(super) fn creaking_settle(&mut self, facing: (f32, f32)) {
        if self.kind != Kind::Creaking {
            return;
        }
        let ctx = std::mem::take(&mut self.ai.state.creaking);
        self.creaking.cooldown = ctx.cooldown;
        if self.creaking.frozen {
            (self.yaw, self.pitch) = facing;
            self.body.velocity.x = 0.0;
            self.body.velocity.z = 0.0;
        }
        if ctx.struck {
            self.creaking.attack_anim = 15;
            let at = self.position();
            self.voices.push((Voice::Event("entity.creaking.attack", 1.0, voice_pitch(&mut self.random, false)), at));
        }
        self.creaking.struck = ctx.struck;
    }

    /// `Creaking.tickDeath`: one its heart's breaking killed twitches for
    /// forty-five ticks before it crumbles. Whether it is gone.
    pub(super) fn creaking_death(&mut self, removed: bool) -> bool {
        if !(self.heart_bound() && self.creaking.tearing_down && self.damage.dead) {
            return removed;
        }
        self.creaking.death += 1;
        let gone = self.creaking.death > TWITCH_DEATH;
        self.damage.death_ticks = if gone { 20 } else { 0 };
        if gone {
            let at = self.position();
            self.voices.push((Voice::Event("entity.creaking.death", 1.0, voice_pitch(&mut self.random, false)), at));
        }
        gone
    }
}

impl EntityWorld {
    /// The level's creaking hearts and the creaking each keeps.
    pub fn set_creaking_hearts(&mut self, hearts: Vec<(Cell, u128)>) {
        self.creaking_hearts = hearts;
    }

    /// The hearts whose creaking a player struck since the last call.
    pub fn take_heart_hurts(&mut self) -> Vec<Cell> {
        std::mem::take(&mut self.heart_hurts)
    }

    /// Whether a creaking's heart still keeps it (`isProtector`).
    pub(super) fn creaking_kept(&self, id: u64) -> bool {
        let Some(home) = self.mobs.iter().find(|e| e.id == id).and_then(|e| e.creaking.home) else { return true };
        let uuid = self.uuid_of(id);
        self.creaking_hearts.iter().any(|&(c, u)| c == home && u == uuid)
    }

    /// The creakings the hearts keep, by UUID: where each stands, its box,
    /// and whether it must stay (`isPersistenceRequired`).
    pub fn creakings(&self) -> Vec<(u128, DVec3, DVec3, DVec3, bool)> {
        self.mobs
            .iter()
            .filter(|e| e.heart_bound() && e.health > 0.0)
            .map(|e| {
                let p = e.body.position;
                let half = f64::from(e.body.width) / 2.0;
                let min = DVec3::new(p.x - half, p.y, p.z - half);
                let max = DVec3::new(p.x + half, p.y + f64::from(e.body.height), p.z + half);
                (self.uuid_of(e.id), p, min, max, e.persistence_required)
            })
            .collect()
    }

    /// `CreakingHeartBlockEntity.removeProtector`: with no blame the
    /// creaking crumbles at once (`tearDown`); a player who broke the heart
    /// kills it, and it twitches a while first (`creakingDeathEffects`).
    pub fn remove_creaking(&mut self, uuid: u128, killed: bool, by_player: Option<u64>) {
        let Some(id) = self.mobs.iter().find(|e| e.kind == Kind::Creaking && self.uuid_of(e.id) == uuid).map(|e| e.id) else { return };
        match killed.then_some(by_player) {
            None => {
                if let Some(e) = self.discard_mob(id) {
                    let at = e.position();
                    self.sounds.push(MobSound { event: "entity.creaking.death".to_owned(), position: at, volume: 1.0, pitch: 1.0, category: "hostile" });
                }
            }
            Some(player) => {
                let Some(e) = self.mob_mut(id) else { return };
                let at = e.position();
                e.voices.push((Voice::Event("entity.creaking.twitch", 1.0, 1.0), at));
                e.creaking.bypass = true;
                e.creaking.tearing_down = true;
                if let Some(player) = player {
                    e.ai.state.hurt_by = Some((Target::Player(player), e.tick_count));
                }
                let hit = e.hurt(1.0e6);
                if player.is_some() {
                    e.damage.credit(hit, "minecraft:player", true);
                }
            }
        }
    }

    /// After its tick: the strike's entity event, and a player's blow told
    /// to its heart.
    pub(super) fn creaking_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        let hurt = std::mem::take(&mut e.creaking.heart_hurt);
        let swayed = std::mem::take(&mut e.creaking.swayed);
        let struck = std::mem::take(&mut e.creaking.struck);
        let home = e.creaking.home;
        if hurt {
            if let Some(home) = home {
                self.heart_hurts.push(home);
            }
        }
        if swayed {
            self.entity_events.push((id, 66));
        }
        if struck {
            self.entity_events.push((id, 4));
        }
    }
}
