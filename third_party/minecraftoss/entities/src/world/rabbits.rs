//! Rabbits (26.3 `Rabbit`, `RabbitMoveControl`, `RabbitJumpControl`,
//! `RabbitPanicGoal`, `RabbitAvoidEntityGoal`, `RaidGardenGoal`).
//!
//! - A rabbit only moves by hopping: on the ground it stands until its
//!   landing delay (half a second, or three ticks when fleeing) runs out
//!   and it has somewhere to go, then faces it and jumps (0.3 high, 0.2
//!   for a stroll, 0.5 up a step), carried forward at its last pace.
//! - It flees players, wolves and monsters, panics when hurt, and raids
//!   ripe carrots: one bite takes an age from the crop (the last bite eats
//!   it), and it wants no more for a couple of seconds.
//! - The killer bunny (type 99) never flees: it hunts players and wolves,
//!   leaping at a target within four blocks.
use super::*;
use super::emissions::play_movement;
use super::mobs::MobEntity;
use super::turtles::MoveToBlock;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;

type Cell = (i32, i32, i32);

/// The killer bunny's `RabbitType`.
pub const EVIL: &str = "99";

/// A rabbit's hop.
#[derive(Clone, Debug, Default)]
pub struct RabbitState {
    pub jump_ticks: i32,
    pub jump_duration: i32,
    pub was_on_ground: bool,
    pub jump_delay_ticks: i32,
    pub more_carrot_ticks: i32,
    /// `RabbitJumpControl.jump`: the move control asked for a jump.
    pub want_jump: bool,
    pub can_jump: bool,
    /// `RabbitMoveControl.nextJumpSpeed`.
    pub next_jump_speed: f64,
    /// `LivingEntity.jumping` as `setJumping` leaves it.
    pub jumping: bool,
    /// When its hop began (entity event 1), while it lasts.
    pub hop_start: Option<i32>,
    /// Its bite of a crop this tick: where, what is left, what was there.
    pub bite: Option<(Cell, Option<minecraftoss_player::Block>, minecraftoss_player::Block)>,
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct RabbitCtx {
    pub evil: bool,
    pub more_carrot_ticks: i32,
    /// A bite of a carrot crop: where, and what is left of it.
    pub eat: Option<(Cell, Option<minecraftoss_player::Block>, minecraftoss_player::Block)>,
}

/// `Rabbit.registerGoals`, with the killer bunny's melee (each wish gated
/// on its type, which the goals see).
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    use crate::monster_ai::AvoidEntityGoal;
    let mut selector = GoalSelector::default();
    selector.add(1, crate::monster_ai::FloatGoal);
    selector.add(1, crate::mob_ai::PanicGoal::new(2.2));
    selector.add(2, crate::mob_ai::BreedGoal::new());
    selector.add(3, crate::mob_ai::TemptGoal::new(1.0));
    selector.add(4, Tame(AvoidEntityGoal::new("minecraft:player", 8.0, 2.2, 2.2)));
    selector.add(4, Tame(AvoidEntityGoal::new("minecraft:wolf", 10.0, 2.2, 2.2)));
    selector.add(4, Tame(AvoidEntityGoal::new("#monster", 4.0, 2.2, 2.2)));
    selector.add(4, Evil(crate::monster_ai::MeleeAttackGoal { hits: true, raises_arms: false, follow_unseen: true, gives_up_in_light: false }));
    selector.add(5, RaidGardenGoal { search: MoveToBlock::new(16, 0), wants: false, can_raid: false });
    selector.add(6, crate::monster_ai::StrollGoal::new(0.6, 0.001));
    selector.add(11, crate::monster_ai::LookAtPlayerGoal { range: 10.0 });
    selector
}

/// The killer bunny's targets.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    use crate::monster_ai::{HurtByTargetGoal, NearestTargetGoal, Prey};
    let mut selector = GoalSelector::default();
    selector.add(1, Evil(HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: true }));
    selector.add(2, Evil(NearestTargetGoal::every(Prey::Player, true, 5)));
    selector.add(2, Evil(NearestTargetGoal::every(Prey::Kinds(&["minecraft:wolf"]), true, 5)));
    selector
}

/// A goal only a plain rabbit has (`RabbitAvoidEntityGoal`).
#[derive(Clone)]
struct Tame<G>(G);

/// A goal only the killer bunny has.
#[derive(Clone)]
struct Evil<G>(G);

macro_rules! gated {
    ($name:ident, $test:expr) => {
        impl<G: Goal<MonsterGoalContext> + Clone + 'static> Goal<MonsterGoalContext> for $name<G> {
            fn controls(&self) -> Controls {
                self.0.controls()
            }
            fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
                let test: fn(&MonsterGoalContext) -> bool = $test;
                test(ctx) && self.0.can_start(ctx, world)
            }
            fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
                self.0.can_continue(ctx, world)
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
    };
}
gated!(Tame, |ctx| !ctx.rabbit.evil);
gated!(Evil, |ctx| ctx.rabbit.evil);

/// A ripe carrot crop on farmland (`isValidTarget`).
fn ripe_carrots_on(world: &dyn World, c: Cell) -> bool {
    if !world.block_in_tag(c, "minecraft:supports_crops") {
        return false;
    }
    world.block((c.0, c.1 + 1, c.2)).is_some_and(|b| b.id == "minecraft:carrots" && b.property("age") == Some("7"))
}

/// `RaidGardenGoal`: `MoveToBlockGoal(rabbit, 0.7, 16)` to farmland under
/// ripe carrots, while it wants food and mob griefing is on; within a block
/// of the crop it takes its bite. Finding the crop spends its wish, so the
/// goal holds only for the tick it starts.
#[derive(Clone)]
struct RaidGardenGoal {
    search: MoveToBlock,
    wants: bool,
    can_raid: bool,
}

impl Goal<MonsterGoalContext> for RaidGardenGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Jump])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if self.search.next_start <= 0 {
            if !ctx.mob_griefing {
                return false;
            }
            self.can_raid = false;
            self.wants = ctx.rabbit.more_carrot_ticks <= 0;
        }
        let (wants, can_raid) = (self.wants, self.can_raid);
        let found = self.search.can_use(ctx, world, &|w, c| wants && !can_raid && ripe_carrots_on(w, c));
        if found {
            self.can_raid = true;
        }
        found
    }
    fn can_continue(&mut self, _ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        // `canRaid && super.canContinueToUse()`, whose `isValidTarget` asks
        // for `!canRaid`.
        false
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.search.start(ctx, world, 0.7);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.search.tick(ctx, world, 0.7, 40);
        let t = self.search.target;
        let x_max = ctx.max_head_x_rot();
        ctx.look_control.set_look_at_with_limits(DVec3::new(f64::from(t.0) + 0.5, f64::from(t.1 + 1), f64::from(t.2) + 0.5), 10.0, x_max);
        if self.search.reached {
            let crop = (t.0, t.1 + 1, t.2);
            if let Some(block) = world.block(crop).filter(|b| b.id == "minecraft:carrots" && self.can_raid) {
                let age: i32 = block.property("age").and_then(|a| a.parse().ok()).unwrap_or(0);
                // The last bite leaves air (`destroyBlock` then finds nothing).
                let left = (age > 0).then(|| block.clone().with("age", &(age - 1).to_string()));
                ctx.rabbit.eat = Some((crop, left, block));
                ctx.rabbit.more_carrot_ticks = 40;
            }
            self.can_raid = false;
            self.search.next_start = 10;
        }
    }
}

impl MobEntity {
    fn rabbit_evil(&self) -> bool {
        self.variant == EVIL
    }

    /// `setJumping`, with the hop's sound on a jump.
    fn rabbit_set_jumping(&mut self, jump: bool) {
        self.rabbit.jumping = jump;
        if jump {
            let pitch = ((self.random.next_float() - self.random.next_float()) * 0.2 + 1.0) * 0.8;
            self.voices.push((Voice::Event("entity.rabbit.jump", 1.0, pitch), self.body.position));
        }
    }

    /// `startJumping`.
    fn rabbit_start_jumping(&mut self) {
        self.rabbit_set_jumping(true);
        self.rabbit.jump_duration = 15;
        self.rabbit.jump_ticks = 0;
    }

    /// `facePoint`.
    fn rabbit_face(&mut self, x: f64, z: f64) {
        self.ai.yaw = ((z - self.body.position.z).atan2(x - self.body.position.x).to_degrees() as f32) - 90.0;
    }

    /// `Rabbit.setSpeedModifier` through `RabbitMoveControl.setWantedPosition`
    /// (in water always 1.5).
    fn rabbit_speed_modifier(&mut self, speed: f64, in_water: bool) {
        let speed = if in_water { 1.5 } else { speed };
        self.ai.state.navigation.speed_modifier = speed;
        let wanted = self.ai.move_control.wanted;
        self.ai.move_control.set_wanted_position(wanted, speed);
        if speed > 0.0 {
            self.rabbit.next_jump_speed = speed;
        }
    }

    /// A rabbit's tick on the ground: `Mob.serverAiStep` with its own
    /// `customServerAiStep`, move and jump controls, then `aiStep`'s jump
    /// and travel and its hop's clock. Returns the fall damage a landing
    /// deals.
    pub(super) fn tick_rabbit(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[crate::monster_ai::MobCandidate], game_time: i64, difficulty: i32) -> Option<f32> {
        let s = self.species();
        let movement_speed = self.effects.movement_speed(s.speed);
        let sounds = self.movement_sounds();
        let evil = self.rabbit_evil();
        self.ai.state.on_fire = self.body.fire_ticks > 0;
        self.ai.state.tick_count = self.tick_count;
        let eye_height = self.eye_height();
        self.ai.state.eye_height = eye_height;
        self.ai.state.rabbit = RabbitCtx { evil, more_carrot_ticks: self.rabbit.more_carrot_ticks, eat: None };
        let position = self.body.position;
        if players.iter().any(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 32.0 * 32.0) {
            self.no_action_time = 0;
        }
        let eye = (position.x.floor() as i32, (position.y + f64::from(eye_height)).floor() as i32, position.z.floor() as i32);
        if world.light_path_cost(eye) > 0.0 {
            self.no_action_time += 2;
        }
        if self.ai.no_jump_delay > 0 {
            self.ai.no_jump_delay -= 1;
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
        ai.state.random = std::mem::take(&mut self.random);
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut self.ai.state.random);
        // `RabbitPanicGoal.tick` keeps its pace.
        if crate::mob_ai::panicking(&self.ai.state) && !evil {
            self.rabbit_speed_modifier(2.2, fluid.in_water());
        }
        let (can_update, surface) = crate::navigation::ground_view(world, &self.body, fluid, self.ai.state.walk.can_float);
        if let Some((wanted, speed)) = self.ai.state.navigation.tick_in(world, position, can_update, surface, self.body.width, self.ai.speed) {
            let speed = if fluid.in_water() { 1.5 } else { speed };
            self.ai.move_control.set_wanted_position(wanted, speed);
            if speed > 0.0 {
                self.rabbit.next_jump_speed = speed;
            }
        }
        // `customServerAiStep`.
        let r = &mut self.rabbit;
        if r.jump_delay_ticks > 0 {
            r.jump_delay_ticks -= 1;
        }
        if r.more_carrot_ticks > 0 {
            r.more_carrot_ticks = (r.more_carrot_ticks - self.random.next_int(3) as i32).max(0);
        }
        if self.body.on_ground {
            if !self.rabbit.was_on_ground {
                self.rabbit_set_jumping(false);
                // `checkLandingDelay`.
                self.rabbit.jump_delay_ticks = if self.ai.move_control.speed_modifier < 2.2 { 10 } else { 3 };
                self.rabbit.can_jump = false;
            }
            if evil && self.rabbit.jump_delay_ticks == 0 {
                if let Some(target) = self.ai.state.target().filter(|t| t.position.distance_squared(self.body.position) < 16.0) {
                    self.rabbit_face(target.position.x, target.position.z);
                    let speed = self.ai.move_control.speed_modifier;
                    self.ai.move_control.set_wanted_position(target.position, speed);
                    self.rabbit_start_jumping();
                    self.rabbit.was_on_ground = true;
                }
            }
            if !self.rabbit.want_jump {
                if self.ai.move_control.has_wanted() && self.rabbit.jump_delay_ticks == 0 {
                    let wanted = self.ai.move_control.wanted;
                    self.rabbit_face(wanted.x, wanted.z);
                    self.rabbit_start_jumping();
                }
            } else if !self.rabbit.can_jump {
                self.rabbit.can_jump = true;
            }
        }
        self.rabbit.was_on_ground = self.body.on_ground;
        // `RabbitMoveControl.tick`.
        if self.body.on_ground && !self.rabbit.jumping && !self.rabbit.want_jump {
            self.rabbit_speed_modifier(0.0, fluid.in_water());
        } else if self.ai.move_control.has_wanted() || self.ai.move_control.operation == crate::control::MoveOperation::Jumping {
            let next = self.rabbit.next_jump_speed;
            self.rabbit_speed_modifier(next, fluid.in_water());
        }
        let obstacle_top = crate::control::obstacle_top(world, position);
        let ai = &mut self.ai;
        ai.sideways *= 0.98;
        ai.forward *= 0.98;
        let control = ai.move_control.tick(position, self.body.on_ground, ai.yaw, ai.speed, ai.forward, ai.sideways, movement_speed, self.body.width, self.body.step_height, obstacle_top, |_, _| true);
        ai.yaw = control.yaw;
        ai.speed = control.speed;
        ai.forward = control.forward;
        ai.sideways = control.sideways;
        if control.jump {
            self.rabbit.want_jump = true;
        }
        let ai = &mut self.ai;
        ai.state.look_control.tick(position, eye_height, ai.body_rotation.body_yaw, !ai.state.navigation.is_done());
        // `RabbitJumpControl.tick`.
        if self.rabbit.want_jump {
            self.rabbit_start_jumping();
            self.rabbit.want_jump = false;
        }
        // `LivingEntity.aiStep`'s jump: the rabbit's own off the ground.
        let jumping = self.rabbit.jumping || self.ai.state.jump;
        self.ai.jumping = jumping;
        if jumping && self.body.on_ground && !fluid.in_water() && !fluid.in_lava() && self.ai.no_jump_delay == 0 {
            self.rabbit_jump_from_ground(world);
            self.ai.no_jump_delay = 10;
        } else {
            self.body.living_jump(world, fluid, jumping, &mut self.ai.no_jump_delay, 0.4);
        }
        let ai = &mut self.ai;
        let input = DVec3::new(f64::from(ai.sideways), 0.0, f64::from(ai.forward));
        let landed = if fluid.in_water() {
            self.body.travel_water(world, input, ai.yaw);
            None
        } else if fluid.in_lava() {
            self.body.travel_lava(world, input, ai.yaw, fluid.lava_height);
            None
        } else {
            self.body.travel_air_jumping(world, input, ai.speed, ai.yaw, jumping)
        };
        play_movement(&mut self.body, self.tick_count, &mut self.random, &mut self.voices, sounds);
        self.ai.body_rotation.tick(self.ai.yaw, &mut self.ai.state.look_control, self.previous_position, self.body.position);
        // `Rabbit.aiStep` after the rest: the hop's clock.
        let r = &mut self.rabbit;
        if r.jump_ticks != r.jump_duration {
            r.jump_ticks += 1;
        } else if r.jump_duration != 0 {
            r.jump_ticks = 0;
            r.jump_duration = 0;
            r.hop_start = None;
            self.rabbit_set_jumping(false);
        }
        self.yaw = self.ai.yaw;
        self.pitch = 0.0;
        // Its bite of the crop.
        let ctx = std::mem::take(&mut self.ai.state.rabbit);
        if ctx.more_carrot_ticks == 40 {
            self.rabbit.more_carrot_ticks = 40;
        }
        self.rabbit.bite = ctx.eat;
        landed.and_then(|fallen| super::living::fall_damage(&self.body, world, fallen, evil, &mut self.voices))
    }

    /// `Rabbit.jumpFromGround` with `getJumpPower`: 0.3 high (0.2 at a
    /// stroll's pace, 0.5 when its way climbs or it is blocked), and a
    /// shove forward when it would hop in place; event 1 starts its hop.
    fn rabbit_jump_from_ground(&mut self, world: &impl World) {
        let mc = &self.ai.move_control;
        let y = self.body.position.y;
        let mut power: f32 = if mc.speed_modifier <= 0.6 { 0.2 } else { 0.3 };
        if mc.wanted.y > y + 0.5 && !self.ai.state.navigation.is_done() {
            power = 0.5;
        }
        if self.body.horizontal_collision || self.rabbit.jumping && mc.wanted.y > y + 0.5 {
            power = 0.5;
        }
        let power = power * self.body.block_jump_factor(world);
        if power > 1.0e-5 {
            self.body.velocity.y = f64::from(power).max(self.body.velocity.y);
            self.body.needs_sync = true;
        }
        if mc.speed_modifier > 0.0 && self.body.velocity.x * self.body.velocity.x + self.body.velocity.z * self.body.velocity.z < 0.01 {
            // `moveRelative(0.1, (0, 1.5 or 0.5, 1))`, the input made unit.
            let input = DVec3::new(0.0, if self.baby() { 0.5 } else { 1.5 }, 1.0);
            let input = input.normalize() * f64::from(0.1_f32);
            let yaw = self.ai.yaw.to_radians();
            let (sin, cos) = (f64::from(yaw.sin()), f64::from(yaw.cos()));
            self.body.velocity += DVec3::new(input.x * cos - input.z * sin, input.y, input.z * cos + input.x * sin);
        }
        self.rabbit.hop_start = Some(self.tick_count);
    }
}

impl EntityWorld {
    /// A rabbit's bite of the carrots: the crop an age younger with its
    /// break effect, or gone.
    pub(super) fn rabbit_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let Some((cell, left, was)) = e.rabbit.bite.take() else { return };
        match left {
            Some(block) => {
                world.set_block(cell, Some(block));
                world.break_effect(cell, &was);
            }
            None => world.set_block(cell, None),
        }
    }
}
