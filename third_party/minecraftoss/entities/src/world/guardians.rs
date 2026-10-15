//! Guardians and elder guardians (26.3 `Guardian`, `ElderGuardian`).
//!
//! - A guardian swims under `GuardianMoveControl`: turning up to 90
//!   degrees a tick, easing its speed an eighth of the way to the wanted
//!   one, and bobbing as it goes; it counts as moving only while it has
//!   somewhere to be.
//! - It fixes a player, squid or axolotl in sight more than three blocks
//!   off and holds still, charging its beam (`GuardianAttackGoal`): after
//!   ten ticks the beam shows, and once it has charged for its attack
//!   duration (80 ticks, an elder's 60) the target takes 1 magic damage
//!   (3 on hard, 2 more from an elder) and its melee hit. Losing sight of
//!   it, or a guardian's target coming within three blocks, breaks it off.
//! - Struck in melee while still, it pricks its attacker for 2 (thorns).
//! - Ashore it flops about.
//! - An elder stays within 16 blocks of where it settled
//!   (`MoveTowardsRestrictionGoal`) and every minute lays Mining Fatigue
//!   III for five minutes on players within 50 blocks.
use super::*;
use super::mobs::MobEntity;
use crate::fluid::FluidFrame;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{LookAtPlayerGoal, MonsterGoalContext, NearestTargetGoal, Prey, RandomLookGoal, Target};
use crate::species::Kind;

/// A guardian's sway and spikes (its client-side animation), its beam,
/// and the hit its beam lands this tick.
#[derive(Clone, Debug, Default)]
pub struct GuardianState {
    /// `DATA_ID_MOVING`.
    pub moving: bool,
    /// Its move control's `speed`.
    speed: f32,
    /// `DATA_ID_ATTACK_TARGET`: who its beam holds.
    pub attack_target: Option<Target>,
    /// `clientSideAttackTime`: ticks the beam has shown.
    pub attack_time: i32,
    pub tail: f32,
    pub tail_o: f32,
    tail_speed: f32,
    pub spikes: f32,
    pub spikes_o: f32,
    touched_ground: bool,
    /// An elder's home (`setHomeTo` its first spot, 16 blocks).
    pub home: Option<DVec3>,
    /// The target its charged beam strikes this tick.
    pub fire: Option<Target>,
}

impl GuardianState {
    /// `getTailAnimation`.
    pub fn tail(&self, partial: f32) -> f32 {
        self.tail_o + (self.tail - self.tail_o) * partial
    }

    /// `getSpikesAnimation`.
    pub fn spikes(&self, partial: f32) -> f32 {
        self.spikes_o + (self.spikes - self.spikes_o) * partial
    }
}

/// `Guardian.getAttackDuration` (`ElderGuardian`'s).
pub fn attack_duration(kind: Kind) -> i32 {
    if kind == Kind::ElderGuardian {
        60
    } else {
        80
    }
}

/// `Guardian.registerGoals` (with `ElderGuardian`'s slower stroll).
pub fn goals(kind: Kind) -> GoalSelector<MonsterGoalContext> {
    let elder = kind == Kind::ElderGuardian;
    let mut selector = GoalSelector::default();
    selector.add(4, GuardianAttackGoal { attack_time: 0, elder });
    selector.add(5, MoveTowardsHomeGoal);
    selector.add(7, GuardianStrollGoal { interval: if elder { 400 } else { 80 } });
    selector.add(8, LookAtPlayerGoal { range: 8.0 });
    selector.add(9, RandomLookGoal);
    selector
}

/// The target selector: players, squids and axolotls, seen, re-checked
/// every ten ticks.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, NearestTargetGoal::every(Prey::Player, true, 10));
    selector.add(1, NearestTargetGoal::every(Prey::Kinds(&["minecraft:squid", "minecraft:glow_squid", "minecraft:axolotl"]), true, 10));
    selector
}

/// `GuardianAttackGoal`.
#[derive(Clone)]
struct GuardianAttackGoal {
    attack_time: i32,
    elder: bool,
}

impl Goal<MonsterGoalContext> for GuardianAttackGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some_and(|t| t.alive)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_start(ctx, world) && (self.elder || ctx.target().is_some_and(|t| t.position.distance_squared(ctx.body.position) > 9.0))
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.attack_time = -10;
        ctx.fly_target = None;
        if let Some(t) = ctx.target() {
            ctx.look_control.set_look_at_with_limits(t.position + DVec3::new(0.0, f64::from(t.eye_height), 0.0), 90.0, 90.0);
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.guardian.attack_target = None;
        ctx.clear_target();
        ctx.guardian.trigger_stroll = true;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(target) = ctx.target() else { return };
        ctx.fly_target = None;
        ctx.look_control.set_look_at_with_limits(target.position + DVec3::new(0.0, f64::from(target.eye_height), 0.0), 90.0, 90.0);
        if !ctx.sees(world, target) {
            ctx.clear_target();
            return;
        }
        self.attack_time += 1;
        let duration = if self.elder { 60 } else { 80 };
        if self.attack_time == 0 {
            ctx.guardian.attack_target = Some(target.target);
            ctx.guardian.beam_started = true;
        } else if self.attack_time >= duration {
            ctx.guardian.fire = Some(target.target);
            ctx.clear_target();
        }
    }
}

/// `MoveTowardsRestrictionGoal`: an elder strayed from home heads back.
#[derive(Clone)]
struct MoveTowardsHomeGoal;

impl Goal<MonsterGoalContext> for MoveTowardsHomeGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.guardian.home.is_some_and(|home| home.distance_squared(ctx.body.position) > 16.0 * 16.0)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fly_target.is_some()
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = ctx.guardian.home.map(|home| (home, 1.0));
    }
}

/// `RandomStrollGoal(guardian, 1.0, 80)` (an elder's 400), which a hurt or
/// a broken-off attack sets going at once (`trigger`).
#[derive(Clone)]
struct GuardianStrollGoal {
    interval: u32,
}

impl Goal<MonsterGoalContext> for GuardianStrollGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.fly_target.is_some() {
            return false;
        }
        let forced = std::mem::take(&mut ctx.guardian.trigger_stroll);
        if !forced && (ctx.no_action_time >= 100 || ctx.random.next_int(self.interval) != 0) {
            return false;
        }
        match crate::mob_ai::random_open(world, ctx, 10, 7, true) {
            Some(spot) => {
                ctx.fly_target = Some((spot, 1.0));
                true
            }
            None => false,
        }
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fly_target.is_some()
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
    }
}

impl MobEntity {
    /// `Guardian.aiStep` around `LivingEntity.aiStep`: flopping ashore, the
    /// sway and spikes, the goals, `GuardianMoveControl` and
    /// `travelInWater`; returns the fall damage a landing deals.
    pub(super) fn tick_guardian(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[crate::monster_ai::MobCandidate], game_time: i64, difficulty: i32) -> Option<f32> {
        let position = self.body.position;
        let fluid = FluidFrame::sample(world, position, self.body.width, self.body.height);
        let in_water = fluid.in_water();
        if self.kind == Kind::ElderGuardian && self.guardian.home.is_none() {
            self.guardian.home = Some(DVec3::new(position.x.floor() + 0.5, position.y.floor(), position.z.floor() + 0.5));
        }
        // The client's sway and spikes.
        let g = &mut self.guardian;
        g.tail_o = g.tail;
        if !in_water {
            g.tail_speed = 2.0;
            if self.body.velocity.y > 0.0 && g.touched_ground {
                let flop = if self.kind == Kind::ElderGuardian { "entity.elder_guardian.flop" } else { "entity.guardian.flop" };
                self.voices.push((Voice::Event(flop, 1.0, 1.0), position));
            }
            g.touched_ground = self.body.velocity.y < 0.0 && self.body.on_ground;
        } else if g.moving {
            g.tail_speed = if g.tail_speed < 0.5 { 4.0 } else { g.tail_speed + (0.5 - g.tail_speed) * 0.1 };
        } else {
            g.tail_speed += (0.125 - g.tail_speed) * 0.2;
        }
        g.tail += g.tail_speed;
        g.spikes_o = g.spikes;
        if !in_water {
            g.spikes = self.random.next_float();
        } else if g.moving {
            g.spikes += (0.0 - g.spikes) * 0.25;
        } else {
            g.spikes += (1.0 - g.spikes) * 0.06;
        }
        if g.attack_target.is_some() && g.attack_time < attack_duration(self.kind) {
            g.attack_time += 1;
        }
        if in_water {
            self.body.air = 300;
        } else if self.body.on_ground {
            let (a, b) = (self.random.next_float(), self.random.next_float());
            self.body.velocity += DVec3::new(f64::from((a * 2.0 - 1.0) * 0.4), 0.5, f64::from((b * 2.0 - 1.0) * 0.4));
            self.yaw = self.random.next_float() * 360.0;
            self.ai.yaw = self.yaw;
            self.body.on_ground = false;
            self.body.needs_sync = true;
        }
        if self.guardian.attack_target.is_some() {
            self.yaw = self.ai.state.look_control.head_yaw;
            self.ai.yaw = self.yaw;
        }
        // `LivingEntity.aiStep`: the goals.
        if players.iter().any(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 32.0 * 32.0) {
            self.no_action_time = 0;
        }
        self.body.trim_small_velocity();
        self.no_action_time += 1;
        let eye = self.eye_height();
        let ai = &mut self.ai;
        ai.state.body = self.body.clone();
        ai.state.health = self.health;
        ai.state.game_time = game_time;
        ai.state.difficulty = difficulty;
        ai.state.players = players.to_vec();
        ai.state.villagers = villagers.to_vec();
        ai.state.fluid = fluid;
        ai.state.eye_height = eye;
        ai.state.no_action_time = self.no_action_time;
        ai.state.tick_count = self.tick_count;
        ai.state.guardian.attack_target = self.guardian.attack_target;
        ai.state.guardian.home = self.guardian.home;
        ai.state.random = std::mem::take(&mut self.random);
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut self.ai.state.random);
        let state = &mut self.ai.state.guardian;
        self.guardian.attack_target = state.attack_target;
        self.guardian.fire = state.fire.take();
        if std::mem::take(&mut state.beam_started) {
            // The beam's charge (event 21: `GuardianAttackSoundInstance`).
            self.guardian.attack_time = 0;
            self.voices.push((Voice::Event("entity.guardian.attack", 1.0, 1.0), position));
        }
        // `GuardianMoveControl.tick`.
        let attribute_speed = self.species().speed;
        let beaming = self.guardian.attack_target.is_some();
        let g = &mut self.guardian;
        if let Some((wanted, modifier)) = self.ai.state.fly_target {
            let delta = wanted - self.body.position;
            let length = delta.length();
            if length < 1.0 {
                self.ai.state.fly_target = None;
            }
            if length > 1.0e-7 {
                let heading = (delta.z.atan2(delta.x).to_degrees() as f32) - 90.0;
                self.yaw = crate::control::rotlerp(self.yaw, heading, 90.0);
                let wanted_speed = (modifier * attribute_speed) as f32;
                g.speed += (wanted_speed - g.speed) * 0.125;
                let phase = f64::from(self.tick_count) + self.id as f64;
                let push = (phase * 0.5).sin() * 0.05;
                let (sin, cos) = (f64::from(self.yaw.to_radians().sin()), f64::from(self.yaw.to_radians().cos()));
                let y_push = (phase * 0.75).sin() * 0.05;
                self.body.velocity += DVec3::new(push * cos, y_push * (sin + cos) * 0.25 + f64::from(g.speed) * (delta.y / length) * 0.1, push * sin);
                let look = DVec3::new(self.body.position.x + delta.x / length * 2.0, self.body.position.y + f64::from(eye) + delta.y / length / length, self.body.position.z + delta.z / length * 2.0);
                if !beaming {
                    self.ai.state.look_control.set_look_at_with_limits(look, 10.0, 40.0);
                }
                g.moving = true;
            }
        } else {
            g.speed = 0.0;
            g.moving = false;
        }
        let speed = g.speed;
        let moving = g.moving;
        // `Mob.setSpeed` drives it forward at that speed.
        let input = DVec3::new(0.0, 0.0, f64::from(speed));
        let landed = if in_water {
            // `Guardian.travelInWater`.
            self.body.velocity += super::dolphins::relative(input, 0.1, self.yaw);
            let landed = self.move_raw(world);
            self.body.velocity *= 0.9;
            if !moving && self.ai.state.target.is_none() {
                self.body.velocity.y -= 0.005;
            }
            landed
        } else if fluid.in_lava() {
            self.body.travel_lava(world, input, self.yaw, fluid.lava_height);
            None
        } else {
            self.body.travel_air_jumping(world, input, speed, self.yaw, false)
        };
        let ai = &mut self.ai;
        ai.yaw = self.yaw;
        ai.state.look_control.tick(self.body.position, eye, self.yaw, moving);
        ai.body_rotation.body_yaw = self.yaw;
        self.pitch = ai.state.look_control.pitch;
        landed.and_then(|fallen| super::living::fall_damage(&self.body, world, fallen, true, &mut self.voices))
    }

    /// `Guardian.hurtServer`'s stroll: a hurt sets it wandering.
    pub(super) fn guardian_hurt(&mut self) {
        if matches!(self.kind, Kind::Guardian | Kind::ElderGuardian) {
            self.ai.state.guardian.trigger_stroll = true;
        }
    }

    /// `Guardian.hurtServer`'s spikes: struck in melee while still, it
    /// pricks back for 2.
    pub fn guardian_thorns(&self) -> bool {
        matches!(self.kind, Kind::Guardian | Kind::ElderGuardian) && !self.guardian.moving && self.health > 0.0
    }
}

impl EntityWorld {
    /// The beam's landing (`GuardianAttackGoal.tick`) and an elder's curse
    /// (`ElderGuardian.customServerAiStep`).
    pub(super) fn guardian_step(&mut self, id: u64, kind: Kind, players: &[PlayerCandidate]) {
        let Some(e) = self.mob_mut(id) else { return };
        let position = e.body.position;
        let fire = e.guardian.fire.take();
        let curse = kind == Kind::ElderGuardian && (e.tick_count + id as i32) % 1200 == 0 && e.health > 0.0;
        if let Some(target) = fire {
            let elder = if kind == Kind::ElderGuardian { 2.0 } else { 0.0 };
            let magic = 1.0 + if self.difficulty >= 3 { 2.0 } else { 0.0 } + elder;
            self.direct_hit(id, target, magic, "minecraft:indirect_magic", None, position);
            self.mob_melee(id, kind, target, position);
        }
        if curse {
            // `MobEffectUtil.addEffectToPlayersAround`: survival players
            // within 50 blocks.
            for p in players.iter().filter(|p| p.alive && p.attackable && p.position.distance_squared(position) < 50.0 * 50.0) {
                let effect = minecraftoss_player::effects::EffectInstance::new(minecraftoss_player::effects::MobEffect::MiningFatigue, 6000, 2);
                self.player_splashes.push((p.id, PlayerSplash { effects: vec![effect], instant: Vec::new(), extinguish: false, at: position }));
                // `GUARDIAN_ELDER_EFFECT`: the curse's sound at the player.
                if let Some(e) = self.mob_mut(id) {
                    e.voices.push((Voice::Event("entity.elder_guardian.curse", 1.0, 1.0), p.position));
                }
            }
        }
    }
}
