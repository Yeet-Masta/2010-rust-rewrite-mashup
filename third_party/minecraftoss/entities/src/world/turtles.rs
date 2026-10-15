//! Turtles (26.3 `Turtle` and its goals).
//!
//! - A turtle remembers the beach it was born or first found on
//!   (`homePos`). Bred, it carries an egg instead of a baby
//!   (`TurtleBreedGoal`), goes home (`TurtleGoHomeGoal`), and on sand
//!   within nine blocks of home digs for ten seconds and lays one to four
//!   eggs (`TurtleLayEggGoal`).
//! - Grown turtles wander back home now and then when more than 64 blocks
//!   off; otherwise in water they set out for somewhere far away
//!   (`TurtleTravelGoal`), and ashore they head back to the water
//!   (`TurtleGoToWaterGoal`) or stroll (`TurtleRandomStrollGoal`).
//! - Growing up, a baby drops a scute (`TURTLE_GROW`).
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{LookAtPlayerGoal, MonsterGoalContext};
use crate::navigation::navigate_walk_to;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// `Level.getSeaLevel` in the overworld.
const SEA_LEVEL: i32 = 63;

/// A turtle's home, its egg and its digging.
#[derive(Clone, Debug, Default)]
pub struct TurtleState {
    pub home: Option<Cell>,
    pub has_egg: bool,
    pub laying_egg: bool,
    pub lay_egg_counter: i32,
    /// An egg block it laid this tick: where, and how many eggs.
    pub laid: Option<(Cell, u8)>,
    /// Where it kicked up sand this tick.
    pub dig: Option<Cell>,
    /// It grew up this tick (its scute).
    pub grew: bool,
}

/// `Turtle.registerGoals`.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::mob_ai::PanicGoal::new(1.2));
    selector.add(1, crate::mob_ai::BreedGoal::new());
    selector.add(1, TurtleLayEggGoal { search: MoveToBlock::new(16, 0) });
    selector.add(2, crate::mob_ai::TemptGoal::new(1.1));
    selector.add(3, TurtleGoToWaterGoal { search: MoveToBlock::new(24, -1) });
    selector.add(4, TurtleGoHomeGoal { stuck: false, close_ticks: 0 });
    selector.add(7, TurtleTravelGoal { stuck: false });
    selector.add(8, LookAtPlayerGoal { range: 8.0 });
    selector.add(9, TurtleStrollGoal);
    selector
}

/// The middle of a cell's floor (`Vec3.atBottomCenterOf`).
fn bottom_center(c: Cell) -> DVec3 {
    DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1), f64::from(c.2) + 0.5)
}

/// `BlockPos.closerToCenterThan`: within `distance` of the cell's centre.
fn near(c: Cell, position: DVec3, distance: f64) -> bool {
    DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5).distance_squared(position) < distance * distance
}

fn is_water(world: &dyn World, c: Cell) -> bool {
    world.block(c).is_some_and(|b| b.id == "minecraft:water")
}

fn is_air(world: &dyn World, c: Cell) -> bool {
    world.block(c).is_none_or(|b| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"))
}

/// `MoveToBlockGoal`'s search and approach.
#[derive(Clone)]
struct MoveToBlock {
    range: i32,
    vertical_start: i32,
    next_start: i32,
    try_ticks: i32,
    max_stay: i32,
    reached: bool,
    target: Cell,
}

impl MoveToBlock {
    fn new(range: i32, vertical_start: i32) -> Self {
        Self { range, vertical_start, next_start: 0, try_ticks: 0, max_stay: 0, reached: false, target: (0, 0, 0) }
    }

    /// `canUse`: every 100 to 200 goal ticks, the nearest valid block.
    fn can_use(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World, valid: &dyn Fn(&dyn World, Cell) -> bool) -> bool {
        if self.next_start > 0 {
            self.next_start -= 1;
            return false;
        }
        self.next_start = (200 + ctx.random.next_int(200) as i32 + 1) / 2;
        let p = ctx.body.position;
        let (mx, my, mz) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        // `findNearestBlock`: rings outward on each level, the levels
        // alternating about the start.
        let mut y = self.vertical_start;
        while y <= 1 {
            for r in 0..self.range {
                let mut x = 0;
                while x <= r {
                    let mut z = if x < r && x > -r { r } else { 0 };
                    while z <= r {
                        let c = (mx + x, my + y - 1, mz + z);
                        if valid(world, c) {
                            self.target = c;
                            return true;
                        }
                        z = if z > 0 { -z } else { 1 - z };
                    }
                    x = if x > 0 { -x } else { 1 - x };
                }
            }
            y = if y > 0 { -y } else { 1 - y };
        }
        false
    }

    fn walk(&self, ctx: &mut MonsterGoalContext, world: &dyn World, speed: f64) {
        let profile = ctx.profile();
        let to = DVec3::new(f64::from(self.target.0) + 0.5, f64::from(self.target.1 + 1), f64::from(self.target.2) + 0.5);
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, speed, 1);
    }

    fn start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World, speed: f64) {
        self.walk(ctx, world, speed);
        self.try_ticks = 0;
        let inner = ctx.random.next_int(1200) as i32 + 1200;
        self.max_stay = ctx.random.next_int(inner as u32) as i32 + 1200;
    }

    /// `tick`: on until within a block of the top of the target.
    fn tick(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World, speed: f64, recalculate: i32) {
        let above = (self.target.0, self.target.1 + 1, self.target.2);
        if !near(above, ctx.body.position, 1.0) {
            self.reached = false;
            self.try_ticks += 1;
            if self.try_ticks % recalculate == 0 {
                self.walk(ctx, world, speed);
            }
        } else {
            self.reached = true;
            self.try_ticks -= 1;
        }
    }
}

/// `TurtleLayEggGoal`: to sand with air above near home, then the dig.
#[derive(Clone)]
struct TurtleLayEggGoal {
    search: MoveToBlock,
}

fn egg_spot(world: &dyn World, c: Cell) -> bool {
    is_air(world, (c.0, c.1 + 1, c.2)) && world.block_in_tag(c, "minecraft:sand")
}

impl Goal<MonsterGoalContext> for TurtleLayEggGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Jump])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let home = ctx.turtle.home;
        ctx.turtle.has_egg && home.is_some_and(|h| near(h, ctx.body.position, 9.0)) && self.search.can_use(ctx, world, &egg_spot)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let s = &self.search;
        s.try_ticks >= -s.max_stay && s.try_ticks <= 1200 && egg_spot(world, s.target) && ctx.turtle.has_egg && ctx.turtle.home.is_some_and(|h| near(h, ctx.body.position, 9.0))
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.search.start(ctx, world, 1.0);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.search.tick(ctx, world, 1.0, 40);
        if ctx.fluid.in_water() || !self.search.reached {
            return;
        }
        let t = &mut ctx.turtle;
        if t.lay_egg_counter < 1 {
            t.laying_egg = true;
        } else if t.lay_egg_counter > 200 {
            let target = self.search.target;
            let eggs = ctx.random.next_int(4) as u8 + 1;
            let t = &mut ctx.turtle;
            t.laid = Some(((target.0, target.1 + 1, target.2), eggs));
            t.has_egg = false;
            t.laying_egg = false;
            t.love = Some(600);
        }
        let t = &mut ctx.turtle;
        if t.laying_egg {
            t.lay_egg_counter += 1;
        }
    }
}

/// `TurtleGoToWaterGoal`: ashore, to water within 24 blocks (a baby at
/// twice the pace).
#[derive(Clone)]
struct TurtleGoToWaterGoal {
    search: MoveToBlock,
}

fn water_spot(world: &dyn World, c: Cell) -> bool {
    is_water(world, c)
}

impl TurtleGoToWaterGoal {
    fn speed(ctx: &MonsterGoalContext) -> f64 {
        if ctx.turtle.baby {
            2.0
        } else {
            1.0
        }
    }
}

impl Goal<MonsterGoalContext> for TurtleGoToWaterGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Jump])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let t = &ctx.turtle;
        let eligible = if t.baby && !ctx.fluid.in_water() { true } else { !t.going_home && !ctx.fluid.in_water() && !t.has_egg };
        eligible && self.search.can_use(ctx, world, &water_spot)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        !ctx.fluid.in_water() && self.search.try_ticks <= 1200 && water_spot(world, self.search.target)
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let speed = Self::speed(ctx);
        self.search.start(ctx, world, speed);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let speed = Self::speed(ctx);
        self.search.tick(ctx, world, speed, 160);
    }
}

/// `TurtleGoHomeGoal`.
#[derive(Clone)]
struct TurtleGoHomeGoal {
    stuck: bool,
    close_ticks: i32,
}

impl Goal<MonsterGoalContext> for TurtleGoHomeGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some(home) = ctx.turtle.home else { return false };
        if ctx.turtle.baby {
            false
        } else if ctx.turtle.has_egg {
            true
        } else {
            ctx.random.next_int(350) == 0 && !near(home, ctx.body.position, 64.0)
        }
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.turtle.home.is_some_and(|home| !near(home, ctx.body.position, 7.0)) && !self.stuck && self.close_ticks <= 300
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.turtle.going_home = true;
        self.stuck = false;
        self.close_ticks = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.turtle.going_home = false;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(home) = ctx.turtle.home else { return };
        let close = near(home, ctx.body.position, 16.0);
        if close {
            self.close_ticks += 1;
        }
        if !ctx.navigation.is_done() {
            return;
        }
        let towards = bottom_center(home);
        let profile = ctx.profile();
        let p = ctx.body.position;
        let mut next = crate::stroll::default_random_position_towards(world, &profile, p, 16, 3, towards, std::f32::consts::PI / 10.0, &mut ctx.random)
            .or_else(|| crate::stroll::default_random_position_towards(world, &profile, p, 8, 7, towards, std::f32::consts::PI / 2.0, &mut ctx.random));
        if let Some(n) = next {
            if !close && !is_water(world, (n.x.floor() as i32, n.y.floor() as i32, n.z.floor() as i32)) {
                next = crate::stroll::default_random_position_towards(world, &profile, p, 16, 5, towards, std::f32::consts::PI / 2.0, &mut ctx.random);
            }
        }
        let Some(next) = next else {
            self.stuck = true;
            return;
        };
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, next, 1.0, 0);
    }
}

/// `TurtleTravelGoal`: in water, off towards a far point.
#[derive(Clone)]
struct TurtleTravelGoal {
    stuck: bool,
}

impl Goal<MonsterGoalContext> for TurtleTravelGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.turtle.going_home && !ctx.turtle.has_egg && ctx.fluid.in_water()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.navigation.is_done() && !self.stuck && !ctx.turtle.going_home && !ctx.in_love && !ctx.turtle.has_egg
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        let xt = ctx.random.next_int(1025) as i32 - 512;
        let mut yt = ctx.random.next_int(9) as i32 - 4;
        let zt = ctx.random.next_int(1025) as i32 - 512;
        let p = ctx.body.position;
        if f64::from(yt) + p.y > f64::from(SEA_LEVEL - 1) {
            yt = 0;
        }
        ctx.turtle.travel = Some(((f64::from(xt) + p.x).floor() as i32, (f64::from(yt) + p.y).floor() as i32, (f64::from(zt) + p.z).floor() as i32));
        self.stuck = false;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.turtle.travel = None;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(travel) = ctx.turtle.travel else {
            self.stuck = true;
            return;
        };
        if !ctx.navigation.is_done() {
            return;
        }
        let towards = bottom_center(travel);
        let profile = ctx.profile();
        let p = ctx.body.position;
        let next = crate::stroll::default_random_position_towards(world, &profile, p, 16, 3, towards, std::f32::consts::PI / 10.0, &mut ctx.random)
            .or_else(|| crate::stroll::default_random_position_towards(world, &profile, p, 8, 7, towards, std::f32::consts::PI / 2.0, &mut ctx.random));
        let Some(next) = next else {
            self.stuck = true;
            return;
        };
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, next, 1.0, 0);
    }
}

/// `TurtleRandomStrollGoal(turtle, 1.0, 100)`: ashore, with nowhere to be.
#[derive(Clone)]
struct TurtleStrollGoal;

impl Goal<MonsterGoalContext> for TurtleStrollGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.fluid.in_water() || ctx.turtle.going_home || ctx.turtle.has_egg {
            return false;
        }
        if ctx.no_action_time >= 100 || ctx.random.next_int(50) != 0 {
            return false;
        }
        let profile = ctx.profile();
        let Some(spot) = crate::stroll::default_random_position(world, &profile, ctx.body.position, 10, 7, &mut ctx.random) else { return false };
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, spot, 1.0, 0);
        true
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.navigation.is_done()
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.navigation.stop();
    }
}

impl MobEntity {
    /// Hands its home and egg to its goals before they run.
    pub(super) fn turtle_prepare(&mut self) {
        if self.kind != Kind::Turtle {
            return;
        }
        if self.turtle.home.is_none() {
            let p = self.body.position;
            self.turtle.home = Some((p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
        }
        let baby = self.baby();
        let t = &mut self.ai.state.turtle;
        t.home = self.turtle.home;
        t.has_egg = self.turtle.has_egg;
        t.laying_egg = self.turtle.laying_egg;
        t.lay_egg_counter = self.turtle.lay_egg_counter;
        t.baby = baby;
    }

    /// Takes back what its goals changed, and `Turtle.aiStep`'s digging.
    pub(super) fn turtle_settle(&mut self, world: &impl World) {
        if self.kind != Kind::Turtle {
            return;
        }
        let t = &mut self.ai.state.turtle;
        self.turtle.has_egg = t.has_egg;
        self.turtle.laying_egg = t.laying_egg;
        self.turtle.lay_egg_counter = if t.laying_egg { t.lay_egg_counter } else { 0 };
        if let Some(love) = t.love.take() {
            self.in_love = love;
        }
        self.turtle.laid = t.laid.take();
        if self.health > 0.0 && self.turtle.laying_egg && self.turtle.lay_egg_counter >= 1 && self.turtle.lay_egg_counter % 5 == 0 {
            let p = self.body.position;
            let cell = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            if world.block_in_tag((cell.0, cell.1 - 1, cell.2), "minecraft:sand") {
                // Sand kicked up (level event 2001 for the block below).
                self.turtle.dig = Some(cell);
            }
        }
    }
}

impl EntityWorld {
    /// A turtle's eggs (the block and its sound), its digging and its
    /// scute (`ageBoundaryReached`'s `TURTLE_GROW` gift).
    pub(super) fn turtle_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let (laid, dig, grew, position) = (e.turtle.laid.take(), e.turtle.dig.take(), std::mem::take(&mut e.turtle.grew), e.body.position);
        if let Some(cell) = dig {
            // The data names no state: the server takes the block below.
            self.spawned.global_events.push((2001, cell, -1));
        }
        if grew {
            self.spawned.items.push((position, ItemStack::new("minecraft:turtle_scute", 1)));
        }
        let Some((cell, eggs)) = laid else { return };
        let Some(e) = self.mob_mut(id) else { return };
        let pitch = 0.9 + e.random.next_float() * 0.2;
        let at = bottom_center(cell);
        e.voices.push((Voice::Event("entity.turtle.lay_egg", 0.3, pitch), at));
        world.set_block(cell, Some(minecraftoss_player::Block::new("minecraft:turtle_egg").with("eggs", &eggs.to_string())));
    }
}
