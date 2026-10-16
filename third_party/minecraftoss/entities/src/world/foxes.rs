//! Foxes (26.3 `Fox` and its goals).
//!
//! - By day a fox seeks shade (`SeekShelterGoal`) and, out of the sky's
//!   sight with nothing about to alarm it, curls up asleep (`SleepGoal`);
//!   water, a target or thunder wake it.
//! - It hunts chickens and rabbits (a red fox first; a snow fox first
//!   fish), stalking at 1.5 until within six blocks, then crouching, its
//!   head cocked, and pouncing (`StalkPreyGoal`, `FoxPounceGoal`): a leap
//!   that bites within two blocks, or plants its face in snow.
//! - It shies from players it does not trust, wild wolves and polar bears;
//!   it picks ripe sweet berries (`FoxEatBerriesGoal`), keeping one in its
//!   mouth, and sits to look about now and then (`PerchAndSearchGoal`).
//! - What it carries it eats after half a minute if it is food.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{MonsterGoalContext, Target};
use crate::navigation::navigate_walk_to;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// A fox's poses, its pick, and the players it trusts.
#[derive(Clone, Debug, Default)]
pub struct FoxState {
    pub sleeping: bool,
    pub sitting: bool,
    pub crouching: bool,
    pub interested: bool,
    pub pouncing: bool,
    pub faceplanted: bool,
    pub crouch_amount: f32,
    pub crouch_amount_o: f32,
    pub interested_angle: f32,
    pub interested_angle_o: f32,
    pub ticks_since_eaten: i32,
    pub trusted: Vec<u64>,
    /// A berry bush it picked this tick: the bush, its new state, and the
    /// berries dropped beside it.
    pub picked: Option<(Cell, minecraftoss_player::Block, u8)>,
}

impl FoxState {
    /// `getHeadRollAngle`: the cocked head of an interested fox.
    pub fn head_roll(&self, partial: f32) -> f32 {
        (self.interested_angle_o + (self.interested_angle - self.interested_angle_o) * partial) * 0.11 * std::f32::consts::PI
    }

    /// `getCrouchAmount`.
    pub fn crouch(&self, partial: f32) -> f32 {
        self.crouch_amount_o + (self.crouch_amount - self.crouch_amount_o) * partial
    }
}

/// What a fox's goals share.
#[derive(Clone, Debug, Default)]
pub struct FoxCtx {
    pub sleeping: bool,
    pub sitting: bool,
    pub crouching: bool,
    pub interested: bool,
    pub pouncing: bool,
    pub faceplanted: bool,
    pub fully_crouched: bool,
    pub bright: bool,
    pub thundering: bool,
    pub mouth_empty: bool,
    pub picked: Option<(Cell, minecraftoss_player::Block, u8)>,
}

/// `Fox.registerGoals` and `setTargetGoals` (by its kind).
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(1, FaceplantGoal { countdown: 0 });
    selector.add(2, crate::mob_ai::PanicGoal::new(2.2));
    selector.add(3, crate::mob_ai::BreedGoal::new());
    selector.add(4, crate::monster_ai::AvoidEntityGoal::new("minecraft:player", 16.0, 1.6, 1.4));
    selector.add(4, crate::monster_ai::AvoidEntityGoal::new("minecraft:wolf", 8.0, 1.6, 1.4));
    selector.add(4, crate::monster_ai::AvoidEntityGoal::new("minecraft:polar_bear", 8.0, 1.6, 1.4));
    selector.add(5, StalkPreyGoal);
    selector.add(6, FoxPounceGoal);
    selector.add(6, SeekShelterGoal { interval: 50, wanted: None });
    selector.add(7, FoxMeleeGoal { inner: crate::monster_ai::MeleeAttackGoal { hits: true, raises_arms: false, follow_unseen: true, gives_up_in_light: false } });
    selector.add(7, SleepGoal { countdown: 0 });
    selector.add(10, FoxEatBerriesGoal { target: None, next_start: 0, waited: 0, try_ticks: 0 });
    selector.add(10, crate::monster_ai::LeapGoal::new(0.4));
    selector.add(11, crate::monster_ai::StrollGoal::new(1.0, 0.001));
    selector.add(11, FoxSearchForItemsGoal);
    selector.add(12, crate::monster_ai::LookAtPlayerGoal { range: 24.0 });
    selector.add(13, PerchAndSearchGoal { look_time: 0, looks: 0, rel: (0.0, 0.0) });
    selector
}

/// The hunt: chickens and rabbits, and schooling fish.
pub fn targets(snow: bool) -> GoalSelector<MonsterGoalContext> {
    use crate::monster_ai::{NearestTargetGoal, Prey};
    let land = NearestTargetGoal::every(Prey::Kinds(&["minecraft:chicken", "minecraft:rabbit"]), false, 10);
    let fish = NearestTargetGoal::every(Prey::Kinds(&["minecraft:cod", "minecraft:salmon", "minecraft:tropical_fish"]), false, 20);
    let mut selector = GoalSelector::default();
    if snow {
        selector.add(4, fish);
        selector.add(6, land);
    } else {
        selector.add(4, land);
        selector.add(6, fish);
    }
    selector
}

/// `FoxBehaviorGoal.hasShelter`: the sky unseen from its head.
fn sheltered(ctx: &MonsterGoalContext, world: &dyn World) -> bool {
    let p = ctx.body.position;
    let head = (p.x.floor() as i32, (p.y + f64::from(ctx.body.height)).floor() as i32, p.z.floor() as i32);
    !world.can_see_sky(head)
}

/// `FoxBehaviorGoal.alertable`: within 12 blocks (six up and down), a
/// monster, prey or a player it does not trust.
fn alertable(ctx: &MonsterGoalContext) -> bool {
    let p = ctx.body.position;
    let near = |q: DVec3| (q.x - p.x).abs() <= 12.0 && (q.y - p.y).abs() <= 6.0 && (q.z - p.z).abs() <= 12.0 && q.distance_squared(p) <= 144.0;
    ctx.players.iter().any(|pl| pl.alive && pl.attackable && !ctx.fox_trusted.contains(&pl.id) && near(pl.position))
        || ctx.mobs.iter().any(|m| m.alive && near(m.position) && (matches!(m.kind, "minecraft:chicken" | "minecraft:rabbit") || is_monster(m.kind)))
}

/// A `Monster` by its entity type: the generic kinds' category, and the
/// world's own monsters.
fn is_monster(kind: &str) -> bool {
    matches!(kind, "minecraft:zombie" | "minecraft:husk" | "minecraft:drowned" | "minecraft:zombie_villager" | "minecraft:skeleton" | "minecraft:stray" | "minecraft:bogged" | "minecraft:creeper" | "minecraft:spider" | "minecraft:enderman" | "minecraft:witch" | "minecraft:slime" | "minecraft:magma_cube")
        || crate::species::ALL.iter().any(|k| k.species().id == kind && k.species().category == crate::species::Category::Monster)
}

/// `Fox.isPathClear`: nothing solid on the way, three blocks up.
fn path_clear(ctx: &MonsterGoalContext, world: &dyn World, to: DVec3) -> bool {
    let p = ctx.body.position;
    let (zdiff, xdiff) = (to.z - p.z, to.x - p.x);
    let slope = zdiff / xdiff;
    for i in 0..6 {
        let f = f64::from(i as f32 / 6.0);
        let z = if slope == 0.0 { 0.0 } else { zdiff * f };
        let x = if slope == 0.0 { xdiff * f } else { z / slope };
        for j in 1..4 {
            let c = ((p.x + x).floor() as i32, (p.y + f64::from(j)).floor() as i32, (p.z + z).floor() as i32);
            if world.solid(c) {
                return false;
            }
        }
    }
    true
}

fn is_prey(ctx: &MonsterGoalContext, target: Target) -> bool {
    match target {
        Target::Mob(id) => ctx.mobs.iter().any(|m| m.id == id && matches!(m.kind, "minecraft:chicken" | "minecraft:rabbit")),
        _ => false,
    }
}

/// `FaceplantGoal`: a second stuck in the snow.
#[derive(Clone)]
struct FaceplantGoal {
    countdown: i32,
}

impl Goal<MonsterGoalContext> for FaceplantGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Look, Control::Jump, Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fox.faceplanted
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fox.faceplanted && self.countdown > 0
    }
    fn start(&mut self, _ctx: &mut MonsterGoalContext) {
        self.countdown = 20;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fox.faceplanted = false;
    }
    fn tick(&mut self, _ctx: &mut MonsterGoalContext) {
        self.countdown -= 1;
    }
}

/// `StalkPreyGoal`.
#[derive(Clone)]
struct StalkPreyGoal;

impl Goal<MonsterGoalContext> for StalkPreyGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let f = &ctx.fox;
        if f.sleeping || f.crouching || f.interested || ctx.jump {
            return false;
        }
        ctx.target().is_some_and(|t| t.alive && is_prey(ctx, t.target) && t.position.distance_squared(ctx.body.position) > 36.0)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fox.sitting = false;
        ctx.fox.faceplanted = false;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        // Close enough: crouched and still, eyes on it (the pounce checks
        // the way is clear).
        if ctx.target().is_some() {
            ctx.fox.interested = true;
            ctx.fox.crouching = true;
            ctx.navigation.stop();
        } else {
            ctx.fox.interested = false;
            ctx.fox.crouching = false;
        }
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(t) = ctx.target() else { return };
        ctx.look_control.set_look_at_with_limits(t.position + DVec3::new(0.0, f64::from(t.eye_height), 0.0), 10.0, 40.0);
        if t.position.distance_squared(ctx.body.position) <= 36.0 {
            ctx.fox.interested = true;
            ctx.fox.crouching = true;
            ctx.navigation.stop();
        } else {
            let profile = ctx.profile();
            let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, t.position, 1.5, 0);
        }
    }
}

/// `FoxPounceGoal`: the crouched leap.
#[derive(Clone)]
struct FoxPounceGoal;

impl Goal<MonsterGoalContext> for FoxPounceGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Jump])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !ctx.fox.fully_crouched {
            return false;
        }
        let Some(t) = ctx.target().filter(|t| t.alive) else { return false };
        let clear = path_clear(ctx, world, t.position);
        if !clear {
            ctx.fox.crouching = false;
            ctx.fox.interested = false;
        }
        clear
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if !ctx.target().is_some_and(|t| t.alive) {
            return false;
        }
        let yd = ctx.body.velocity.y;
        (yd * yd >= f64::from(0.05_f32) || ctx.fox_pitch.abs() >= 15.0 || !ctx.body.on_ground) && !ctx.fox.faceplanted
    }
    fn interruptible(&self) -> bool {
        false
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.jump = true;
        ctx.fox.pouncing = true;
        ctx.fox.interested = false;
        if let Some(t) = ctx.target() {
            ctx.look_control.set_look_at_with_limits(t.position, 60.0, 30.0);
            let uv = (t.position - ctx.body.position).normalize_or_zero();
            ctx.leap = Some(ctx.body.velocity + DVec3::new(uv.x * 0.8, 0.9, uv.z * 0.8));
        }
        ctx.navigation.stop();
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fox.crouching = false;
        ctx.fox.interested = false;
        ctx.fox.pouncing = false;
        ctx.fox_reset_crouch = true;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let target = ctx.target();
        if let Some(t) = target {
            ctx.look_control.set_look_at_with_limits(t.position, 60.0, 30.0);
        }
        if !ctx.fox.faceplanted {
            let v = ctx.body.velocity;
            if v.y * v.y < f64::from(0.03_f32) && ctx.fox_pitch != 0.0 {
                ctx.fox_pitch += 0.2 * wrap(-ctx.fox_pitch);
            } else {
                let direction = v.x.hypot(v.z);
                let bias = if v.y > 0.0 { 6.5 } else { 1.0 };
                let biased = v.y * bias;
                let len = direction.hypot(biased);
                if len > f64::from(1.0e-5_f32) {
                    ctx.fox_pitch = ((-biased).signum() * (direction / len).acos()).to_degrees() as f32;
                }
            }
        }
        if let Some(t) = target.filter(|t| t.position.distance(ctx.body.position) <= 2.0) {
            ctx.attack = Some(t.target);
        } else if ctx.fox_pitch > 0.0 && ctx.body.on_ground && ctx.body.velocity.y as f32 != 0.0 {
            let p = ctx.body.position;
            if world.block((p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)).is_some_and(|b| b.id == "minecraft:snow") {
                ctx.fox_pitch = 60.0;
                ctx.clear_target();
                ctx.fox.faceplanted = true;
            }
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

/// `SeekShelterGoal` (`FleeSunGoal`): from the open sky under thunder,
/// or every five seconds by day, to somewhere the sky cannot see.
#[derive(Clone)]
struct SeekShelterGoal {
    interval: i32,
    wanted: Option<DVec3>,
}

impl SeekShelterGoal {
    fn hide_spot(ctx: &mut MonsterGoalContext, world: &dyn World) -> Option<DVec3> {
        let p = ctx.body.position;
        for _ in 0..10 {
            let c = (p.x.floor() as i32 + ctx.random.next_int(20) as i32 - 10, p.y.floor() as i32 + ctx.random.next_int(6) as i32 - 3, p.z.floor() as i32 + ctx.random.next_int(20) as i32 - 10);
            if !world.can_see_sky(c) && !world.solid(c) && world.solid((c.0, c.1 - 1, c.2)) {
                return Some(DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1), f64::from(c.2) + 0.5));
            }
        }
        None
    }
}

impl Goal<MonsterGoalContext> for SeekShelterGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.fox.sleeping || ctx.target.is_some() {
            return false;
        }
        let p = ctx.body.position;
        let cell = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        if ctx.fox.thundering && world.can_see_sky(cell) {
            self.wanted = Self::hide_spot(ctx, world);
            return self.wanted.is_some();
        }
        if self.interval > 0 {
            self.interval -= 1;
            return false;
        }
        self.interval = 100;
        if ctx.fox.bright && world.can_see_sky(cell) {
            self.wanted = Self::hide_spot(ctx, world);
            return self.wanted.is_some();
        }
        false
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.navigation.is_done()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        ctx.fox.sitting = false;
        ctx.fox.crouching = false;
        ctx.fox.interested = false;
        ctx.fox.sleeping = false;
        ctx.fox.faceplanted = false;
        let Some(to) = self.wanted.take() else { return };
        let profile = ctx.profile();
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 1.25, 0);
    }
}

/// `FoxMeleeAttackGoal(1.2, true)`: not while sitting, asleep, crouched or
/// stuck in snow.
#[derive(Clone)]
struct FoxMeleeGoal {
    inner: crate::monster_ai::MeleeAttackGoal,
}

impl Goal<MonsterGoalContext> for FoxMeleeGoal {
    fn controls(&self) -> Controls {
        self.inner.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let f = &ctx.fox;
        !f.sitting && !f.sleeping && !f.crouching && !f.faceplanted && self.inner.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.inner.can_continue(ctx, world)
    }
    fn every_tick(&self) -> bool {
        self.inner.every_tick()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        ctx.fox.sitting = false;
        self.inner.start_world(ctx, world);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.inner.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.inner.tick_world(ctx, world);
    }
}

/// `SleepGoal`.
#[derive(Clone)]
struct SleepGoal {
    countdown: i32,
}

impl SleepGoal {
    fn can_sleep(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if self.countdown > 0 {
            self.countdown -= 1;
            return false;
        }
        ctx.fox.bright && sheltered(ctx, world) && !alertable(ctx)
    }
}

impl Goal<MonsterGoalContext> for SleepGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look, Control::Jump])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !ctx.navigation.is_done() {
            return false;
        }
        let was = ctx.fox.sleeping;
        self.can_sleep(ctx, world) || was
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_sleep(ctx, world)
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.countdown = ctx.random.next_int(70) as i32;
        let f = &mut ctx.fox;
        f.sitting = false;
        f.crouching = false;
        f.interested = false;
        f.sleeping = false;
        f.faceplanted = false;
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        let f = &mut ctx.fox;
        f.sitting = false;
        f.crouching = false;
        f.interested = false;
        f.sleeping = true;
        ctx.navigation.stop();
    }
}

/// `FoxEatBerriesGoal(1.2, 12, 1)`: to a ripe sweet berry bush within 12
/// blocks, then two seconds' sniffing and the pick.
#[derive(Clone)]
struct FoxEatBerriesGoal {
    target: Option<Cell>,
    next_start: i32,
    waited: i32,
    try_ticks: i32,
}

fn ripe(world: &dyn World, c: Cell) -> bool {
    world.block(c).is_some_and(|b| b.id == "minecraft:sweet_berry_bush" && b.property("age").and_then(|a| a.parse::<i32>().ok()).is_some_and(|a| a >= 2))
}

impl Goal<MonsterGoalContext> for FoxEatBerriesGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Jump])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.fox.sleeping {
            return false;
        }
        if self.next_start > 0 {
            self.next_start -= 1;
            return false;
        }
        self.next_start = (200 + ctx.random.next_int(200) as i32 + 1) / 2;
        let p = ctx.body.position;
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let mut best: Option<(i32, Cell)> = None;
        for dx in -12..=12 {
            for dz in -12..=12 {
                for dy in -1..=1 {
                    let c = (x + dx, y + dy, z + dz);
                    if ripe(world, c) {
                        let d = dx * dx + dz * dz + dy * dy;
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, c));
                        }
                    }
                }
            }
        }
        self.target = best.map(|(_, c)| c);
        self.target.is_some()
    }
    fn can_continue(&mut self, _ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.try_ticks <= 1200 && self.target.is_some_and(|c| ripe(world, c))
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.waited = 0;
        self.try_ticks = 0;
        ctx.fox.sitting = false;
        let Some(c) = self.target else { return };
        let profile = ctx.profile();
        let to = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1 + 1), f64::from(c.2) + 0.5);
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 1.2, 1);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(c) = self.target else { return };
        let reached = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1 + 1) + 0.5, f64::from(c.2) + 0.5).distance_squared(ctx.body.position) < 4.0;
        if reached {
            self.try_ticks -= 1;
            if self.waited >= 40 {
                // `pickSweetBerries`: one to the mouth if free, the rest dropped.
                let Some(bush) = world.block(c) else { return };
                let age: i32 = bush.property("age").and_then(|a| a.parse().ok()).unwrap_or(2);
                let mut count = 1 + ctx.random.next_int(2) as u8 + u8::from(age == 3);
                if ctx.fox.mouth_empty {
                    ctx.fox_took_berry = true;
                    count -= 1;
                }
                ctx.fox.picked = Some((c, bush.with("age", "1"), count));
                self.target = None;
            } else {
                self.waited += 1;
            }
        } else {
            self.try_ticks += 1;
            if ctx.random.next_float() < 0.05 {
                ctx.fox_sniff = true;
            }
            if self.try_ticks % 100 == 0 {
                let profile = ctx.profile();
                let to = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1 + 1), f64::from(c.2) + 0.5);
                let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 1.2, 1);
            }
        }
    }
}

/// `FoxSearchForItemsGoal`: mouth empty and left in peace, to the first
/// item within eight blocks.
#[derive(Clone)]
struct FoxSearchForItemsGoal;

impl FoxSearchForItemsGoal {
    fn walk(ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(&to) = ctx.fox_items.first() else { return };
        let profile = ctx.profile();
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 1.2, 0);
    }
}

impl Goal<MonsterGoalContext> for FoxSearchForItemsGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let f = &ctx.fox;
        if !f.mouth_empty || ctx.target.is_some() || ctx.hurt_by.is_some() || f.sleeping || f.sitting || f.faceplanted {
            return false;
        }
        ctx.random.next_int(5) == 0 && !ctx.fox_items.is_empty()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.navigation.is_done() && ctx.fox.mouth_empty
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        Self::walk(ctx, world);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if ctx.fox.mouth_empty {
            Self::walk(ctx, world);
        }
    }
}

/// `PerchAndSearchGoal`: sat, looking about two to four times.
#[derive(Clone)]
struct PerchAndSearchGoal {
    look_time: i32,
    looks: i32,
    rel: (f64, f64),
}

impl PerchAndSearchGoal {
    fn reset_look(&mut self, ctx: &mut MonsterGoalContext) {
        let r = std::f64::consts::TAU * f64::from(ctx.random.next_float());
        self.rel = (r.cos(), r.sin());
        self.look_time = (80 + ctx.random.next_int(20) as i32 + 1) / 2;
    }
}

impl Goal<MonsterGoalContext> for PerchAndSearchGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let f = &ctx.fox;
        ctx.hurt_by.is_none() && ctx.random.next_float() < 0.02 && !f.sleeping && ctx.target.is_none() && ctx.navigation.is_done() && !alertable(ctx) && !f.pouncing && !f.crouching
    }
    fn can_continue(&mut self, _ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        self.looks > 0
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.reset_look(ctx);
        self.looks = 2 + ctx.random.next_int(3) as i32;
        ctx.fox.sitting = true;
        ctx.navigation.stop();
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fox.sitting = false;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        self.look_time -= 1;
        if self.look_time <= 0 {
            self.looks -= 1;
            self.reset_look(ctx);
        }
        let p = ctx.body.position;
        let eye = p.y + f64::from(ctx.eye_height);
        ctx.look_control.set_look_at_with_limits(DVec3::new(p.x + self.rel.0, eye, p.z + self.rel.1), 10.0, 40.0);
    }
}

impl MobEntity {
    /// What a fox carries in its mouth (its main hand).
    pub fn fox_mouth(&self) -> Option<String> {
        if self.kind == Kind::Fox {
            self.hand.clone()
        } else {
            None
        }
    }

    /// Before its goals: what it is doing, and what the world shows it.
    pub(super) fn fox_prepare(&mut self, bright: bool, thundering: bool, items: Vec<DVec3>) {
        self.ai.state.fox_items = items;
        let target = self.ai.state.target.is_some();
        let f = &mut self.fox;
        // `Fox.tick`: water, a target or thunder wakes it.
        if self.body.touching_water || target || thundering {
            f.sleeping = false;
        }
        if self.body.touching_water || f.sleeping {
            f.sitting = false;
        }
        let ctx = &mut self.ai.state.fox;
        ctx.sleeping = f.sleeping;
        ctx.sitting = f.sitting;
        ctx.crouching = f.crouching;
        ctx.interested = f.interested;
        ctx.pouncing = f.pouncing;
        ctx.faceplanted = f.faceplanted;
        ctx.fully_crouched = f.crouch_amount >= 3.0;
        ctx.bright = bright;
        ctx.thundering = thundering;
        ctx.mouth_empty = self.hand.is_none();
        self.ai.state.fox_trusted = f.trusted.clone();
        self.ai.state.fox_pitch = self.pitch;
    }

    /// After its goals: the poses they set, its mouth, its crouch and head.
    pub(super) fn fox_settle(&mut self) {
        let position = self.body.position;
        let ctx = &mut self.ai.state.fox;
        let f = &mut self.fox;
        f.sleeping = ctx.sleeping;
        f.sitting = ctx.sitting;
        f.crouching = ctx.crouching;
        f.interested = ctx.interested;
        f.pouncing = ctx.pouncing;
        f.faceplanted = ctx.faceplanted;
        f.picked = ctx.picked.take();
        if std::mem::take(&mut self.ai.state.fox_took_berry) {
            self.hand = Some("minecraft:sweet_berries".to_owned());
            self.fox.ticks_since_eaten = 0;
        }
        if std::mem::take(&mut self.ai.state.fox_sniff) {
            self.voices.push((Voice::Event("entity.fox.sniff", 1.0, 1.0), position));
        }
        if std::mem::take(&mut self.ai.state.fox_reset_crouch) {
            self.fox.crouch_amount = 0.0;
            self.fox.crouch_amount_o = 0.0;
        }
        self.pitch = self.ai.state.fox_pitch;
        // No target, no stalk (`aiStep`).
        if self.ai.state.target().is_none() {
            self.fox.crouching = false;
            self.fox.interested = false;
        }
        // `aiStep`: food in the mouth eaten after half a minute.
        let f = &mut self.fox;
        f.ticks_since_eaten += 1;
        if let Some(item) = self.hand.as_deref() {
            if is_fox_food(item) && self.ai.state.target.is_none() && self.body.on_ground && !f.sleeping {
                if f.ticks_since_eaten > 600 {
                    self.hand = None;
                    f.ticks_since_eaten = 0;
                    let max = Kind::Fox.species().health;
                    self.health = (self.health + 2.0).min(max);
                } else if f.ticks_since_eaten > 560 && self.random.next_float() < 0.1 {
                    self.voices.push((Voice::Event("entity.fox.eat", 1.0, 1.0), position));
                }
            }
        }
        // `Fox.tick`: the head's tilt and the crouch.
        f.interested_angle_o = f.interested_angle;
        let goal = if f.interested { 1.0 } else { 0.0 };
        f.interested_angle += (goal - f.interested_angle) * 0.4;
        f.crouch_amount_o = f.crouch_amount;
        f.crouch_amount = if f.crouching { (f.crouch_amount + 0.2).min(5.0) } else { 0.0 };
        if f.sleeping || f.sitting {
            self.body.velocity.x = 0.0;
            self.body.velocity.z = 0.0;
        }
    }
}

/// What a fox eats from its mouth (foods with `food` and `consumable`).
fn is_fox_food(item: &str) -> bool {
    matches!(
        item,
        "minecraft:sweet_berries" | "minecraft:glow_berries" | "minecraft:apple" | "minecraft:bread" | "minecraft:carrot" | "minecraft:potato" | "minecraft:baked_potato" | "minecraft:beetroot"
            | "minecraft:chicken" | "minecraft:cooked_chicken" | "minecraft:rabbit" | "minecraft:cooked_rabbit" | "minecraft:mutton" | "minecraft:cooked_mutton" | "minecraft:beef" | "minecraft:cooked_beef"
            | "minecraft:porkchop" | "minecraft:cooked_porkchop" | "minecraft:cod" | "minecraft:cooked_cod" | "minecraft:salmon" | "minecraft:cooked_salmon" | "minecraft:melon_slice" | "minecraft:cookie"
            | "minecraft:pumpkin_pie" | "minecraft:rotten_flesh" | "minecraft:spider_eye" | "minecraft:golden_carrot" | "minecraft:golden_apple" | "minecraft:dried_kelp" | "minecraft:tropical_fish"
    )
}

impl EntityWorld {
    /// `Mob.aiStep`'s pickup for those that take things: items touching
    /// its box, a block out to the sides, with no pickup delay, as each
    /// kind wants them (`wantsToPickUp`, `pickUpItem`).
    pub(super) fn mob_pick_up(&mut self, id: u64, world: &mut impl World) {
        if !self.mob_griefing {
            return;
        }
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        if !matches!(e.kind, Kind::Fox | Kind::Piglin | Kind::Panda) || e.health <= 0.0 {
            return;
        }
        let body = &e.body;
        let half = f64::from(body.width) / 2.0;
        let (min, max) = (body.position - DVec3::new(half + 1.0, 0.0, half + 1.0), body.position + DVec3::new(half + 1.0, f64::from(body.height), half + 1.0));
        for item in world.items_in(min, max) {
            if item.count <= 0 || item.pickup_delay > 0 {
                continue;
            }
            let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
            match e.kind {
                // `Fox.canHoldItem` and `Fox.pickUpItem`: one in the jaws,
                // the rest dropped, what it held spat out.
                Kind::Fox => {
                    let held_food = e.hand.as_deref().is_some_and(is_fox_food);
                    let wants = e.hand.is_none() || e.fox.ticks_since_eaten > 0 && is_fox_food(&item.item) && !held_food;
                    if !wants {
                        continue;
                    }
                    let position = e.body.position;
                    let yaw = e.yaw.to_radians();
                    let look = DVec3::new(f64::from(-yaw.sin()), 0.0, f64::from(yaw.cos()));
                    if let Some(old) = e.hand.take() {
                        let spat = minecraftoss_player::inventory::ItemStack::new(&old, 1);
                        e.voices.push((Voice::Event("entity.fox.spit", 1.0, 1.0), position));
                        world.spawn_item(position + look + DVec3::new(0.0, 1.0, 0.0), &spat, DVec3::ZERO, 40);
                    }
                    let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
                    e.hand = Some(item.item.clone());
                    e.fox.ticks_since_eaten = 0;
                    e.persistence_required = true;
                    if item.count > 1 {
                        let rest = minecraftoss_player::inventory::ItemStack::new(&item.item, (item.count - 1).clamp(1, 255) as u8);
                        world.spawn_item(position, &rest, DVec3::ZERO, 0);
                    }
                    world.take_item(item.id, item.count);
                    return;
                }
                // `PiglinAi.wantsToPickup` for its bartering currency: a gold
                // ingot taken in hand and admired.
                Kind::Piglin => {
                    if item.item != "minecraft:gold_ingot" || e.off_hand.is_some() {
                        continue;
                    }
                    if self.piglin_take_gold(id, &item.item) {
                        world.take_item(item.id, 1);
                        return;
                    }
                }
                // `Panda.pickUpItem`: a grown panda with empty paws takes
                // the whole stack of bamboo or cake.
                Kind::Panda => {
                    if e.hand.is_some() || e.baby() || !super::pandas::eats_from_ground(&item.item) {
                        continue;
                    }
                    e.hand = Some(item.item.clone());
                    e.panda.hand_count = item.count;
                    e.persistence_required = true;
                    world.take_item(item.id, item.count);
                    return;
                }
                _ => return,
            }
        }
    }

    /// A fox's picked bush: the bush stripped, the spare berries dropped.
    pub(super) fn fox_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let Some((cell, bush, count)) = e.fox.picked.take() else { return };
        let at = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
        e.voices.push((Voice::Event("block.sweet_berry_bush.pick_berries", 1.0, 1.0), at));
        world.set_block(cell, Some(bush));
        if count > 0 {
            self.spawned.items.push((at, ItemStack::new("minecraft:sweet_berries", count)));
        }
    }
}
