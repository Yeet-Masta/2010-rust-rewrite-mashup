//! Dolphins (26.3 `Dolphin` and its goals).
//!
//! - A dolphin holds 4800 ticks of air, losing it only with its head
//!   under; with less than seven seconds left it heads straight up for air
//!   (`BreathAirGoal`). It also dries out after two minutes away from water
//!   and rain, and flops about on land.
//! - It swims under `SmoothSwimmingMoveControl`: turning at most 10 degrees
//!   a tick, pitching 5 towards its goal, at a fiftieth of its speed in
//!   water (a tenth ashore, less while it still has to turn), and without
//!   a target it sinks gently (`travelInWater`).
//! - It swims along with a swimming player within ten blocks, granting
//!   Dolphin's Grace now and then (`DolphinSwimWithPlayerGoal`), wanders
//!   (`RandomSwimmingGoal`), looks about, leaps out of open water
//!   (`DolphinJumpGoal`) and fights back with its friends (`HurtByTarget`
//!   with alerts, `MeleeAttackGoal`).
//! - `TryFindLiquidGoal` sets a wanted position with no path behind it, so
//!   its smooth-swimming move control never acts on it: it is left out.
use super::*;
use super::mobs::MobEntity;
use crate::fluid::FluidFrame;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{LookAtPlayerGoal, MonsterGoalContext, RandomLookGoal, Target};
use crate::species::Kind;

/// `Dolphin.TOTAL_AIR_SUPPLY`.
const MAX_AIR: i32 = 4800;
/// `Dolphin.TOTAL_MOISTNESS_LEVEL`.
const MAX_MOISTNESS: i32 = 2400;

/// A dolphin's moisture, its fish, and the player it graced this tick.
#[derive(Clone, Debug, Default)]
pub struct DolphinState {
    pub moisture: i32,
    pub got_fish: bool,
    /// Whether its air and moisture were set full (`finalizeSpawn`).
    pub ready: bool,
    pub grace: Option<u64>,
    /// `DolphinJumpGoal.breached`, which nothing ever clears.
    breached: bool,
}

/// `Dolphin.registerGoals`' goal selector (the treasure hunt, the items
/// and the boats aside).
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, BreathAirGoal);
    selector.add(2, SwimWithPlayerGoal { player: None });
    selector.add(4, crate::mob_ai::WanderGoal::swimming(1.0, 10));
    selector.add(4, RandomLookGoal);
    selector.add(5, LookAtPlayerGoal { range: 6.0 });
    selector.add(5, DolphinJumpGoal { breached: false });
    selector.add(6, crate::mob_ai::ChargeGoal::new(1.2));
    selector
}

/// `BreathAirGoal`: up the column for the first block that gives air.
#[derive(Clone)]
struct BreathAirGoal;

/// `BreathAirGoal.givesAir`: open air, or a bubble column.
fn gives_air(world: &dyn World, cell: (i32, i32, i32)) -> bool {
    world.block(cell).is_none_or(|b| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:bubble_column"))
}

impl BreathAirGoal {
    fn find_air(ctx: &mut MonsterGoalContext, world: &dyn World) {
        let p = ctx.body.position;
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let to = (y..=y + 8).find(|&cy| gives_air(world, (x, cy, z))).unwrap_or((p.y + 8.0).floor() as i32);
        ctx.fly_target = Some((DVec3::new(f64::from(x) + 0.5, f64::from(to + 1), f64::from(z) + 0.5), 1.0));
    }
}

impl Goal<MonsterGoalContext> for BreathAirGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.body.air < 140
    }
    fn interruptible(&self) -> bool {
        false
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        ctx.fly_target = None;
        ctx.dolphin.breathing = true;
        Self::find_air(ctx, world);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.dolphin.breathing = false;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        Self::find_air(ctx, world);
    }
}

/// Whether a player is swimming, as its vitals say.
fn swimming(ctx: &MonsterGoalContext, player: u64) -> bool {
    ctx.vitals.iter().any(|(id, v)| *id == player && v.swimming)
}

/// `DolphinSwimWithPlayerGoal(dolphin, 4.0)`.
#[derive(Clone)]
struct SwimWithPlayerGoal {
    player: Option<u64>,
}

impl Goal<MonsterGoalContext> for SwimWithPlayerGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        // `getNearestPlayer` within ten blocks, sight or not.
        let here = ctx.body.position;
        self.player = ctx
            .players
            .iter()
            .filter(|p| p.alive && !p.spectator && p.position.distance_squared(here) <= 100.0)
            .min_by(|a, b| a.position.distance_squared(here).total_cmp(&b.position.distance_squared(here)))
            .map(|p| p.id);
        self.player.is_some_and(|id| swimming(ctx, id) && ctx.target != Some(Target::Player(id)))
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let here = ctx.body.position;
        self.player.is_some_and(|id| swimming(ctx, id) && ctx.players.iter().any(|p| p.id == id && p.position.distance_squared(here) < 256.0))
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.dolphin.grace = self.player;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.player = None;
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(id) = self.player else { return };
        let Some(player) = ctx.players.iter().find(|p| p.id == id).copied() else { return };
        ctx.look_control.set_look_at_with_limits(player.position + DVec3::new(0.0, f64::from(player.eye_height), 0.0), 21.0, 1.0);
        if ctx.body.position.distance_squared(player.position) < 6.25 {
            ctx.fly_target = None;
        } else {
            ctx.fly_target = Some((player.position, 4.0));
        }
        if swimming(ctx, id) && ctx.random.next_int(6) == 0 {
            ctx.dolphin.grace = Some(id);
        }
    }
}

/// `DolphinJumpGoal(dolphin, 10)`: now and then, where the water ahead is
/// open with two blocks of air above it, a leap.
#[derive(Clone)]
struct DolphinJumpGoal {
    breached: bool,
}

/// `Entity.getMotionDirection` (`Direction.fromYRot`): the step of the
/// way it faces.
fn facing_step(yaw: f32) -> (i32, i32) {
    match ((yaw / 90.0 + 0.5).floor() as i32) & 3 {
        0 => (0, 1),
        1 => (-1, 0),
        2 => (0, -1),
        _ => (1, 0),
    }
}

impl Goal<MonsterGoalContext> for DolphinJumpGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Jump])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.random.next_int(5) != 0 {
            return false;
        }
        let (sx, sz) = facing_step(ctx.yaw);
        let p = ctx.body.position;
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let air = |cell| world.block(cell).is_none_or(|b: minecraftoss_player::Block| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"));
        [0, 1, 4, 5, 6, 7].into_iter().all(|i| {
            let (cx, cz) = (x + sx * i, z + sz * i);
            // Water that blocks no motion (`#blocks_dolphin_jump`).
            let water = world.block((cx, y, cz)).is_some_and(|b| matches!(b.id.as_str(), "minecraft:water" | "minecraft:bubble_column" | "minecraft:kelp" | "minecraft:kelp_plant" | "minecraft:seagrass" | "minecraft:tall_seagrass"));
            water && air((cx, y + 1, cz)) && air((cx, y + 2, cz))
        })
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let yd = ctx.body.velocity.y;
        let pitch = ctx.dolphin.pitch;
        let settled = yd * yd < f64::from(0.03_f32) && pitch != 0.0 && pitch.abs() < 10.0 && ctx.fluid.in_water();
        !settled && !ctx.body.on_ground
    }
    fn interruptible(&self) -> bool {
        false
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        let (sx, sz) = facing_step(ctx.yaw);
        ctx.dolphin.leap = Some(DVec3::new(f64::from(sx) * 0.6, 0.7, f64::from(sz) * 0.6));
        ctx.dolphin.jumping = true;
        ctx.fly_target = None;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.dolphin.pitch = 0.0;
        ctx.dolphin.jumping = false;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let already = self.breached;
        if !already {
            let p = ctx.body.position;
            self.breached = world.block((p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)).is_some_and(|b| b.id == "minecraft:water" || b.property("waterlogged") == Some("true"));
        }
        if self.breached && !already {
            ctx.dolphin.breached = true;
        }
        let v = ctx.body.velocity;
        if v.y * v.y < f64::from(0.03_f32) && ctx.dolphin.pitch != 0.0 {
            // `Mth.rotLerp(0.2, xRot, 0)`.
            let pitch = ctx.dolphin.pitch;
            ctx.dolphin.pitch = pitch + 0.2 * wrap(-pitch);
        } else if v.length() > f64::from(1.0e-5_f32) {
            ctx.dolphin.pitch = (-v.y).atan2(v.x.hypot(v.z)).to_degrees() as f32;
        }
    }
}

/// `Mth.wrapDegrees`.
fn wrap(angle: f32) -> f32 {
    let mut a = angle % 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// `Entity.getInputVector`: the input, no longer than one, at `speed`,
/// turned to the yaw.
pub(super) fn relative(input: DVec3, speed: f32, yaw: f32) -> DVec3 {
    let length = input.length_squared();
    if length < 1.0e-7 {
        return DVec3::ZERO;
    }
    let movement = if length > 1.0 { input.normalize() } else { input } * f64::from(speed);
    let (sin, cos) = (f64::from(yaw.to_radians().sin()), f64::from(yaw.to_radians().cos()));
    DVec3::new(movement.x * cos - movement.z * sin, movement.y, movement.z * cos + movement.x * sin)
}

impl MobEntity {
    /// `Entity.move(SELF, deltaMovement)` alone: the motion kept but where
    /// it struck something; returns how far it fell when it landed.
    pub(super) fn move_raw(&mut self, world: &impl World) -> Option<f64> {
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
        landed
    }

    /// `Dolphin.tick` around `LivingEntity.tick`: its air, its goals, its
    /// smooth swimming, then its moisture and flopping; returns the fall
    /// damage a landing deals.
    pub(super) fn tick_dolphin(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[crate::monster_ai::MobCandidate], game_time: i64, difficulty: i32) -> Option<f32> {
        if !self.dolphin.ready {
            self.dolphin.ready = true;
            self.dolphin.moisture = MAX_MOISTNESS;
            self.body.air = MAX_AIR;
        }
        let position = self.body.position;
        // `LivingEntity.baseTick`: the air, which comes back all at once
        // (`increaseAirSupply`).
        if FluidFrame::eye_in_water(world, position, self.eye_height()) {
            self.body.air -= 1;
            if self.body.air <= -20 {
                self.body.air = 0;
                self.hurt(2.0);
            }
        } else {
            self.body.air = MAX_AIR;
        }
        if players.iter().any(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 32.0 * 32.0) {
            self.no_action_time = 0;
        }
        self.body.trim_small_velocity();
        self.no_action_time += 1;
        let fluid = FluidFrame::sample(world, position, self.body.width, self.body.height);
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
        ai.state.dolphin.pitch = self.pitch;
        ai.state.dolphin.grace = None;
        ai.state.random = std::mem::take(&mut self.random);
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut self.ai.state.random);
        let state = &mut self.ai.state.dolphin;
        self.pitch = state.pitch;
        self.dolphin.grace = state.grace.take();
        if let Some(push) = state.leap.take() {
            self.body.velocity += push;
            self.body.needs_sync = true;
        }
        if std::mem::take(&mut state.breached) && !self.dolphin.breached {
            self.dolphin.breached = true;
            self.voices.push((Voice::Event("entity.dolphin.jump", 1.0, 1.0), position));
        }
        let breathing = state.breathing;
        // `SmoothSwimmingMoveControl.tick`.
        let in_water = fluid.in_water();
        if in_water {
            self.body.velocity.y += 0.005;
        }
        let (mut speed, mut input) = (0.0_f32, DVec3::ZERO);
        if let Some((wanted, modifier)) = self.ai.state.fly_target {
            let d = wanted - self.body.position;
            if d.length_squared() < 1.0 {
                self.ai.state.fly_target = None;
            } else if d.length_squared() >= f64::from(2.500_000_3e-7_f32) {
                let heading = (d.z.atan2(d.x).to_degrees() as f32) - 90.0;
                self.yaw = crate::control::rotlerp(self.yaw, heading, 10.0);
                let wanted_speed = (modifier * self.species().speed) as f32;
                if in_water {
                    speed = wanted_speed * 0.02;
                    let horizontal = d.x.hypot(d.z);
                    if d.y.abs() > f64::from(1.0e-5_f32) || horizontal.abs() > f64::from(1.0e-5_f32) {
                        let x_rot = wrap(-(d.y.atan2(horizontal).to_degrees() as f32)).clamp(-85.0, 85.0);
                        self.pitch += wrap(x_rot - self.pitch).clamp(-5.0, 5.0);
                    }
                    let (sin, cos) = self.pitch.to_radians().sin_cos();
                    input = DVec3::new(0.0, f64::from(-sin * wanted_speed), f64::from(cos * wanted_speed));
                } else {
                    let left = wrap(self.yaw - heading).abs();
                    let turning = 1.0 - ((left - 10.0) / 50.0).clamp(0.0, 1.0);
                    speed = wanted_speed * 0.1 * turning;
                    input = DVec3::new(0.0, 0.0, f64::from(speed));
                }
            }
        }
        // `SmoothSwimmingLookControl`: with nowhere to go it levels out.
        if self.ai.state.fly_target.is_none() && !self.ai.state.dolphin.jumping {
            self.pitch += wrap(-self.pitch).clamp(-5.0, 5.0);
        }
        // `BreathAirGoal.tick`'s own extra push and move.
        if breathing {
            self.body.velocity += relative(input, 0.02, self.yaw);
            let _ = self.move_raw(world);
        }
        let landed = if in_water {
            // `Dolphin.travelInWater`.
            self.body.velocity += relative(input, speed, self.yaw);
            let landed = self.move_raw(world);
            self.body.velocity *= 0.9;
            if self.ai.state.target.is_none() {
                self.body.velocity.y -= 0.005;
            }
            landed
        } else if fluid.in_lava() {
            self.body.travel_lava(world, input, self.yaw, fluid.lava_height);
            None
        } else {
            self.body.travel_air_jumping(world, input, speed, self.yaw, false)
        };
        let damage = landed.and_then(|fallen| super::living::fall_damage(&self.body, world, fallen, false, &mut self.voices));
        // `Dolphin.tick`: it dries out, and flops ashore.
        let p = self.body.position;
        let top = (p.x.floor() as i32, (p.y + f64::from(self.body.height)).floor() as i32, p.z.floor() as i32);
        let wet = self.body.touching_water || world.rain_at((p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)) || world.rain_at(top);
        if wet {
            self.dolphin.moisture = MAX_MOISTNESS;
        } else {
            self.dolphin.moisture -= 1;
            if self.dolphin.moisture <= 0 {
                self.hurt(1.0);
            }
            if self.body.on_ground {
                let (a, b) = (self.random.next_float(), self.random.next_float());
                self.body.velocity += DVec3::new(f64::from((a * 2.0 - 1.0) * 0.2), 0.5, f64::from((b * 2.0 - 1.0) * 0.2));
                self.yaw = self.random.next_float() * 360.0;
                self.body.on_ground = false;
                self.body.needs_sync = true;
            }
        }
        // The move control turns body and head with it.
        let ai = &mut self.ai;
        ai.yaw = self.yaw;
        ai.body_rotation.body_yaw = self.yaw;
        ai.state.look_control.head_yaw = self.yaw;
        damage
    }
}

impl EntityWorld {
    /// `DolphinSwimWithPlayerGoal`'s gift: Dolphin's Grace for five
    /// seconds.
    pub(super) fn dolphin_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        if e.kind != Kind::Dolphin {
            return;
        }
        let Some(player) = e.dolphin.grace.take() else { return };
        let at = e.body.position;
        let effect = minecraftoss_player::effects::EffectInstance::new(minecraftoss_player::effects::MobEffect::DolphinsGrace, 100, 0);
        self.player_splashes.push((player, PlayerSplash { effects: vec![effect], instant: Vec::new(), extinguish: false, at }));
    }
}
