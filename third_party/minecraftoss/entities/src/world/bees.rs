//! Bees (26.3 `Bee` and its goals).
//!
//! - A bee finds a hive or nest with room within 20 blocks
//!   (`BeeLocateHiveGoal`) and goes in (`BeeGoToHiveGoal`,
//!   `BeeEnterHiveGoal`) when it carries nectar, has searched three
//!   minutes in vain, or bees stay in (night, rain) — not after stinging,
//!   while chasing, or with fire beside the hive.
//! - Without nectar and out of rain it finds a flower within five blocks
//!   and hovers about it (`BeePollinateGoal`); twenty seconds of that and
//!   it carries nectar. Bored after half a minute it seeks the flower it
//!   knows (`BeeGoToKnownFlowerGoal`).
//! - Laden, it grows the crops below it now and then, ten at most
//!   (`BeeGrowCropGoal`), dripping nectar.
//! - Hurt, it and the bees about it grow angry for 20 to 39 seconds and
//!   chase (`BeeAttackGoal` at 1.4) to sting once: the sting poisons (ten
//!   seconds on normal, 18 on hard), loses the stinger, and the bee dies
//!   soon after. Close to its target it rolls.
//! - It drowns after a second under water.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// A bee's hive, flower, nectar and temper.
#[derive(Clone, Debug, Default)]
pub struct BeeState {
    pub hive: Option<Cell>,
    pub flower: Option<Cell>,
    pub has_nectar: bool,
    pub has_stung: bool,
    /// `stayOutOfHiveCountdown` (`CannotEnterHiveTicks`).
    pub stay_out: i32,
    /// `ticksWithoutNectarSinceExitingHive` (`TicksSincePollination`).
    pub ticks_without_nectar: i32,
    /// `numCropsGrownSincePollination`.
    pub crops_grown: i32,
    hive_cooldown: i32,
    flower_cooldown: i32,
    /// Ticks of anger left (`remainingPersistentAngerTime`).
    pub anger: i32,
    under_water: i32,
    time_since_sting: i32,
    pub roll: f32,
    pub roll_o: f32,
    /// It went into its hive this tick.
    pub enter: Option<Cell>,
    /// A crop it grew this tick.
    pub grow: Option<(Cell, minecraftoss_player::Block)>,
}

impl BeeState {
    /// `Bee.getRollAmount`.
    pub fn roll(&self, partial: f32) -> f32 {
        self.roll_o + (self.roll - self.roll_o) * partial
    }
}

/// What a bee's goals share.
#[derive(Clone, Debug, Default)]
pub struct BeeCtx {
    pub hive: Option<Cell>,
    pub flower: Option<Cell>,
    pub has_nectar: bool,
    pub has_stung: bool,
    pub angry: bool,
    pub stay_out: i32,
    pub ticks_without_nectar: i32,
    pub crops_grown: i32,
    pub hive_cooldown: i32,
    pub flower_cooldown: i32,
    pub pollinating: bool,
    /// The hives about it: where, how full, and whether fire burns beside.
    pub hives: Vec<(Cell, usize, bool)>,
    /// `gameplay/bees_stay_in_hive` here, and rain.
    pub stay_in: bool,
    pub raining: bool,
    pub enter: Option<Cell>,
    pub grow: Option<(Cell, minecraftoss_player::Block)>,
}

impl BeeCtx {
    /// `isHiveValid`: a hive the level still has, within 48 blocks.
    fn hive_valid(&self, at: DVec3) -> bool {
        self.hive.is_some_and(|h| within(h, at, 48.0) && self.hives.iter().any(|(c, _, _)| *c == h))
    }

    /// `wantsToEnterHive`.
    fn wants_to_enter(&self, target: bool) -> bool {
        if self.stay_out > 0 || self.pollinating || self.has_stung || target {
            return false;
        }
        let wants = self.has_nectar || self.ticks_without_nectar > 3600 || self.stay_in;
        let near_fire = self.hive.is_some_and(|h| self.hives.iter().any(|(c, _, fire)| *c == h && *fire));
        wants && !near_fire
    }
}

/// `BlockPos.closerThan` (to the block's corner, as vanilla measures it).
fn within(c: Cell, at: DVec3, distance: f64) -> bool {
    DVec3::new(f64::from(c.0), f64::from(c.1), f64::from(c.2)).distance_squared(at.floor()) < distance * distance
}

/// `BlockPos.closerToCenterThan`.
fn near_center(c: Cell, at: DVec3, distance: f64) -> bool {
    DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5).distance_squared(at) < distance * distance
}

fn center(c: Cell) -> DVec3 {
    DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5)
}

/// `Bee.attractsBees`: `#bee_attractive`, dry, a sunflower's top half.
fn attracts(world: &dyn World, c: Cell) -> bool {
    let Some(block) = world.block(c) else { return false };
    if !world.block_in_tag(c, "minecraft:bee_attractive") || block.property("waterlogged") == Some("true") {
        return false;
    }
    block.id != "minecraft:sunflower" || block.property("half") == Some("upper")
}

/// `Bee.registerGoals`.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, BeeAttackGoal { inner: crate::mob_ai::ChargeGoal::new(1.4) });
    selector.add(1, BeeEnterHiveGoal);
    selector.add(2, crate::mob_ai::BreedGoal::new());
    selector.add(3, crate::mob_ai::TemptGoal::new(1.25));
    selector.add(4, BeePollinateGoal { successful: 0, ticks: 0, last_sound: 0, hover: None });
    selector.add(5, BeeLocateHiveGoal);
    selector.add(5, BeeGoToHiveGoal { travelling: 0 });
    selector.add(6, BeeGoToKnownFlowerGoal { travelling: 0 });
    selector.add(7, BeeGrowCropGoal);
    selector.add(8, BeeWanderGoal);
    selector
}

/// `BeeHurtByOtherGoal` (with alerts): who hurt it, and its friends.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, crate::monster_ai::HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: true });
    selector
}

/// `BeeAttackGoal`: the chase, while angry and still armed.
#[derive(Clone)]
struct BeeAttackGoal {
    inner: crate::mob_ai::ChargeGoal,
}

impl Goal<MonsterGoalContext> for BeeAttackGoal {
    fn controls(&self) -> Controls {
        self.inner.controls()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.bee.angry && !ctx.bee.has_stung && self.inner.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.bee.angry && !ctx.bee.has_stung && self.inner.can_continue(ctx, world)
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.inner.stop(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.inner.tick_world(ctx, world);
    }
}

/// `BeeEnterHiveGoal`: at the hive's face with room, in it goes.
#[derive(Clone)]
struct BeeEnterHiveGoal;

impl Goal<MonsterGoalContext> for BeeEnterHiveGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let target = ctx.target.is_some();
        let b = &mut ctx.bee;
        let Some(hive) = b.hive else { return false };
        if b.angry || !b.wants_to_enter(target) || !near_center(hive, ctx.body.position, 2.0) {
            return false;
        }
        match b.hives.iter().find(|(c, _, _)| *c == hive) {
            Some(&(_, count, _)) if count < 3 => {
                b.enter = Some(hive);
            }
            Some(_) => b.hive = None,
            None => {}
        }
        false
    }
}

/// `BeeLocateHiveGoal`: every ten seconds without one, the nearest with
/// room within 20 blocks.
#[derive(Clone)]
struct BeeLocateHiveGoal;

impl Goal<MonsterGoalContext> for BeeLocateHiveGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let target = ctx.target.is_some();
        let at = ctx.body.position;
        let b = &mut ctx.bee;
        if b.angry || b.hive_cooldown != 0 || b.hive.is_some() || !b.wants_to_enter(target) {
            return false;
        }
        b.hive_cooldown = 200;
        let here = at.floor();
        b.hive = b
            .hives
            .iter()
            .filter(|(c, count, _)| *count < 3 && DVec3::new(f64::from(c.0), f64::from(c.1), f64::from(c.2)).distance_squared(here) <= 20.0 * 20.0)
            .min_by(|a, b2| center(a.0).distance_squared(at).total_cmp(&center(b2.0).distance_squared(at)))
            .map(|(c, _, _)| *c);
        false
    }
}

/// `Bee.pathfindRandomlyTowards`: a step of up to six blocks its way.
fn towards(ctx: &mut MonsterGoalContext, target: Cell) -> DVec3 {
    let at = ctx.body.position;
    let goal = DVec3::new(f64::from(target.0) + 0.5, f64::from(target.1), f64::from(target.2) + 0.5);
    let delta = goal - at;
    let reach = delta.length().min(6.0);
    let jitter = DVec3::new(ctx.random.next_float() as f64 - 0.5, ctx.random.next_float() as f64 - 0.5, ctx.random.next_float() as f64 - 0.5);
    at + delta.normalize_or_zero() * reach + jitter
}

/// `BeeGoToHiveGoal`: home, giving up after two minutes on the way.
#[derive(Clone)]
struct BeeGoToHiveGoal {
    travelling: i32,
}

impl BeeGoToHiveGoal {
    fn usable(ctx: &MonsterGoalContext, world: &dyn World) -> bool {
        let b = &ctx.bee;
        let Some(hive) = b.hive else { return false };
        !b.angry
            && within(hive, ctx.body.position, 48.0)
            && b.wants_to_enter(ctx.target.is_some())
            && !within(hive, ctx.body.position, 2.0)
            && world.block_in_tag(hive, "minecraft:beehives")
    }
}

impl Goal<MonsterGoalContext> for BeeGoToHiveGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        Self::usable(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        Self::usable(ctx, world)
    }
    fn start(&mut self, _ctx: &mut MonsterGoalContext) {
        self.travelling = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.travelling = 0;
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(hive) = ctx.bee.hive else { return };
        self.travelling += 1;
        if self.travelling > 1200 {
            ctx.bee.hive = None;
            ctx.bee.hive_cooldown = 200;
            return;
        }
        if ctx.fly_target.is_some() {
            return;
        }
        let at = ctx.body.position;
        let to = if within(hive, at, 16.0) { center(hive) } else { towards(ctx, hive) };
        ctx.fly_target = Some((to, 1.0));
    }
}

/// `BeeGoToKnownFlowerGoal`.
#[derive(Clone)]
struct BeeGoToKnownFlowerGoal {
    travelling: i32,
}

impl BeeGoToKnownFlowerGoal {
    fn usable(ctx: &MonsterGoalContext) -> bool {
        let b = &ctx.bee;
        !b.angry && b.flower.is_some_and(|f| !within(f, ctx.body.position, 2.0)) && b.ticks_without_nectar > 600
    }
}

impl Goal<MonsterGoalContext> for BeeGoToKnownFlowerGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        Self::usable(ctx)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        Self::usable(ctx)
    }
    fn start(&mut self, _ctx: &mut MonsterGoalContext) {
        self.travelling = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.travelling = 0;
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(flower) = ctx.bee.flower else { return };
        self.travelling += 1;
        if self.travelling > 1200 || !within(flower, ctx.body.position, 48.0) {
            ctx.bee.flower = None;
            ctx.bee.flower_cooldown = 20 + ctx.random.next_int(41) as i32;
            return;
        }
        if ctx.fly_target.is_none() {
            let to = towards(ctx, flower);
            ctx.fly_target = Some((to, 1.0));
        }
    }
}

/// `BeePollinateGoal`.
#[derive(Clone)]
struct BeePollinateGoal {
    successful: i32,
    ticks: i32,
    last_sound: i32,
    hover: Option<DVec3>,
}

impl BeePollinateGoal {
    /// `findNearbyFlower`: by Manhattan distance, out to five.
    fn find(world: &dyn World, at: DVec3) -> Option<Cell> {
        let (x, y, z) = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
        for d in 0..=15 {
            for dx in -5i32..=5 {
                for dy in -5i32..=5 {
                    let rest = d - dx.abs() - dy.abs();
                    if !(0..=5).contains(&rest) {
                        continue;
                    }
                    for dz in if rest == 0 { vec![0] } else { vec![rest, -rest] } {
                        let c = (x + dx, y + dy, z + dz);
                        if attracts(world, c) {
                            return Some(c);
                        }
                    }
                }
            }
        }
        None
    }

    fn flower_point(c: Cell) -> DVec3 {
        DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + f64::from(0.6_f32), f64::from(c.2) + 0.5)
    }
}

impl Goal<MonsterGoalContext> for BeePollinateGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let b = &mut ctx.bee;
        if b.angry || b.flower_cooldown > 0 || b.has_nectar || b.raining {
            return false;
        }
        match Self::find(world, ctx.body.position) {
            Some(c) => {
                b.flower = Some(c);
                ctx.fly_target = Some((center(c), 1.2));
                true
            }
            None => {
                b.flower_cooldown = 20 + ctx.random.next_int(41) as i32;
                false
            }
        }
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let b = &ctx.bee;
        if b.angry || !b.pollinating || b.flower.is_none() || b.raining {
            return false;
        }
        if self.successful > 400 {
            ctx.random.next_float() < 0.2
        } else {
            true
        }
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.successful = 0;
        self.ticks = 0;
        self.last_sound = 0;
        self.hover = None;
        ctx.bee.pollinating = true;
        ctx.bee.ticks_without_nectar = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        if self.successful > 400 {
            ctx.bee.has_nectar = true;
            ctx.bee.ticks_without_nectar = 0;
        }
        ctx.bee.pollinating = false;
        ctx.fly_target = None;
        ctx.bee.flower_cooldown = 200;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(flower) = ctx.bee.flower else { return };
        self.ticks += 1;
        if self.ticks > 600 {
            ctx.bee.flower = None;
            ctx.bee.pollinating = false;
            ctx.bee.flower_cooldown = 200;
            return;
        }
        let point = Self::flower_point(flower);
        let at = ctx.body.position;
        if point.distance(at) > 1.0 {
            self.hover = Some(point);
            ctx.fly_target = Some((point, 0.35));
            return;
        }
        let hover = *self.hover.get_or_insert(point);
        let arrived = at.distance(hover) <= 0.1;
        let mut set = true;
        if arrived {
            if ctx.random.next_int(25) == 0 {
                let mut offset = || f64::from((ctx.random.next_float() * 2.0 - 1.0) * 0.333_333_34);
                self.hover = Some(DVec3::new(point.x + offset(), point.y, point.z + offset()));
            } else {
                set = false;
            }
            ctx.look_control.set_look_at(point);
        }
        if set {
            ctx.fly_target = self.hover.map(|h| (h, 0.35));
        }
        self.successful += 1;
        if ctx.random.next_float() < 0.05 && self.successful > self.last_sound + 60 {
            self.last_sound = self.successful;
            ctx.bee_pollinate_sound = true;
        }
    }
}

/// `BeeGrowCropGoal`.
#[derive(Clone)]
struct BeeGrowCropGoal;

/// The next stage of a `#bee_growables` block, if it has one.
fn grown(block: &minecraftoss_player::Block) -> Option<minecraftoss_player::Block> {
    let id = block.id.as_str();
    if matches!(id, "minecraft:cave_vines" | "minecraft:cave_vines_plant") {
        return (block.property("berries") == Some("false")).then(|| block.clone().with("berries", "true"));
    }
    let age: i32 = block.property("age")?.parse().ok()?;
    let max = match id {
        "minecraft:sweet_berry_bush" | "minecraft:beetroots" => 3,
        "minecraft:torchflower_crop" => 2,
        _ => 7,
    };
    (age < max).then(|| block.clone().with("age", &(age + 1).to_string()))
}

impl Goal<MonsterGoalContext> for BeeGrowCropGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if ctx.bee.angry || ctx.bee.crops_grown >= 10 || ctx.random.next_float() < 0.3 {
            return false;
        }
        let at = ctx.body.position;
        ctx.bee.has_nectar && ctx.bee.hive_valid(at)
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if ctx.random.next_int(15) != 0 {
            return;
        }
        let p = ctx.body.position;
        for i in 1..=2 {
            let c = (p.x.floor() as i32, p.y.floor() as i32 - i, p.z.floor() as i32);
            if !world.block_in_tag(c, "minecraft:bee_growables") {
                continue;
            }
            if let Some(next) = world.block(c).as_ref().and_then(grown) {
                ctx.bee.grow = Some((c, next));
                ctx.bee.crops_grown += 1;
                return;
            }
        }
    }
}

/// `BeeWanderGoal`: one tick in ten with nowhere to go, a hover spot ahead
/// (or homeward when strayed far).
#[derive(Clone)]
struct BeeWanderGoal;

impl Goal<MonsterGoalContext> for BeeWanderGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.fly_target.is_some() || ctx.random.next_int(10) != 0 {
            return false;
        }
        let at = ctx.body.position;
        let threshold = if ctx.bee.hive.is_none() && ctx.bee.flower.is_none() { 32.0 } else { 24.0 };
        let homeward = ctx.bee.hive.filter(|&h| ctx.bee.hive_valid(at) && !within(h, at, threshold));
        let spot = match homeward {
            Some(h) => Some(towards(ctx, h)),
            None => crate::mob_ai::random_open(world, ctx, 8, 7, false),
        };
        let Some(spot) = spot else { return false };
        ctx.fly_target = Some((spot, 1.0));
        true
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.fly_target.is_some()
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
    }
}

impl MobEntity {
    /// Before its goals: its state for them, the hives about it, and
    /// `customServerAiStep` (drowning, the sting's toll, the clocks).
    pub(super) fn bee_prepare(&mut self, hives: &[(Cell, usize, bool)], stay_in: bool, raining: bool, zones: &[(Cell, u64, i32)]) {
        let at = self.body.position;
        // `BeehiveBlock.angerNearbyBees` and the robbed hive's bees: those
        // about it with no target turn on the robber.
        if self.ai.state.target.is_none() && !self.bee.has_stung {
            if let Some(&(_, player, _)) = zones.iter().find(|(c, _, _)| {
                let d = at - center(*c);
                d.x.abs() <= 8.5 && d.y.abs() <= 6.5 && d.z.abs() <= 8.5
            }) {
                self.ai.state.set_target(Some(crate::monster_ai::Target::Player(player)));
                self.bee.anger = 400 + self.random.next_int(380) as i32;
            }
        }
        let b = &mut self.bee;
        b.under_water = if self.body.touching_water { b.under_water + 1 } else { 0 };
        let mut hurt = 0.0;
        if b.under_water > 20 {
            hurt = 1.0;
        }
        if b.has_stung {
            b.time_since_sting += 1;
            if b.time_since_sting % 5 == 0 && self.random.next_int((1200 - b.time_since_sting).clamp(1, 1200) as u32) == 0 {
                hurt = self.health;
            }
        }
        if !b.has_nectar {
            b.ticks_without_nectar += 1;
        }
        b.stay_out = (b.stay_out - 1).max(0);
        b.hive_cooldown = (b.hive_cooldown - 1).max(0);
        b.flower_cooldown = (b.flower_cooldown - 1).max(0);
        // Anger runs out; one that targets someone is angry at them.
        if self.ai.state.target.is_some() && b.anger == 0 && !b.has_stung {
            b.anger = 400 + self.random.next_int(380) as i32;
        }
        if b.anger > 0 {
            b.anger -= 1;
            if b.anger == 0 {
                self.ai.state.clear_target();
            }
        }
        // `ValidateHiveGoal` and `ValidateFlowerGoal`, about once a second.
        if self.tick_count % 20 == 0 {
            if let Some(h) = b.hive {
                if !within(h, at, 48.0) || !hives.iter().any(|(c, _, _)| *c == h) {
                    b.hive = None;
                    b.hive_cooldown = 200;
                }
            }
        }
        let ctx = &mut self.ai.state.bee;
        ctx.hive = b.hive;
        ctx.flower = b.flower;
        ctx.has_nectar = b.has_nectar;
        ctx.has_stung = b.has_stung;
        ctx.angry = b.anger > 0;
        ctx.stay_out = b.stay_out;
        ctx.ticks_without_nectar = b.ticks_without_nectar;
        ctx.crops_grown = b.crops_grown;
        ctx.hive_cooldown = b.hive_cooldown;
        ctx.flower_cooldown = b.flower_cooldown;
        ctx.hives = hives.iter().filter(|(c, _, _)| within(*c, at, 48.0)).copied().collect();
        ctx.stay_in = stay_in;
        ctx.raining = raining;
        if hurt > 0.0 {
            self.hurt(hurt);
        }
    }

    /// After its goals: what they changed, its roll, and its requests.
    pub(super) fn bee_settle(&mut self, world: &impl World) {
        let ctx = &mut self.ai.state.bee;
        let b = &mut self.bee;
        b.hive = ctx.hive;
        b.has_nectar = ctx.has_nectar;
        b.ticks_without_nectar = ctx.ticks_without_nectar;
        b.crops_grown = ctx.crops_grown;
        b.hive_cooldown = ctx.hive_cooldown;
        b.flower_cooldown = ctx.flower_cooldown;
        b.enter = ctx.enter.take();
        b.grow = ctx.grow.take();
        // `ValidateFlowerGoal`: a flower gone is forgotten.
        b.flower = ctx.flower.filter(|&f| self.tick_count % 20 != 0 || attracts(world, f));
        let position = self.body.position;
        if std::mem::take(&mut self.ai.state.bee_pollinate_sound) {
            self.voices.push((Voice::Event("entity.bee.pollinate", 1.0, 1.0), position));
        }
        // `updateRollAmount`: rolling while angry, armed and close.
        let target_close = self.ai.state.target().is_some_and(|t| t.position.distance_squared(position) < 4.0);
        let rolling = b.anger > 0 && !b.has_stung && target_close;
        b.roll_o = b.roll;
        b.roll = if rolling { (b.roll + 0.2).min(1.0) } else { (b.roll - 0.24).max(0.0) };
    }

    /// `Bee.doHurtTarget`'s cost: the stinger, and its temper.
    pub(super) fn bee_stung(&mut self) {
        if self.kind == Kind::Bee {
            self.bee.has_stung = true;
            self.bee.anger = 0;
            self.ai.state.clear_target();
        }
    }
}

impl EntityWorld {
    /// The level's hives, and whether bees stay in, for the bees' goals.
    pub fn set_hives(&mut self, hives: Vec<(Cell, usize, bool)>, stay_in: bool) {
        self.hives = hives;
        self.bees_stay_in = stay_in;
        for zone in &mut self.bee_anger_zones {
            zone.2 -= 1;
        }
        self.bee_anger_zones.retain(|z| z.2 > 0);
    }

    /// A hive robbed without smoke by `player`: its bees, and those about
    /// it, turn on them for the next few ticks' worth of arrivals.
    pub fn anger_bees(&mut self, hive: Cell, player: u64) {
        self.bee_anger_zones.push((hive, player, 3));
    }

    /// The bees that went into hives since the last call: each bee and
    /// its hive. The caller hands their saved data to the hive and drops
    /// them.
    pub fn take_hive_entries(&mut self) -> Vec<(u64, Cell)> {
        std::mem::take(&mut self.hive_entries)
    }

    /// A bee's entry and its crop.
    pub(super) fn bee_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let (enter, grow) = (e.bee.enter.take(), e.bee.grow.take());
        if let Some(hive) = enter {
            if let Some(slot) = self.hives.iter_mut().find(|(c, _, _)| *c == hive) {
                slot.1 += 1;
            }
            self.hive_entries.push((id, hive));
        }
        if let Some((cell, block)) = grow {
            // `levelEvent(2011)`: the bonemeal sparkle.
            self.spawned.global_events.push((2011, cell, 15));
            world.set_block(cell, Some(block));
        }
    }
}
