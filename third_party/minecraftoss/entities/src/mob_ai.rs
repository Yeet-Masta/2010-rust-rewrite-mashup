//! The goal sets of the mobs on the generic mob entity (`world::mobs`),
//! over the monster goal framework's shared context. Each kind registers
//! what its class's `registerGoals` does, from the goals here and the
//! monster goals: passive animals float, panic when hurt (`PanicGoal`), go
//! after a player holding their food (`TemptGoal`), stroll and look about;
//! monsters hunt players (`NearestAttackableTargetGoal`) with a melee or
//! ranged attack; neutral mobs only answer whoever hurt them
//! (`HurtByTargetGoal`). Fliers and swimmers wander and close in through
//! the air or water (`WaterAvoidingRandomFlyingGoal`, `RandomSwimmingGoal`,
//! `RandomFloatAroundGoal`), steering towards `fly_target` rather than a
//! ground path. Sources: each class's `registerGoals` and the goal classes
//! named, in the pinned 26.3 JAR.
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{FloatGoal, HurtByTargetGoal, LookAtPlayerGoal, MeleeAttackGoal, MonsterGoalContext, NearestTargetGoal, Prey, RandomLookGoal, StrollGoal};
use crate::navigation::navigate_walk_to;
use crate::species::{AttackKind, Kind, Motion, Temper};
use glam::DVec3;
use minecraftoss_player::World;

/// `PanicGoal(mob, speed)`: hurt in the last two seconds (the last damage
/// source is kept for 40 ticks) or burning, it runs for a random spot up to
/// five blocks out and four up or down (`DefaultRandomPos.getPos(mob, 5, 4)`).
#[derive(Clone)]
pub(crate) struct PanicGoal {
    pub(crate) speed: f64,
    wanted: Option<DVec3>,
}

impl PanicGoal {
    pub(crate) fn new(speed: f64) -> Self {
        Self { speed, wanted: None }
    }
}

/// `getLastDamageSource` is still set, or it burns.
fn panicking(ctx: &MonsterGoalContext) -> bool {
    ctx.on_fire || ctx.hurt_by.is_some_and(|(_, when)| ctx.tick_count - when <= 40)
}

impl Goal<MonsterGoalContext> for PanicGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !panicking(ctx) {
            return false;
        }
        if ctx.mob_motion() != Motion::Ground && ctx.mob_motion() != Motion::Amphibious {
            self.wanted = random_open(world, ctx, 5, 4, ctx.mob_motion() == Motion::Swim);
            return self.wanted.is_some();
        }
        let profile = ctx.profile();
        self.wanted = crate::stroll::default_random_position(world, &profile, ctx.body.position, 5, 4, &mut ctx.random);
        self.wanted.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if ctx.mob_motion() != Motion::Ground && ctx.mob_motion() != Motion::Amphibious {
            return ctx.fly_target.is_some();
        }
        !ctx.navigation.is_done()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(target) = self.wanted.take() else { return };
        if ctx.mob_motion() != Motion::Ground && ctx.mob_motion() != Motion::Amphibious {
            ctx.fly_target = Some((target, self.speed));
            return;
        }
        let profile = ctx.profile();
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, target, self.speed, 0);
    }
}

/// `TemptGoal(mob, speed, items, false)`: a player within ten blocks
/// (`TEMPT_RANGE`) holding its food draws it in, to two and a half blocks,
/// looking at the player all the while; afterwards it waits a hundred
/// ticks before it may be tempted again.
#[derive(Clone)]
pub(crate) struct TemptGoal {
    pub(crate) speed: f64,
    player: Option<u64>,
    calm_down: i32,
}

impl TemptGoal {
    pub(crate) fn new(speed: f64) -> Self {
        Self { speed, player: None, calm_down: 0 }
    }
}

impl Goal<MonsterGoalContext> for TemptGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if self.calm_down > 0 {
            self.calm_down -= 1;
            return false;
        }
        let position = ctx.body.position;
        self.player = ctx
            .players
            .iter()
            .filter(|p| p.alive && !p.spectator && ctx.tempters.contains(&p.id) && p.position.distance_squared(position) < 100.0)
            .min_by(|a, b| a.position.distance_squared(position).total_cmp(&b.position.distance_squared(position)))
            .map(|p| p.id);
        self.player.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_start(ctx, world)
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.player = None;
        ctx.navigation.stop();
        ctx.fly_target = None;
        self.calm_down = 100;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(player) = self.player.and_then(|id| ctx.players.iter().find(|p| p.id == id).copied()) else { return };
        let eye = player.position + DVec3::new(0.0, f64::from(player.eye_height), 0.0);
        let x_max = ctx.max_head_x_rot();
        ctx.look_control.set_look_at_with_limits(eye, 30.0, x_max);
        let near = ctx.body.position.distance_squared(player.position) < 6.25;
        if ctx.mob_motion() != Motion::Ground && ctx.mob_motion() != Motion::Amphibious {
            ctx.fly_target = (!near).then_some((eye, self.speed));
            return;
        }
        if near {
            ctx.navigation.stop();
        } else {
            let profile = ctx.profile();
            let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, player.position, self.speed, 0);
        }
    }
}

/// `BreedGoal(animal, 1.0)`: in love, it seeks the nearest of its kind in
/// love within eight blocks, walks to it, and after three seconds together
/// within three blocks they breed.
#[derive(Clone)]
pub(crate) struct BreedGoal {
    partner: Option<u64>,
    love_time: i32,
}

impl BreedGoal {
    pub(crate) fn new() -> Self {
        Self { partner: None, love_time: 0 }
    }
}

impl Goal<MonsterGoalContext> for BreedGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if !ctx.in_love {
            return false;
        }
        let p = ctx.body.position;
        self.partner = ctx.mates.iter().filter(|(_, at)| at.distance_squared(p) <= 64.0).min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p))).map(|(id, _)| *id);
        self.partner.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.in_love && self.love_time < 60 && self.partner.is_some_and(|id| ctx.mates.iter().any(|(m, _)| *m == id))
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.partner = None;
        self.love_time = 0;
        ctx.fly_target = None;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(at) = self.partner.and_then(|id| ctx.mates.iter().find(|(m, _)| *m == id)).map(|(_, at)| *at) else { return };
        ctx.look_control.set_look_at_with_limits(at + DVec3::new(0.0, 0.5, 0.0), 10.0, ctx.max_head_x_rot());
        if matches!(ctx.mob_motion(), Motion::Ground | Motion::Amphibious) {
            let profile = ctx.profile();
            let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, at, 1.0, 0);
        } else {
            ctx.fly_target = Some((at, 1.0));
        }
        self.love_time += 1;
        // `adjustedTickDelay(60)`: the goal ticks every other tick.
        if self.love_time >= 30 && ctx.body.position.distance_squared(at) < 9.0 {
            ctx.breed_with = self.partner;
        }
    }
}

/// `FollowParentGoal(animal, speed)`: a baby keeps within three blocks of
/// the nearest grown-up of its kind (eight blocks out).
#[derive(Clone)]
pub(crate) struct FollowParentGoal {
    speed: f64,
    time_to_recalc: i32,
}

impl Goal<MonsterGoalContext> for FollowParentGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.parent.is_some_and(|at| at.distance_squared(ctx.body.position) >= 9.0)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.parent.is_some_and(|at| (9.0..=256.0).contains(&at.distance_squared(ctx.body.position)))
    }
    fn start(&mut self, _ctx: &mut MonsterGoalContext) {
        self.time_to_recalc = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.time_to_recalc -= 1;
        if self.time_to_recalc > 0 {
            return;
        }
        self.time_to_recalc = 10;
        let Some(at) = ctx.parent else { return };
        if matches!(ctx.mob_motion(), Motion::Ground | Motion::Amphibious) {
            let profile = ctx.profile();
            let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, at, self.speed, 0);
        } else {
            ctx.fly_target = Some((at, self.speed));
        }
    }
}

/// A random open spot about the mob: air for a flier, water for a swimmer
/// (`AirAndWaterRandomPos`, `BehaviorUtils.getRandomSwimmablePos`): up to
/// `horizontal` out and `vertical` up or down, ten tries.
pub(crate) fn random_open(world: &dyn World, ctx: &mut MonsterGoalContext, horizontal: i32, vertical: i32, water: bool) -> Option<DVec3> {
    let p = ctx.body.position;
    for _ in 0..10 {
        let dx = ctx.random.next_int((2 * horizontal + 1) as u32) as i32 - horizontal;
        let dy = ctx.random.next_int((2 * vertical + 1) as u32) as i32 - vertical;
        let dz = ctx.random.next_int((2 * horizontal + 1) as u32) as i32 - horizontal;
        let at = (p.x.floor() as i32 + dx, p.y.floor() as i32 + dy, p.z.floor() as i32 + dz);
        let Some(block) = world.block(at) else { continue };
        let open = if water { block.id == "minecraft:water" } else { block.id.ends_with("air") };
        if open {
            return Some(DVec3::new(f64::from(at.0) + 0.5, f64::from(at.1), f64::from(at.2) + 0.5));
        }
    }
    None
}

/// `WaterAvoidingRandomFlyingGoal`, `RandomSwimmingGoal` and
/// `RandomFloatAroundGoal`: now and then, a random open spot to drift to.
#[derive(Clone)]
pub(crate) struct WanderGoal {
    speed: f64,
    /// One roll in this many (`interval`).
    interval: u32,
    horizontal: i32,
    vertical: i32,
    water: bool,
}

impl WanderGoal {
    pub(crate) fn swimming(speed: f64, interval: u32) -> Self {
        Self { speed, interval, horizontal: 10, vertical: 7, water: true }
    }
}

impl Goal<MonsterGoalContext> for WanderGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.fly_target.is_some() || ctx.random.next_int(self.interval) != 0 {
            return false;
        }
        // A swimmer out of water stays put (it flops).
        if self.water && !ctx.fluid.in_water() {
            return false;
        }
        match random_open(world, ctx, self.horizontal, self.vertical, self.water) {
            Some(spot) => {
                ctx.fly_target = Some((spot, self.speed));
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

/// `MeleeAttackGoal` through the air or water: it closes on its target's
/// eyes and strikes once a second within reach (`Vex.VexChargeAttackGoal`,
/// `Bee.BeeAttackGoal`, a guardian's or dolphin's chase).
#[derive(Clone)]
pub(crate) struct ChargeGoal {
    speed: f64,
    cooldown: i32,
}

impl ChargeGoal {
    pub(crate) fn new(speed: f64) -> Self {
        Self { speed, cooldown: 0 }
    }
}

impl Goal<MonsterGoalContext> for ChargeGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
        ctx.melee.aggressive = false;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(target) = ctx.target() else { return };
        let eye = target.position + DVec3::new(0.0, f64::from(target.eye_height) * 0.5, 0.0);
        ctx.look_control.set_look_at_with_limits(eye, 30.0, 30.0);
        ctx.fly_target = Some((eye, self.speed));
        ctx.melee.aggressive = true;
        self.cooldown = (self.cooldown - 1).max(0);
        if self.cooldown == 0 && ctx.within_melee_range(target) && ctx.sees(world, target) {
            self.cooldown = 20;
            ctx.melee.swings = ctx.melee.swings.wrapping_add(1);
            ctx.attack = Some(target.target);
        }
    }
}

/// `RangedAttackGoal`, `RangedBowAttackGoal` and the blaze's and ghast's
/// own: in sight within `range`, it holds its ground (closing in when
/// farther) and looses its projectile every `interval` ticks, after a
/// `warmup` of seeing it.
#[derive(Clone)]
pub(crate) struct ShootGoal {
    range: f64,
    interval: i32,
    warmup: i32,
    speed: f64,
    seen: i32,
    cooldown: i32,
}

impl ShootGoal {
    fn new(range: f64, interval: i32, warmup: i32, speed: f64) -> Self {
        Self { range, interval, warmup, speed, seen: 0, cooldown: 0 }
    }
}

impl Goal<MonsterGoalContext> for ShootGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.seen = 0;
        ctx.charging = false;
        ctx.navigation.stop();
        ctx.fly_target = None;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(target) = ctx.target() else { return };
        let eye = target.position + DVec3::new(0.0, f64::from(target.eye_height), 0.0);
        ctx.look_control.set_look_at_with_limits(eye, 30.0, 30.0);
        let distance = ctx.body.position.distance_squared(target.position);
        let sees = ctx.sees(world, target);
        self.seen = if sees { self.seen + 1 } else { 0 };
        let in_range = distance <= self.range * self.range;
        // Close in while it is out of range or out of sight.
        let motion = ctx.mob_motion();
        if !in_range || !sees {
            match motion {
                Motion::Ground | Motion::Amphibious => {
                    let profile = ctx.profile();
                    let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, target.position, self.speed, 0);
                }
                Motion::Stationary => {}
                _ => ctx.fly_target = Some((eye + DVec3::new(0.0, 2.0, 0.0), self.speed)),
            }
        } else if matches!(motion, Motion::Ground | Motion::Amphibious) && distance < (self.range * 0.5).powi(2) {
            ctx.navigation.stop();
        }
        self.cooldown = (self.cooldown - 1).max(0);
        ctx.charging = in_range && sees && self.seen >= self.warmup / 2;
        if self.cooldown == 0 && in_range && sees && self.seen >= self.warmup {
            self.cooldown = self.interval;
            ctx.shoot = Some((target.target, 1.0));
        }
    }
}

/// The prey a kind hunts besides players, as entity types.
fn hunts(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Fox => &["minecraft:chicken", "minecraft:rabbit", "minecraft:cod", "minecraft:salmon", "minecraft:tropical_fish"],
        Kind::Axolotl => &["minecraft:cod", "minecraft:salmon", "minecraft:tropical_fish", "minecraft:pufferfish", "minecraft:squid", "minecraft:glow_squid", "minecraft:drowned", "minecraft:guardian", "minecraft:elder_guardian", "minecraft:tadpole"],
        Kind::Ocelot | Kind::Cat => &["minecraft:chicken", "minecraft:rabbit"],
        Kind::PolarBear => &["minecraft:fox"],
        Kind::Dolphin => &[],
        Kind::SnowGolem => &["minecraft:zombie", "minecraft:husk", "minecraft:drowned", "minecraft:skeleton", "minecraft:stray", "minecraft:bogged", "minecraft:parched", "minecraft:creeper", "minecraft:spider", "minecraft:cave_spider", "minecraft:witch", "minecraft:blaze", "minecraft:pillager", "minecraft:vindicator", "minecraft:evoker", "minecraft:ravager", "minecraft:silverfish", "minecraft:endermite", "minecraft:slime", "minecraft:magma_cube", "minecraft:piglin_brute", "minecraft:hoglin", "minecraft:zoglin", "minecraft:wither_skeleton", "minecraft:breeze", "minecraft:bogged"],
        Kind::Zoglin => &["minecraft:cow", "minecraft:sheep", "minecraft:pig", "minecraft:chicken", "minecraft:villager", "minecraft:wolf", "minecraft:fox", "minecraft:rabbit", "minecraft:horse", "minecraft:goat", "minecraft:llama"],
        Kind::Guardian | Kind::ElderGuardian => &["minecraft:squid", "minecraft:glow_squid", "minecraft:axolotl"],
        _ => &[],
    }
}

/// Whether it goes for villagers too (`AbstractIllager`, raiders, zombies).
fn hunts_villagers(kind: Kind) -> bool {
    matches!(kind, Kind::Vindicator | Kind::Pillager | Kind::Evoker | Kind::Illusioner | Kind::Ravager | Kind::Vex)
}

/// `registerGoals` for a kind on the generic mob: its goal selector.
pub fn goals(kind: Kind) -> GoalSelector<MonsterGoalContext> {
    if kind == Kind::Vex {
        return crate::vex_ai::goals();
    }
    if kind == Kind::Shulker {
        return crate::world::shulker_goals();
    }
    if kind == Kind::Phantom {
        return crate::world::phantom_goals();
    }
    if matches!(kind, Kind::Squid | Kind::GlowSquid) {
        return crate::world::squid_goals();
    }
    if kind == Kind::Dolphin {
        return crate::world::dolphin_goals();
    }
    if matches!(kind, Kind::Guardian | Kind::ElderGuardian) {
        return crate::world::guardian_goals(kind);
    }
    if kind == Kind::Turtle {
        return crate::world::turtle_goals();
    }
    if kind == Kind::Axolotl {
        return crate::world::axolotl_goals();
    }
    let s = kind.species();
    let mut selector = GoalSelector::default();
    let swims = s.motion == Motion::Swim;
    let flies = matches!(s.motion, Motion::Fly | Motion::Hover);
    if !swims && !flies && s.motion != Motion::Stationary {
        selector.add(0, FloatGoal);
    }
    if s.temper == Temper::Passive && !matches!(kind, Kind::WanderingTrader | Kind::CopperGolem | Kind::HappyGhast) {
        let speed = match kind {
            Kind::Rabbit => 2.2,
            Kind::Cat | Kind::Ocelot | Kind::Fox => 1.4,
            Kind::Camel | Kind::CamelHusk => 4.0,
            Kind::Armadillo => 2.0,
            Kind::Goat | Kind::Panda | Kind::Llama | Kind::TraderLlama => 1.25,
            Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => 1.2,
            Kind::Turtle | Kind::Frog => 1.2,
            Kind::Sniffer | Kind::Strider => 1.65,
            _ if s.fish || swims => 1.25,
            _ => 1.5,
        };
        selector.add(1, PanicGoal::new(speed));
    }
    if matches!(kind, Kind::WanderingTrader) {
        selector.add(1, PanicGoal::new(0.5));
    }
    // The attack.
    match s.attack_kind {
        AttackKind::Melee if flies || swims => {
            selector.add(2, ChargeGoal { speed: if kind == Kind::Vex { 1.0 } else { 1.2 }, cooldown: 0 });
        }
        AttackKind::Melee => {
            let spider = kind == Kind::CaveSpider;
            if spider {
                selector.add(3, crate::monster_ai::LeapGoal::new(0.4));
            }
            selector.add(4, MeleeAttackGoal { hits: true, raises_arms: matches!(kind, Kind::ZombifiedPiglin | Kind::Giant | Kind::Vindicator | Kind::Piglin | Kind::PiglinBrute), follow_unseen: true, gives_up_in_light: spider });
        }
        AttackKind::None => {}
        attack => {
            // Range, interval and warm-up (ticks) of each shooter.
            let (range, interval, warmup, speed) = match attack {
                AttackKind::SmallFireball => (16.0, 100, 20, 1.0),
                AttackKind::LargeFireball => (64.0, 40, 20, 1.0),
                AttackKind::WitherSkull => (20.0, 40, 1, 1.0),
                AttackKind::WindCharge => (16.0, 30, 10, 1.0),
                AttackKind::Arrow => (15.0, 40, 20, 1.0),
                AttackKind::ShulkerBullet => (16.0, 40, 20, 0.0),
                AttackKind::Spit => (20.0, 40, 1, 1.25),
                AttackKind::Snowball => (10.0, 20, 1, 1.25),
                AttackKind::Beam => (15.0, 80, 1, 0.5),
                _ => (16.0, 40, 20, 1.0),
            };
            // The evoker's spells are its own (`illagers.rs`); it keeps its
            // distance from players instead (`AvoidEntityGoal(8, 0.6, 1)`).
            // `RangedBowAttackGoal(0.5, 20, 15)` for the illusioner.
            let (interval, speed) = if kind == Kind::Illusioner { (20, 0.5) } else { (interval, speed) };
            if attack == AttackKind::Fangs {
                selector.add(2, crate::monster_ai::AvoidEntityGoal::new("minecraft:player", 8.0, 0.6, 1.0));
            } else {
                selector.add(4, ShootGoal::new(range, interval, warmup, speed));
            }
        }
    }
    if s.temper != Temper::Hostile && !matches!(kind, Kind::Giant) {
        selector.add(3, TemptGoal::new(if flies { 1.0 } else { 1.1 }));
    }
    // `TamableAnimal`s: `SitWhenOrderedToGoal` and `FollowOwnerGoal`
    // (the cat's 10 and 5 blocks, the parrot's 5 and 1).
    match kind {
        Kind::Cat => {
            selector.add(1, crate::wolf_ai::SitGoal);
            selector.add(6, crate::wolf_ai::FollowOwnerGoal { speed: 1.0, start: 10.0, stop: 5.0 });
        }
        Kind::Parrot => {
            selector.add(1, crate::wolf_ai::SitGoal);
            selector.add(2, crate::wolf_ai::FollowOwnerGoal { speed: 1.0, start: 5.0, stop: 1.0 });
        }
        _ => {}
    }
    // `BreedGoal` and `FollowParentGoal` for those that breed.
    if s.breeding_food().is_some() {
        selector.add(2, BreedGoal { partner: None, love_time: 0 });
        selector.add(4, FollowParentGoal { speed: 1.1, time_to_recalc: 0 });
    }
    // Wandering.
    match s.motion {
        Motion::Ground | Motion::Amphibious => {
            if kind != Kind::Giant {
                selector.add(5, StrollGoal::new(if matches!(kind, Kind::Evoker | Kind::Illusioner) { 0.6 } else if s.temper == Temper::Hostile { 1.0 } else { 0.8 }, 0.001));
            }
        }
        Motion::Fly => {
            selector.add(5, WanderGoal { speed: 1.0, interval: 20, horizontal: 8, vertical: 4, water: false });
        }
        Motion::Hover => {
            selector.add(5, WanderGoal { speed: 1.0, interval: 40, horizontal: 16, vertical: 8, water: false });
        }
        Motion::Swim => {
            selector.add(4, WanderGoal { speed: 1.0, interval: if s.fish { 40 } else { 50 }, horizontal: 10, vertical: 7, water: true });
        }
        Motion::Stationary => {}
    }
    if kind != Kind::Giant {
        selector.add(6, LookAtPlayerGoal { range: 8.0 });
        selector.add(7, RandomLookGoal);
    }
    selector
}

/// `registerGoals`' target selector for a kind.
pub fn targets(kind: Kind) -> GoalSelector<MonsterGoalContext> {
    if kind == Kind::Vex {
        return crate::vex_ai::targets();
    }
    if kind == Kind::Shulker {
        return crate::world::shulker_targets();
    }
    if kind == Kind::Phantom {
        return crate::world::phantom_targets();
    }
    if matches!(kind, Kind::Guardian | Kind::ElderGuardian) {
        return crate::world::guardian_targets();
    }
    if kind == Kind::Axolotl {
        return crate::world::axolotl_targets();
    }
    let s = kind.species();
    let mut selector = GoalSelector::default();
    if s.attack_kind == AttackKind::None || s.temper == Temper::Passive && !matches!(kind, Kind::Fox | Kind::Ocelot | Kind::Cat | Kind::Axolotl) {
        return selector;
    }
    if s.temper != Temper::Inert {
        selector.add(1, HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: matches!(kind, Kind::ZombifiedPiglin | Kind::Bee | Kind::Piglin | Kind::Hoglin | Kind::PolarBear | Kind::Dolphin) });
    }
    if s.temper == Temper::Hostile {
        // A piglin spares players in gold (a brute does not).
        let players = if kind == Kind::Piglin { Prey::UngildedPlayer } else { Prey::Player };
        selector.add(2, NearestTargetGoal::every(players, true, 10));
        if hunts_villagers(kind) {
            selector.add(3, NearestTargetGoal::every(Prey::Villager, false, 10));
        }
        if !matches!(kind, Kind::Shulker | Kind::Ghast | Kind::Phantom) {
            selector.add(3, NearestTargetGoal::every(Prey::Golem, true, 10));
        }
    }
    let prey = hunts(kind);
    if !prey.is_empty() {
        selector.add(4, NearestTargetGoal::every(Prey::Kinds(prey), true, 10));
    }
    selector
}

