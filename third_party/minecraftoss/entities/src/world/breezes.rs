//! Breezes (26.3 `Breeze`, `BreezeAi`, `Shoot`, `LongJump`, `Slide`,
//! `BreezeUtil`), the fight's behaviours folded into one goal.
//!
//! - With a target it slides to a spot four to eight blocks behind it (or
//!   off to the side, or away when too close), then inhales for three
//!   quarters of a second and fires a wind charge, and between shots leaps:
//!   half a second's inhale, then a long arc to a spot behind its target,
//!   choosing among five launch angles one that reaches.
//! - Landing, it is ready to shoot again for five seconds; after a slide,
//!   for three.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{MonsterGoalContext, Target};
use crate::species::Kind;

/// The breeze's `Pose`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BreezePose {
    #[default]
    Standing,
    Shooting,
    Inhaling,
    LongJumping,
    Sliding,
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct BreezeCtx {
    pub pose: BreezePose,
    /// Out: the horizontal speed of a jump begun this tick.
    pub jumped: Option<DVec3>,
    pub sound: Option<(&'static str, f32)>,
}

/// A breeze's pose and its jump's carry (`setDiscardFriction`).
#[derive(Clone, Debug, Default)]
pub struct BreezeState {
    pub pose: BreezePose,
    pub pose_start: i32,
    pub carry: Option<DVec3>,
}

/// `BreezeAi`'s activities.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(1, BreezeFightGoal::default());
    selector.add(3, crate::monster_ai::StrollGoal::new(0.6, 0.001));
    selector.add(4, crate::monster_ai::LookAtPlayerGoal { range: 8.0 });
    selector
}

/// The memories its fight's behaviours keep, in ticks left.
#[derive(Clone, Default)]
struct BreezeFightGoal {
    shoot: i32,
    shoot_cooldown: i32,
    charging: i32,
    recovering: i32,
    shooting: Option<i32>,
    jump_cooldown: i32,
    inhaling: i32,
    jump_target: Option<DVec3>,
    jumping: Option<i32>,
    leaving_water: bool,
    slide: Option<i32>,
}

/// `BreezeUtil.randomPointBehindTarget`: four to eight blocks behind where
/// it faces, spread about a gaussian.
fn point_behind(at: DVec3, head_yaw: f32, random: &mut LegacyRandom) -> DVec3 {
    let view = head_yaw + 180.0 + random.next_gaussian() as f32 * 90.0 / 2.0;
    let r = 4.0 + random.next_float() * 4.0;
    // `Vec3.directionFromRotation(0, view)`.
    let a = -view.to_radians() - std::f32::consts::PI;
    at + DVec3::new(f64::from(-a.sin()), 0.0, f64::from(-a.cos())) * f64::from(r)
}

/// `BreezeUtil.hasLineOfSight`: nothing solid between its feet and there,
/// within fifty blocks.
fn clear_to(world: &dyn World, from: DVec3, to: DVec3) -> bool {
    to.distance(from) <= 50.0 && crate::sight::clip(world, from, to).is_none()
}

/// `LongJump.snapToSurface`: the block over the ground ten below, else over
/// a ceiling ten above.
fn snap_to_surface(world: &dyn World, target: DVec3) -> Option<(i32, i32, i32)> {
    let cell = |p: DVec3| (p.x.floor() as i32, p.y.floor() as i32 + 1, p.z.floor() as i32);
    if let Some((_, hit)) = crate::sight::clip(world, target, target - DVec3::new(0.0, 10.0, 0.0)) {
        return Some(cell(hit));
    }
    crate::sight::clip(world, target, target + DVec3::new(0.0, 10.0, 0.0)).map(|(_, hit)| cell(hit))
}

/// `LongJumpUtil.calculateJumpVectorForAngle` without its collision check.
fn jump_vector(from: DVec3, to: DVec3, max: f32, angle: i32) -> Option<DVec3> {
    let plane = DVec3::new(to.x - from.x, 0.0, to.z - from.z).normalize_or_zero() * 0.5;
    let d = to - plane - from;
    let rad = f64::from(angle as f32 * std::f32::consts::PI / 180.0);
    let xz = d.z.atan2(d.x);
    let r2 = d.x * d.x + d.z * d.z;
    let r = r2.sqrt();
    let g = 0.08;
    let v0sqr = r2 * g / (r * (2.0 * rad).sin() - 2.0 * d.y * rad.cos().powi(2));
    if v0sqr < 0.0 {
        return None;
    }
    let v0 = v0sqr.sqrt();
    if v0 > f64::from(max) {
        return None;
    }
    let (v0r, v0y) = (v0 * rad.cos(), v0 * rad.sin());
    Some(DVec3::new(v0r * xz.cos(), v0y, v0r * xz.sin()) * f64::from(0.95_f32))
}

impl BreezeFightGoal {
    fn set_pose(ctx: &mut MonsterGoalContext, pose: BreezePose) {
        ctx.breeze.pose = pose;
    }

    /// `LongJump.canRun`.
    fn can_jump(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !ctx.body.on_ground && !ctx.fluid.in_water() || ctx.fluid.in_lava() || ctx.fluid.water_height > 0.4 {
            return false;
        }
        if self.jump_target.is_some() {
            return true;
        }
        let Some(target) = ctx.target() else { return false };
        let p = ctx.body.position;
        if target.position.distance(p) >= ctx.follow_range {
            ctx.clear_target();
            return false;
        }
        if target.position.distance(p) - 4.0 <= 0.0 {
            return false;
        }
        let feet = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        if world.block(feet).is_some_and(|b| b.id == "minecraft:honey_block") {
            return false;
        }
        for i in 1..=4 {
            let c = (feet.0, feet.1 + i, feet.2);
            if world.block(c).is_some_and(|b| !matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:water")) {
                return false;
            }
        }
        let head_yaw = match target.target {
            Target::Player(id) => ctx.views.iter().find(|(p, _)| *p == id).map_or(0.0, |(_, v)| v.head_yaw),
            _ => 0.0,
        };
        let behind = point_behind(target.position, head_yaw, &mut ctx.random);
        let Some(spot) = snap_to_surface(world, behind) else { return false };
        let below = (spot.0, spot.1 - 1, spot.2);
        if world.block(below).is_some_and(|b| matches!(b.id.as_str(), "minecraft:lava" | "minecraft:fire" | "minecraft:soul_fire" | "minecraft:magma_block" | "minecraft:campfire" | "minecraft:soul_campfire" | "minecraft:cactus" | "minecraft:sweet_berry_bush" | "minecraft:wither_rose" | "minecraft:powder_snow")) {
            return false;
        }
        let centre = DVec3::new(f64::from(spot.0) + 0.5, f64::from(spot.1) + 0.5, f64::from(spot.2) + 0.5);
        if !clear_to(world, p, centre) && !clear_to(world, p, centre + DVec3::new(0.0, 4.0, 0.0)) {
            return false;
        }
        self.jump_target = Some(DVec3::new(f64::from(spot.0) + 0.5, f64::from(spot.1), f64::from(spot.2) + 0.5));
        true
    }
}

impl Goal<MonsterGoalContext> for BreezeFightGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some() || self.slide.is_some() || self.jumping.is_some()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        if ctx.breeze.pose != BreezePose::Standing {
            Self::set_pose(ctx, BreezePose::Standing);
        }
        self.shooting = None;
        self.jumping = None;
        self.jump_target = None;
        self.slide = None;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        for timer in [&mut self.shoot, &mut self.shoot_cooldown, &mut self.charging, &mut self.recovering, &mut self.jump_cooldown, &mut self.inhaling] {
            *timer = (*timer - 1).max(0);
        }
        let p = ctx.body.position;
        // `SlideToTargetSink`: the walk to its slide's spot, then a shot.
        if let Some(left) = self.slide {
            if ctx.navigation.is_done() || left <= 0 {
                self.slide = None;
                ctx.navigation.stop();
                Self::set_pose(ctx, BreezePose::Standing);
                if ctx.target().is_some() {
                    self.shoot = 60;
                }
            } else {
                self.slide = Some(left - 1);
            }
            return;
        }
        let Some(target) = ctx.target() else { return };
        // `Shoot`.
        if let Some(elapsed) = self.shooting {
            if self.shoot > 0 && elapsed < 20 {
                self.shooting = Some(elapsed + 1);
                let eye = target.position + DVec3::new(0.0, f64::from(target.eye_height), 0.0);
                ctx.look_control.set_look_at_with_limits(eye, 30.0, 30.0);
                if self.charging == 0 && self.recovering == 0 {
                    self.recovering = 4;
                    ctx.shoot = Some((target.target, 0.7));
                }
                return;
            }
            self.shooting = None;
            if ctx.breeze.pose == BreezePose::Shooting {
                Self::set_pose(ctx, BreezePose::Standing);
            }
            self.shoot_cooldown = 10;
            self.shoot = 0;
        } else if self.shoot > 0 && self.shoot_cooldown == 0 && self.charging == 0 && self.recovering == 0 && self.jump_target.is_none() && ctx.breeze.pose == BreezePose::Standing {
            if target.position.distance_squared(p) < 256.0 {
                self.shooting = Some(0);
                Self::set_pose(ctx, BreezePose::Shooting);
                self.charging = 15;
                ctx.breeze.sound = Some(("entity.breeze.inhale", 1.0));
                return;
            }
            self.shoot = 0;
        }
        // `LongJump`.
        if let Some(elapsed) = self.jumping {
            let pose = ctx.breeze.pose;
            if pose != BreezePose::Standing && self.jump_cooldown == 0 && elapsed < 200 {
                self.jumping = Some(elapsed + 1);
                if !ctx.fluid.in_water() {
                    self.leaving_water = false;
                }
                if pose == BreezePose::Inhaling && self.inhaling == 0 {
                    // `calculateOptimalJumpVector`: the angles shuffled.
                    let mut angles = [40, 55, 60, 75, 80];
                    for i in (2..=angles.len()).rev() {
                        let j = ctx.random.next_int(i as u32) as usize;
                        angles.swap(i - 1, j);
                    }
                    let max = 0.058_333_334 * ctx.follow_range as f32;
                    let vector = self.jump_target.and_then(|to| angles.iter().find_map(|&a| jump_vector(p, to, max, a)));
                    match vector {
                        Some(v) => {
                            if ctx.fluid.in_water() {
                                self.leaving_water = true;
                            }
                            ctx.breeze.sound = Some(("entity.breeze.jump", 1.0));
                            Self::set_pose(ctx, BreezePose::LongJumping);
                            ctx.leap = Some(v);
                            ctx.breeze.jumped = Some(v);
                        }
                        None => Self::set_pose(ctx, BreezePose::Standing),
                    }
                } else if pose == BreezePose::LongJumping && (ctx.body.on_ground || ctx.fluid.in_water() && !self.leaving_water) {
                    ctx.breeze.sound = Some(("entity.breeze.land", 1.0));
                    Self::set_pose(ctx, BreezePose::Standing);
                    let hurt = ctx.hurt_by.is_some();
                    self.jump_cooldown = if hurt { 2 } else { 10 };
                    self.shoot = 100;
                }
                return;
            }
            self.jumping = None;
            if matches!(pose, BreezePose::LongJumping | BreezePose::Inhaling) {
                Self::set_pose(ctx, BreezePose::Standing);
            }
            self.jump_target = None;
            self.inhaling = 0;
            self.leaving_water = false;
        } else if self.jump_cooldown == 0 && self.shoot == 0 && self.can_jump(ctx, world) {
            self.jumping = Some(0);
            self.inhaling = 10;
            Self::set_pose(ctx, BreezePose::Inhaling);
            ctx.breeze.sound = Some(("entity.breeze.charge", 1.0));
            if let Some(to) = self.jump_target {
                ctx.look_control.set_look_at_with_limits(to + DVec3::new(0.0, 0.5, 0.0), 30.0, 30.0);
            }
            return;
        }
        // `Slide`.
        if self.jump_cooldown == 0 && self.shoot == 0 && ctx.body.on_ground && !ctx.fluid.in_water() && ctx.breeze.pose == BreezePose::Standing {
            let Some(target) = ctx.target() else { return };
            let enemy = target.position;
            let mut to = None;
            // `withinInnerCircleRange`: within four blocks.
            if enemy.distance_squared(p) <= 16.0 {
                let profile = ctx.profile();
                if let Some(away) = crate::stroll::default_random_position_away(world, &profile, p, 5, 5, enemy, &mut ctx.random) {
                    if clear_to(world, p, away) && enemy.distance_squared(away) > enemy.distance_squared(p) {
                        to = Some(away);
                    }
                }
            }
            let to = to.unwrap_or_else(|| {
                if ctx.random.next_boolean() {
                    let head_yaw = match target.target {
                        Target::Player(id) => ctx.views.iter().find(|(q, _)| *q == id).map_or(0.0, |(_, v)| v.head_yaw),
                        _ => 0.0,
                    };
                    point_behind(enemy, head_yaw, &mut ctx.random)
                } else {
                    // `randomPointInMiddleCircle`: four to eight short of it.
                    let d = enemy - p;
                    let distance = d.length() - (8.0 + ctx.random.next_double() * (4.0 - 8.0));
                    p + d.normalize_or_zero() * distance
                }
            });
            let profile = ctx.profile();
            let cell = DVec3::new(to.x.floor() + 0.5, to.y.floor(), to.z.floor() + 0.5);
            let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, cell, 0.6, 1);
            // `MoveToTargetSink(20, 40)` with the slide's sound and pose.
            self.slide = Some(20 + ctx.random.next_int(21) as i32);
            ctx.breeze.sound = Some(("entity.breeze.slide", 1.0));
            Self::set_pose(ctx, BreezePose::Sliding);
        }
    }
}

impl MobEntity {
    /// Before its goals: its pose for them.
    pub(super) fn breeze_prepare(&mut self) {
        if self.kind == Kind::Breeze {
            self.ai.state.breeze.pose = self.breeze.pose;
        }
    }

    /// After its move: the pose its goals left, its sounds, and a jump's
    /// carry through the air (no friction until it lands).
    pub(super) fn breeze_settle(&mut self) {
        if self.kind != Kind::Breeze {
            return;
        }
        let ctx = std::mem::take(&mut self.ai.state.breeze);
        if ctx.pose != self.breeze.pose {
            self.breeze.pose = ctx.pose;
            self.breeze.pose_start = self.tick_count;
        }
        if let Some((sound, volume)) = ctx.sound {
            self.voices.push((Voice::Event(sound, volume, 1.0), self.body.position));
        }
        if let Some(v) = ctx.jumped {
            self.breeze.carry = Some(DVec3::new(v.x, 0.0, v.z));
        }
        if self.breeze.pose != BreezePose::LongJumping {
            self.breeze.carry = None;
        }
        if let Some(carry) = self.breeze.carry {
            if !self.body.on_ground {
                self.body.velocity.x = carry.x;
                self.body.velocity.z = carry.z;
            }
        }
    }
}
