//! Squids and glow squids (26.3 `Squid` and `GlowSquid`).
//!
//! - A squid swims in pulses: its tentacles sweep through the first half of
//!   each stroke, and over the last quarter of that half it is pushed along
//!   its `movementVector`; the rest of the stroke it coasts, slowing.
//! - It picks a fresh random heading now and then, or whenever it has none
//!   or left the water (`SquidRandomMovementGoal`), and stops after a while
//!   with no player about.
//! - Hurt by something within ten blocks while in water, it swims straight
//!   away, blowing bubbles (`SquidFleeGoal`), and squirts a cloud of ink.
//! - Ashore it falls straight down, its tentacles waving, and slowly tips
//!   over on its side.
//! - A glow squid glows, sheds glow motes, and goes dark for five seconds
//!   when hurt.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;
use std::f32::consts::PI;

/// A squid's swim stroke, body tilt and spin, and what it shed this tick.
#[derive(Clone, Debug, Default)]
pub struct SquidState {
    pub x_body_rot: f32,
    pub x_body_rot_o: f32,
    pub z_body_rot: f32,
    pub z_body_rot_o: f32,
    pub tentacle_movement: f32,
    pub tentacle_angle: f32,
    pub old_tentacle_angle: f32,
    tentacle_speed: f32,
    rotate_speed: f32,
    /// `GlowSquid.DATA_DARK_TICKS_REMAINING`.
    pub dark_ticks: i32,
    /// The ink it squirted this tick: each puff's start and motion.
    pub ink: Vec<(DVec3, DVec3)>,
    /// Fleeing, it blew a bubble this tick.
    pub bubble: bool,
}

impl SquidState {
    /// `SquidRenderer.extractRenderState`'s tentacle angle.
    pub fn tentacle_angle(&self, partial: f32) -> f32 {
        self.old_tentacle_angle + (self.tentacle_angle - self.old_tentacle_angle) * partial
    }

    /// The body's tilt and spin, in degrees, between ticks.
    pub fn body_rot(&self, partial: f32) -> (f32, f32) {
        (self.x_body_rot_o + (self.x_body_rot - self.x_body_rot_o) * partial, self.z_body_rot_o + (self.z_body_rot - self.z_body_rot_o) * partial)
    }

    /// `GlowSquidRenderer.getBlockLightLevel`: full while it glows, fading
    /// to the world's light over the last half second of going dark.
    pub fn glow_light(&self, block: f32) -> f32 {
        let glow = ((1.0 - self.dark_ticks as f32 / 10.0).clamp(0.0, 1.0) * 15.0) as i32;
        if glow == 15 {
            15.0
        } else {
            (glow as f32).max(block)
        }
    }
}

/// `Squid.registerGoals`' goal selector.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, SquidRandomMovementGoal);
    selector.add(1, SquidFleeGoal { flee_ticks: 0 });
    selector
}

/// A squid picks no targets.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    GoalSelector::default()
}

/// `Squid.hasMovementVector`.
fn has_vector(ctx: &MonsterGoalContext) -> bool {
    ctx.squid_vector.length_squared() > f64::from(1.0e-5_f32)
}

/// `SquidRandomMovementGoal`.
#[derive(Clone)]
struct SquidRandomMovementGoal;

impl Goal<MonsterGoalContext> for SquidRandomMovementGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, _ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        true
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if ctx.no_action_time > 100 {
            ctx.squid_vector = DVec3::ZERO;
        } else if ctx.random.next_int(25) == 0 || !ctx.fluid.in_water() || !has_vector(ctx) {
            let angle = ctx.random.next_float() * PI * 2.0;
            let y = -0.1 + ctx.random.next_float() * 0.2;
            ctx.squid_vector = DVec3::new(f64::from(angle.cos() * 0.2), f64::from(y), f64::from(angle.sin() * 0.2));
        }
    }
}

/// `SquidFleeGoal`: away from whoever last hurt it, faster the nearer
/// they are.
#[derive(Clone)]
struct SquidFleeGoal {
    flee_ticks: i32,
}

/// Whether a block holds water (`FluidState.is(FluidTags.WATER)`).
fn watery(block: &minecraftoss_player::Block) -> bool {
    matches!(block.id.as_str(), "minecraft:water" | "minecraft:bubble_column" | "minecraft:kelp" | "minecraft:kelp_plant" | "minecraft:seagrass" | "minecraft:tall_seagrass")
        || block.property("waterlogged") == Some("true")
}

impl Goal<MonsterGoalContext> for SquidFleeGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some((attacker, _)) = ctx.hurt_by else { return false };
        ctx.fluid.in_water() && ctx.info(attacker).is_some_and(|a| a.position.distance_squared(ctx.body.position) < 100.0)
    }
    fn start(&mut self, _ctx: &mut MonsterGoalContext) {
        self.flee_ticks = 0;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.flee_ticks += 1;
        let Some(attacker) = ctx.hurt_by.and_then(|(a, _)| ctx.info(a)) else { return };
        let here = ctx.body.position;
        let mut away = here - attacker.position;
        let to = here + away;
        let cell = (to.x.floor() as i32, to.y.floor() as i32, to.z.floor() as i32);
        let block = world.block(cell);
        let air = block.as_ref().is_none_or(|b| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"));
        if air || block.as_ref().is_some_and(watery) {
            // The vector is left at its length (`normalize`'s result goes
            // unused), then scaled by a speed falling off past five blocks.
            let length = away.length();
            if length > 0.0 {
                let mut speed = 3.0;
                if length > 5.0 {
                    speed -= (length - 5.0) / 5.0;
                }
                if speed > 0.0 {
                    away *= speed;
                }
            }
            if air {
                away.y = 0.0;
            }
            ctx.squid_vector = away / 20.0;
        }
        if self.flee_ticks % 10 == 5 {
            ctx.squid_bubble = true;
        }
    }
}

impl MobEntity {
    /// `Squid.aiStep` (and `GlowSquid.aiStep`): its goals, the move along
    /// its motion (`Squid.travel`), then its stroke; returns the fall damage
    /// a landing deals.
    pub(super) fn tick_squid(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[crate::monster_ai::MobCandidate], game_time: i64, difficulty: i32) -> Option<f32> {
        self.squid.ink.clear();
        self.squid.bubble = false;
        if self.ai.state.hurt_by.is_some_and(|(_, when)| self.tick_count - when > 100) {
            self.ai.state.hurt_by = None;
        }
        let position = self.body.position;
        if players.iter().any(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 32.0 * 32.0) {
            self.no_action_time = 0;
        }
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
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut self.ai.state.random);
        self.squid.bubble = std::mem::take(&mut self.ai.state.squid_bubble);
        // `Squid.travel`: just the move, keeping its motion but where it
        // struck something.
        let before = self.body.velocity;
        let gravity = self.body.gravity;
        self.body.gravity = 0.0;
        let landed = self.body.travel_air_jumping(world, DVec3::ZERO, 0.0, self.yaw, false);
        self.body.gravity = gravity;
        let after = &mut self.body.velocity;
        for (now, was) in [(&mut after.x, before.x), (&mut after.y, before.y), (&mut after.z, before.z)] {
            if *now != 0.0 {
                *now = was;
            }
        }
        let damage = landed.and_then(|fallen| super::living::fall_damage(&self.body, world, fallen, false, &mut self.voices));
        // The stroke.
        let sq = &mut self.squid;
        if sq.tentacle_speed == 0.0 {
            sq.tentacle_speed = 1.0 / (self.random.next_float() + 1.0) * 0.2;
        }
        sq.x_body_rot_o = sq.x_body_rot;
        sq.z_body_rot_o = sq.z_body_rot;
        sq.old_tentacle_angle = sq.tentacle_angle;
        sq.tentacle_movement += sq.tentacle_speed;
        if sq.tentacle_movement > PI * 2.0 {
            sq.tentacle_movement -= PI * 2.0;
            if self.random.next_int(10) == 0 {
                sq.tentacle_speed = 1.0 / (self.random.next_float() + 1.0) * 0.2;
            }
        }
        if self.body.touching_water {
            if sq.tentacle_movement < PI {
                let stroke = sq.tentacle_movement / PI;
                sq.tentacle_angle = (stroke * stroke * PI).sin() * PI * 0.25;
                if stroke > 0.75 {
                    self.body.velocity = self.ai.state.squid_vector;
                    self.body.needs_sync = true;
                    sq.rotate_speed = 1.0;
                } else {
                    sq.rotate_speed *= 0.8;
                }
            } else {
                sq.tentacle_angle = 0.0;
                self.body.velocity *= 0.9;
                sq.rotate_speed *= 0.99;
            }
            let v = self.body.velocity;
            let horizontal = v.x.hypot(v.z);
            let body = &mut self.ai.body_rotation.body_yaw;
            *body += (-(v.x.atan2(v.z) as f32).to_degrees() - *body) * 0.1;
            self.ai.yaw = *body;
            sq.z_body_rot += PI * sq.rotate_speed * 1.5;
            sq.x_body_rot += (-(horizontal.atan2(v.y) as f32).to_degrees() - sq.x_body_rot) * 0.1;
        } else {
            sq.tentacle_angle = sq.tentacle_movement.sin().abs() * PI * 0.25;
            let yd = self.body.velocity.y - 0.08;
            self.body.velocity = DVec3::new(0.0, yd * f64::from(0.98_f32), 0.0);
            sq.x_body_rot += (-90.0 - sq.x_body_rot) * 0.02;
        }
        self.yaw = self.ai.yaw;
        self.ai.state.look_control.head_yaw = self.ai.yaw;
        if self.kind == Kind::GlowSquid && sq.dark_ticks > 0 {
            sq.dark_ticks -= 1;
        }
        damage
    }

    /// `Squid.hurtServer`: hurt by something, it squirts its ink;
    /// `GlowSquid.hurtServer`: hurt at all, it goes dark.
    pub(super) fn squid_hurt(&mut self, by_something: bool) {
        if !matches!(self.kind, Kind::Squid | Kind::GlowSquid) {
            return;
        }
        if self.kind == Kind::GlowSquid {
            self.squid.dark_ticks = 100;
        }
        if !by_something {
            return;
        }
        let squirt = if self.kind == Kind::GlowSquid { "entity.glow_squid.squirt" } else { "entity.squid.squirt" };
        let pitch = (self.random.next_float() - self.random.next_float()) * 0.2 + 1.0;
        let position = self.body.position;
        self.voices.push((Voice::Event(squirt, 0.4, pitch), position));
        // `rotateVector`: by its last tilt, then its last body turn.
        let (x_rot, y_rot) = (self.squid.x_body_rot_o.to_radians(), self.ai.body_rotation.body_yaw.to_radians());
        let turn = |v: DVec3| {
            let (c, s) = (f64::from(x_rot.cos()), f64::from(x_rot.sin()));
            let v = DVec3::new(v.x, v.y * c + v.z * s, v.z * c - v.y * s);
            let (c, s) = (f64::from((-y_rot).cos()), f64::from((-y_rot).sin()));
            DVec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
        };
        let origin = turn(DVec3::new(0.0, -1.0, 0.0)) + position + DVec3::new(0.0, 0.5, 0.0);
        let reach = if self.baby() { 0.1 } else { 0.3 };
        for _ in 0..30 {
            let (a, b) = (self.random.next_float(), self.random.next_float());
            let direction = turn(DVec3::new(f64::from(a) * 0.6 - 0.3, -1.0, f64::from(b) * 0.6 - 0.3));
            let offset = direction * f64::from(reach + self.random.next_float() * 2.0);
            self.squid.ink.push((origin, offset * f64::from(0.1_f32)));
        }
    }
}
