//! Axolotls (26.3 `Axolotl` and `AxolotlAi`, its brain as goals).
//!
//! - Out of water and rain it dries out over five minutes (`MAX_AIR`
//!   6000), then takes 2 a second.
//! - Hurt by something in water, one time in three (likelier for a big
//!   hit or when below half health) it plays dead for ten seconds: it lies
//!   still, regenerating (`PlayDead`), and nothing hunts it.
//! - In water it hunts fish, squid and tadpoles within eight blocks unless
//!   it hunted lately (a two-minute `HAS_HUNTING_COOLDOWN` after each
//!   fight), and always fights drowned and guardians; it chases at 0.6
//!   in water, 0.15 ashore, biting once a second.
//! - When a target a player killed dies near them, the player gets five
//!   seconds of Regeneration (`applySupportingEffects`).
//! - Otherwise it swims at 0.5 and crawls at 0.15, following temptation and
//!   its kind, and breeding.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{LookAtPlayerGoal, MonsterGoalContext, Target};
use crate::navigation::navigate_walk_to;
use crate::species::Kind;

/// `Axolotl.getMaxAirSupply`.
pub const MAX_AIR: i32 = 6000;

/// The mobs an axolotl always fights (`#axolotl_always_hostiles`, the
/// generic ones) and hunts when it may (`#axolotl_hunt_targets`).
const HOSTILE: [&str; 2] = ["minecraft:guardian", "minecraft:elder_guardian"];
const HUNTED: [&str; 7] = ["minecraft:tropical_fish", "minecraft:pufferfish", "minecraft:salmon", "minecraft:cod", "minecraft:squid", "minecraft:glow_squid", "minecraft:tadpole"];

/// Its playing dead, its hunting cooldown and the end of a fight.
#[derive(Clone, Debug, Default)]
pub struct AxolotlState {
    /// `PLAY_DEAD_TICKS`.
    pub play_dead: i32,
    /// `PlayDead` began (its regeneration), and the pose's animator.
    started: bool,
    pub dead_pose: i32,
    pub dead_pose_o: i32,
    /// The game time its `HAS_HUNTING_COOLDOWN` lasts until.
    pub hunting_until: i64,
    /// Whom it was fighting last tick, and a fight just ended.
    fighting: Option<Target>,
    pub ended: Option<Target>,
}

impl AxolotlState {
    /// `playingDeadAnimator.getFactor`: eased in and out over half a second.
    pub fn dead_factor(&self, partial: f32) -> f32 {
        let t = (self.dead_pose_o as f32 + (self.dead_pose - self.dead_pose_o) as f32 * partial) / 10.0;
        -((std::f32::consts::PI * t).cos() - 1.0) / 2.0
    }

    pub fn playing_dead(&self) -> bool {
        self.play_dead > 0
    }
}

/// The brain's idle and fight activities as goals.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, crate::mob_ai::BreedGoal::new());
    selector.add(2, crate::mob_ai::TemptGoal::new(0.5));
    selector.add(3, crate::monster_ai::MeleeAttackGoal { hits: true, raises_arms: false, follow_unseen: true, gives_up_in_light: false });
    selector.add(4, AxolotlStrollGoal);
    selector.add(5, LookAtPlayerGoal { range: 6.0 });
    selector
}

/// `StartAttacking` from the attackables sensor.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, AxolotlHuntGoal { next_scan: 0 });
    selector
}

/// `AxolotlAttackablesSensor` and `StartAttacking`: every second, the
/// nearest fit prey in water within eight blocks, unless it is breeding.
#[derive(Clone)]
struct AxolotlHuntGoal {
    next_scan: i32,
}

impl Goal<MonsterGoalContext> for AxolotlHuntGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Target])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.target.is_some() || ctx.in_love {
            return false;
        }
        if self.next_scan > 0 {
            self.next_scan -= 1;
            return false;
        }
        self.next_scan = 10;
        let here = ctx.body.position;
        let may_hunt = ctx.game_time >= ctx.axolotl_hunting_until;
        let mut best: Option<(f64, Target)> = None;
        for m in ctx.mobs.clone() {
            if !m.alive || !(HOSTILE.contains(&m.kind) || may_hunt && HUNTED.contains(&m.kind)) {
                continue;
            }
            let d = m.position.distance_squared(here);
            let cell = (m.position.x.floor() as i32, (m.position.y + f64::from(m.height) * 0.5).floor() as i32, m.position.z.floor() as i32);
            let in_water = world.block(cell).is_some_and(|b| b.id == "minecraft:water" || b.property("waterlogged") == Some("true"));
            if d > 64.0 || !in_water {
                continue;
            }
            let Some(info) = ctx.info(Target::Mob(m.id)) else { continue };
            if !ctx.can_attack(info) || !ctx.sees(world, info) {
                continue;
            }
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, Target::Mob(m.id)));
            }
        }
        let Some((_, target)) = best else { return false };
        ctx.set_target(Some(target));
        false
    }
}

/// `RandomStroll.swim(0.5)` and `RandomStroll.stroll(0.15)`.
#[derive(Clone)]
struct AxolotlStrollGoal;

impl Goal<MonsterGoalContext> for AxolotlStrollGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.target.is_some() || ctx.random.next_int(60) != 0 {
            return false;
        }
        let in_water = ctx.fluid.in_water();
        let profile = ctx.profile();
        let Some(spot) = crate::stroll::default_random_position(world, &profile, ctx.body.position, 10, 7, &mut ctx.random) else { return false };
        let speed = if in_water { 0.5 } else { 0.15 };
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, spot, speed, 0);
        true
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.navigation.is_done() && ctx.target.is_none()
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.navigation.stop();
    }
}

impl MobEntity {
    /// `Axolotl.handleAirSupply`: wet it stays full, dry it counts down to
    /// drying out. Whether it takes the 2.
    pub(super) fn axolotl_air(&mut self, wet: bool) -> bool {
        if self.health > 0.0 && !wet {
            self.body.air -= 1;
            if self.body.air <= -20 {
                self.body.air = 0;
                return true;
            }
        } else {
            self.body.air = MAX_AIR;
        }
        false
    }

    /// `Axolotl.hurtServer`'s chance to play dead, before the hit.
    pub(super) fn axolotl_hurt(&mut self, amount: f32, by_something: bool) {
        if self.kind != Kind::Axolotl || self.no_ai {
            return;
        }
        let health = self.health;
        let max = self.max_health();
        if self.random.next_int(3) == 0
            && ((self.random.next_int(3) as f32) < amount || health / max < 0.5)
            && amount < health
            && self.body.touching_water
            && by_something
            && !self.axolotl.playing_dead()
        {
            self.axolotl.play_dead = 200;
        }
    }

    /// The brain's `PLAY_DEAD` activity and `ValidatePlayDead`: whether it
    /// lies still this tick. Also the pose's animator.
    pub(super) fn axolotl_step_still(&mut self) -> bool {
        if self.kind != Kind::Axolotl {
            return false;
        }
        self.ai.state.axolotl_hunting_until = self.axolotl.hunting_until;
        let a = &mut self.axolotl;
        a.dead_pose_o = a.dead_pose;
        // `EraseMemoryIf(isBreeding)`.
        if self.in_love > 0 {
            a.play_dead = 0;
        }
        let still = a.play_dead > 0 && self.body.touching_water && self.ai.state.hurt_by.is_some();
        if still && !a.started {
            a.started = true;
            self.ai.state.navigation.stop();
            let _ = self.effects.add(crate::effects::EffectInstance::new(crate::effects::MobEffect::Regeneration, 200, 0));
        }
        if !still {
            a.started = false;
        }
        if a.play_dead > 0 {
            a.play_dead -= 1;
        }
        if a.play_dead > 0 {
            a.dead_pose = (a.dead_pose + 1).min(10);
        } else {
            a.dead_pose = (a.dead_pose - 1).max(0);
        }
        if a.play_dead > 0 {
            self.ai.state.clear_target();
        }
        still
    }

    /// After its goals: a fight that ended starts the hunting cooldown.
    pub(super) fn axolotl_settle(&mut self, game_time: i64) {
        if self.kind != Kind::Axolotl {
            return;
        }
        let now = self.ai.state.target;
        let a = &mut self.axolotl;
        if a.fighting.is_some() && now.is_none() {
            a.hunting_until = game_time + 2400;
            a.ended = a.fighting;
        }
        a.fighting = now;
    }
}

impl EntityWorld {
    /// `Axolotl.onStopAttacking`: its prey dead by a player's hand within
    /// 20 blocks, that player is helped along (`applySupportingEffects`).
    pub(super) fn axolotl_step(&mut self, id: u64, players: &[PlayerCandidate]) {
        let Some(e) = self.mob_mut(id) else { return };
        let Some(Target::Mob(prey)) = e.axolotl.ended.take() else { return };
        let at = e.body.position;
        let Some(dead) = self.mobs.iter().find(|m| m.id == prey && m.health <= 0.0) else { return };
        let Some((Target::Player(player), _)) = dead.ai.state.hurt_by else { return };
        if !players.iter().any(|p| p.id == player && p.alive && (p.position - at).abs().max_element() <= 20.0 + 1.0) {
            return;
        }
        // The regeneration lengthens what the player has, up to two
        // minutes; with no sight of that here, five seconds.
        let effect = minecraftoss_player::effects::EffectInstance::new(minecraftoss_player::effects::MobEffect::Regeneration, 100, 0);
        self.player_splashes.push((player, PlayerSplash { effects: vec![effect], instant: Vec::new(), extinguish: false, at }));
    }
}
