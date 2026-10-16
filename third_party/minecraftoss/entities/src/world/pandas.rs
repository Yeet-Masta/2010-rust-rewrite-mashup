//! Pandas (26.3 `Panda` and its goals).
//!
//! - Two genes, main and hidden, make its nature: lazy (slow, lies on its
//!   back), worried (shies from players and monsters, cowers in thunder),
//!   playful (rolls about), aggressive (holds a grudge), weak (sneezes,
//!   ten health), brown, or normal; brown and weak show only when both
//!   genes carry them. A cub takes one gene from each parent, one time in
//!   thirty-two a fresh one.
//! - It picks bamboo or cake off the ground, sits to eat it for four
//!   seconds or so (bamboo is eaten up), and tires of sitting. A cub
//!   sneezes, startling grown pandas about into a hop and dropping a
//!   slime ball. Cubs and playful pandas roll over, and do when the ground
//!   ahead drops.
//! - It breeds only with bamboo within eight blocks; otherwise it shakes
//!   its head. It strikes back when hurt (but for an aggressive one, only
//!   once), and aggressive pandas about join in.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{HurtByTargetGoal, MeleeAttackGoal, MonsterGoalContext};
use crate::species::Kind;

/// `Panda.Gene`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Gene {
    #[default]
    Normal,
    Lazy,
    Worried,
    Playful,
    Brown,
    Weak,
    Aggressive,
}

impl Gene {
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Lazy => "lazy",
            Self::Worried => "worried",
            Self::Playful => "playful",
            Self::Brown => "brown",
            Self::Weak => "weak",
            Self::Aggressive => "aggressive",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim_start_matches("minecraft:") {
            "normal" => Self::Normal,
            "lazy" => Self::Lazy,
            "worried" => Self::Worried,
            "playful" => Self::Playful,
            "brown" => Self::Brown,
            "weak" => Self::Weak,
            "aggressive" => Self::Aggressive,
            _ => return None,
        })
    }

    fn recessive(self) -> bool {
        matches!(self, Self::Brown | Self::Weak)
    }

    /// `getRandom`: one in sixteen each lazy, worried, playful and
    /// aggressive; four weak, two brown, the rest normal.
    fn random(random: &mut LegacyRandom) -> Self {
        match random.next_int(16) {
            0 => Self::Lazy,
            1 => Self::Worried,
            2 => Self::Playful,
            4 => Self::Aggressive,
            n if n < 9 => Self::Weak,
            n if n < 11 => Self::Brown,
            _ => Self::Normal,
        }
    }

    /// `getVariantFromGenes`.
    pub fn variant(main: Self, hidden: Self) -> Self {
        if main.recessive() {
            if main == hidden { main } else { Self::Normal }
        } else {
            main
        }
    }
}

/// A panda's genes, poses and appetite.
#[derive(Clone, Debug, Default)]
pub struct PandaState {
    pub ready: bool,
    pub main: Gene,
    pub hidden: Gene,
    pub unhappy: i32,
    pub sneezing: bool,
    pub sneeze_counter: i32,
    pub eat_counter: i32,
    pub sitting: bool,
    pub on_back: bool,
    pub rolling: bool,
    pub roll_counter: i32,
    pub roll_delta: DVec3,
    pub sit_amount: f32,
    pub sit_amount_o: f32,
    pub on_back_amount: f32,
    pub on_back_amount_o: f32,
    pub roll_amount: f32,
    pub roll_amount_o: f32,
    /// How many of what it holds (`e.hand`).
    pub hand_count: i32,
    pub got_bamboo: bool,
    pub did_bite: bool,
    pub hurt_seen: Option<i32>,
    /// A sneeze done this tick, and what to drop where its sit goal ended.
    pub sneezed: bool,
    pub dropped: Option<(String, i32)>,
    /// Its hidden gene was read from a save (else drawn on first tick).
    pub hidden_saved: bool,
    /// Thunder, as it last ticked.
    pub thundering: bool,
}

impl PandaState {
    pub fn variant(&self) -> Gene {
        Gene::variant(self.main, self.hidden)
    }

    pub fn eating(&self) -> bool {
        self.eat_counter > 0
    }

    pub fn amounts(&self, partial: f32) -> (f32, f32, f32) {
        let lerp = |o: f32, n: f32| o + (n - o) * partial;
        (lerp(self.sit_amount_o, self.sit_amount), lerp(self.on_back_amount_o, self.on_back_amount), lerp(self.roll_amount_o, self.roll_amount))
    }
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct PandaCtx {
    pub gene: Gene,
    pub baby: bool,
    /// `canPerformAction`: not on its back, cowering, eating, rolling or sitting.
    pub can_act: bool,
    pub sitting: bool,
    pub holding: bool,
    pub unhappy: i32,
    /// The bamboo and cake on the ground within eight blocks it could take.
    pub items: Vec<DVec3>,
    pub tick_count: i32,
    pub got_bamboo: bool,
    pub did_bite: bool,
    /// Out.
    pub sit: Option<bool>,
    pub on_back: Option<bool>,
    pub roll: bool,
    pub sneeze: bool,
    pub set_unhappy: bool,
    pub drop_held: bool,
}

/// `Panda.registerGoals`.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    use crate::monster_ai::AvoidEntityGoal;
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(2, PandaPanicGoal(crate::mob_ai::PanicGoal::new(2.0)));
    selector.add(2, PandaBreedGoal { inner: crate::mob_ai::BreedGoal::new(), unhappy_cooldown: 0 });
    selector.add(3, Acting(MeleeAttackGoal { hits: true, raises_arms: false, follow_unseen: true, gives_up_in_light: false }));
    selector.add(4, crate::mob_ai::TemptGoal::new(1.0));
    selector.add(6, Worried(AvoidEntityGoal::new("minecraft:player", 8.0, 2.0, 2.0)));
    selector.add(6, Worried(AvoidEntityGoal::new("#monster", 4.0, 2.0, 2.0)));
    selector.add(7, PandaSitGoal { cooldown: 0 });
    selector.add(8, LieOnBackGoal { cooldown: 0 });
    selector.add(8, SneezeGoal);
    selector.add(9, Acting(crate::monster_ai::LookAtPlayerGoal { range: 6.0 }));
    selector.add(10, crate::monster_ai::RandomLookGoal);
    selector.add(12, RollGoal);
    selector.add(13, crate::mob_ai::FollowParentGoal::new(1.25));
    selector.add(14, crate::monster_ai::StrollGoal::new(1.0, 0.001));
    selector
}

/// `PandaHurtByTargetGoal`: alerting the aggressive pandas about; a bite
/// given or bamboo offered ends the grudge.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, PandaHurtByGoal(HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: true }));
    selector
}

macro_rules! wrapped {
    ($name:ident, $start:expr) => {
        impl<G: Goal<MonsterGoalContext> + Clone + 'static> Goal<MonsterGoalContext> for $name<G> {
            fn controls(&self) -> Controls {
                self.0.controls()
            }
            fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
                let test: fn(&MonsterGoalContext) -> bool = $start;
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

/// A goal that needs `canPerformAction` to start.
#[derive(Clone)]
struct Acting<G>(G);
wrapped!(Acting, |ctx| ctx.panda.can_act);

/// `PandaAvoidGoal`: for worried pandas.
#[derive(Clone)]
struct Worried<G>(G);
wrapped!(Worried, |ctx| ctx.panda.gene == Gene::Worried && ctx.panda.can_act);

/// `PandaPanicGoal(2.0)`: for fire and the like, ended by sitting.
#[derive(Clone)]
struct PandaPanicGoal(crate::mob_ai::PanicGoal);

impl Goal<MonsterGoalContext> for PandaPanicGoal {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.on_fire && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.panda.sitting {
            ctx.navigation.stop();
            return false;
        }
        self.0.can_continue(ctx, world)
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.start_world(ctx, world);
    }
}

/// `PandaBreedGoal`: only with bamboo within eight blocks and three up;
/// otherwise, once in thirty seconds, it shakes its head.
#[derive(Clone)]
struct PandaBreedGoal {
    inner: crate::mob_ai::BreedGoal,
    unhappy_cooldown: i32,
}

/// `canFindBamboo`: rings out to eight blocks on each of three levels.
fn bamboo_near(world: &dyn World, p: DVec3) -> bool {
    let base = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    for y in 0..3 {
        for r in 0..8 {
            let mut x = 0;
            while x <= r {
                let mut z = if x < r && x > -r { r } else { 0 };
                while z <= r {
                    if world.block((base.0 + x, base.1 + y, base.2 + z)).is_some_and(|b| b.id == "minecraft:bamboo") {
                        return true;
                    }
                    z = if z > 0 { -z } else { 1 - z };
                }
                x = if x > 0 { -x } else { 1 - x };
            }
        }
    }
    false
}

impl Goal<MonsterGoalContext> for PandaBreedGoal {
    fn controls(&self) -> Controls {
        self.inner.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !self.inner.can_start(ctx, world) || ctx.panda.unhappy != 0 {
            return false;
        }
        if bamboo_near(world, ctx.body.position) {
            return true;
        }
        if self.unhappy_cooldown <= ctx.panda.tick_count {
            ctx.panda.set_unhappy = true;
            self.unhappy_cooldown = ctx.panda.tick_count + 600;
        }
        false
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.inner.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.inner.start(ctx);
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.inner.start_world(ctx, world);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.inner.stop(ctx);
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        self.inner.tick(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.inner.tick_world(ctx, world);
    }
}

/// `PandaHurtByTargetGoal`.
#[derive(Clone)]
struct PandaHurtByGoal(HurtByTargetGoal);

impl Goal<MonsterGoalContext> for PandaHurtByGoal {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.panda.got_bamboo || ctx.panda.did_bite {
            ctx.clear_target();
            return false;
        }
        self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
}

/// `PandaSitGoal`: with food in hand it sits to eat; with food about it
/// goes for it; it gets up (dropping what is left) now and then, a lazy one
/// seldom, and waits ten to sixty seconds (lazy) or up to two and a half
/// minutes before it sits again.
#[derive(Clone)]
struct PandaSitGoal {
    cooldown: i32,
}

impl Goal<MonsterGoalContext> for PandaSitGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let p = &ctx.panda;
        if self.cooldown > p.tick_count || p.baby || ctx.fluid.in_water() || !p.can_act || p.unhappy > 0 {
            return false;
        }
        let at = ctx.body.position;
        p.holding || p.items.iter().any(|i| (*i - at).abs().max_element() <= 6.0 + 0.65)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if ctx.fluid.in_water() {
            return false;
        }
        if ctx.panda.gene != Gene::Lazy && ctx.random.next_int(300) == 1 {
            return false;
        }
        ctx.random.next_int(1000) != 1
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if ctx.panda.holding {
            ctx.panda.sit = Some(true);
            ctx.navigation.stop();
        } else if let Some(&item) = ctx.panda.items.first() {
            let profile = ctx.profile();
            let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, item, 1.2, 0);
        }
        self.cooldown = 0;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if !ctx.panda.sitting && ctx.panda.holding && !ctx.fluid.in_water() {
            ctx.panda.sit = Some(true);
            ctx.navigation.stop();
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        if ctx.panda.holding {
            ctx.panda.drop_held = true;
            let wait = if ctx.panda.gene == Gene::Lazy { ctx.random.next_int(50) as i32 + 10 } else { ctx.random.next_int(150) as i32 + 10 };
            self.cooldown = ctx.panda.tick_count + wait * 20;
        }
        ctx.panda.sit = Some(false);
    }
}

/// `PandaLieOnBackGoal`: a lazy panda, one goal tick in two hundred, rolls
/// on its back for a while.
#[derive(Clone)]
struct LieOnBackGoal {
    cooldown: i32,
}

impl Goal<MonsterGoalContext> for LieOnBackGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        self.cooldown < ctx.panda.tick_count && ctx.panda.gene == Gene::Lazy && ctx.panda.can_act && ctx.random.next_int(200) == 1
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if ctx.fluid.in_water() {
            return false;
        }
        if ctx.panda.gene != Gene::Lazy && ctx.random.next_int(300) == 1 {
            return false;
        }
        ctx.random.next_int(1000) != 1
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.panda.on_back = Some(true);
        self.cooldown = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.panda.on_back = Some(false);
        self.cooldown = ctx.panda.tick_count + 200;
    }
}

/// `PandaSneezeGoal`: a cub, one goal tick in three thousand (a weak one's
/// two hundred and fifty).
#[derive(Clone)]
struct SneezeGoal;

impl Goal<MonsterGoalContext> for SneezeGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if !ctx.panda.baby || !ctx.panda.can_act {
            return false;
        }
        ctx.panda.gene == Gene::Weak && ctx.random.next_int(250) == 1 || ctx.random.next_int(3000) == 1
    }
    fn can_continue(&mut self, _ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        false
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.panda.sneeze = true;
    }
}

/// `PandaRollGoal`: a cub or playful panda on the ground rolls over where
/// the ground ahead falls away, or now and then (one in thirty goal ticks
/// for the playful, else one in two hundred and fifty).
#[derive(Clone)]
struct RollGoal;

impl Goal<MonsterGoalContext> for RollGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look, Control::Jump])
    }
    fn interruptible(&self) -> bool {
        false
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let playful = ctx.panda.gene == Gene::Playful;
        if !(ctx.panda.baby || playful) || !ctx.body.on_ground || !ctx.panda.can_act {
            return false;
        }
        let angle = ctx.yaw.to_radians();
        let (dx, dz) = (-angle.sin(), angle.cos());
        let step = |d: f32| if d.abs() > 0.5 { d.signum() as i32 } else { 0 };
        let p = ctx.body.position;
        let ahead = (p.x.floor() as i32 + step(dx), p.y.floor() as i32 - 1, p.z.floor() as i32 + step(dz));
        if world.block(ahead).is_none_or(|b| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")) {
            return true;
        }
        playful && ctx.random.next_int(30) == 1 || ctx.random.next_int(250) == 1
    }
    fn can_continue(&mut self, _ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        false
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.panda.roll = true;
    }
}

/// Bamboo and cake: what it takes off the ground (`#panda_eats_from_ground`).
pub fn eats_from_ground(item: &str) -> bool {
    matches!(item, "minecraft:bamboo" | "minecraft:cake")
}

impl MobEntity {
    /// `isScared`: worried in thunder.
    fn panda_scared(&self, thundering: bool) -> bool {
        self.panda.variant() == Gene::Worried && thundering
    }

    /// `canPerformAction`.
    pub fn panda_can_act(&self, thundering: bool) -> bool {
        let p = &self.panda;
        !p.on_back && !self.panda_scared(thundering) && !p.eating() && !p.rolling && !p.sitting
    }

    /// Its genes on first ticking (`finalizeSpawn`), what it saw and was
    /// told, and its move control held while it cannot act
    /// (`PandaMoveControl`).
    pub(super) fn panda_prepare(&mut self, thundering: bool, items: Vec<DVec3>) {
        if self.kind != Kind::Panda {
            return;
        }
        if !self.panda.ready {
            self.panda.ready = true;
            if let Some(main) = Gene::from_name(&self.variant) {
                self.panda.main = main;
            } else {
                self.panda.main = Gene::random(&mut self.random);
                self.variant = self.panda.main.name().to_owned();
            }
            if !self.panda.hidden_saved {
                self.panda.hidden = Gene::random(&mut self.random);
            }
            // `setAttributes`.
            if self.panda.variant() == Gene::Weak {
                self.health = self.health.min(10.0);
            }
        }
        self.panda.thundering = thundering;
        let hurt = self.ai.state.hurt_by.map(|(_, w)| w);
        if hurt.is_some() && hurt != self.panda.hurt_seen {
            // `hurtServer`: it gets up.
            self.panda.sitting = false;
        }
        self.panda.hurt_seen = hurt;
        let can_act = self.panda_can_act(thundering);
        if !can_act {
            self.ai.state.navigation.stop();
            self.ai.move_control.set_wait();
        }
        let p = &self.panda;
        self.ai.state.panda = PandaCtx {
            gene: p.variant(),
            baby: self.age.baby(),
            can_act,
            sitting: p.sitting,
            holding: self.hand.is_some(),
            unhappy: p.unhappy,
            items,
            tick_count: self.tick_count,
            got_bamboo: p.got_bamboo,
            did_bite: p.did_bite,
            ..PandaCtx::default()
        };
    }

    /// After its move: what its goals asked, then `Panda.tick`'s moods,
    /// sneeze, roll, poses and meal.
    pub(super) fn panda_settle(&mut self, thundering: bool) {
        if self.kind != Kind::Panda {
            return;
        }
        let ctx = std::mem::take(&mut self.ai.state.panda);
        let position = self.body.position;
        // `doHurtTarget`: a bite ends a gentle panda's grudge.
        if self.ai.state.attack.is_some() && self.panda.variant() != Gene::Aggressive {
            self.panda.did_bite = true;
        }
        if let Some(sit) = ctx.sit {
            self.panda.sitting = sit && !self.body.touching_water;
        }
        if let Some(back) = ctx.on_back {
            self.panda.on_back = back;
        }
        if ctx.roll {
            self.panda.rolling = true;
        }
        if ctx.sneeze {
            self.panda.sneezing = true;
        }
        if ctx.set_unhappy {
            self.panda.unhappy = 32;
        }
        if ctx.drop_held {
            if let Some(item) = self.hand.take() {
                self.panda.dropped = Some((item, self.panda.hand_count.max(1)));
                self.panda.hand_count = 0;
            }
        }
        // `Panda.tick`.
        if self.panda.variant() == Gene::Worried {
            if thundering && !self.body.touching_water {
                self.panda.sitting = true;
                self.panda.eat_counter = 0;
            } else if !self.panda.eating() {
                self.panda.sitting = false;
            }
        }
        if self.ai.state.target.is_none() {
            self.panda.got_bamboo = false;
            self.panda.did_bite = false;
        }
        if self.panda.unhappy > 0 {
            if matches!(self.panda.unhappy, 29 | 14) {
                self.voices.push((Voice::Event("entity.panda.cant_breed", 1.0, 1.0), position));
            }
            self.panda.unhappy -= 1;
        }
        if self.panda.sneezing {
            self.panda.sneeze_counter += 1;
            if self.panda.sneeze_counter > 20 {
                self.panda.sneezing = false;
                self.panda.sneeze_counter = 0;
                self.panda.sneezed = true;
            } else if self.panda.sneeze_counter == 1 {
                self.voices.push((Voice::Event("entity.panda.pre_sneeze", 1.0, 1.0), position));
            }
        }
        if self.panda.rolling {
            self.panda_roll();
        } else {
            self.panda.roll_counter = 0;
        }
        if self.panda.sitting {
            self.pitch = 0.0;
        }
        let ease = |on: bool, amount: &mut f32, old: &mut f32| {
            *old = *amount;
            *amount = if on { (*amount + 0.15).min(1.0) } else { (*amount - 0.19).max(0.0) };
        };
        let p = &mut self.panda;
        ease(p.sitting, &mut p.sit_amount, &mut p.sit_amount_o);
        self.panda_eat(thundering);
        let p = &mut self.panda;
        ease(p.on_back, &mut p.on_back_amount, &mut p.on_back_amount_o);
        ease(p.rolling, &mut p.roll_amount, &mut p.roll_amount_o);
    }

    /// `handleRoll`: a hop forward and three more as it goes over.
    fn panda_roll(&mut self) {
        let p = &mut self.panda;
        p.roll_counter += 1;
        if p.roll_counter > 32 {
            p.rolling = false;
            return;
        }
        let v = self.body.velocity;
        if p.roll_counter == 1 {
            let angle = self.yaw.to_radians();
            let multiplier = if self.age.baby() { 0.1 } else { 0.2 };
            p.roll_delta = DVec3::new(v.x + f64::from(-angle.sin() * multiplier), 0.0, v.z + f64::from(angle.cos() * multiplier));
            self.body.velocity = p.roll_delta + DVec3::new(0.0, 0.27, 0.0);
        } else if !matches!(p.roll_counter, 7 | 15 | 23) {
            self.body.velocity = DVec3::new(p.roll_delta.x, v.y, p.roll_delta.z);
        } else {
            self.body.velocity = DVec3::new(0.0, if self.body.on_ground { 0.27 } else { v.y }, 0.0);
        }
        self.body.needs_sync = true;
    }

    /// `handleEating`: sitting with food it starts eating one tick in
    /// eighty; munching every fifth tick, after four seconds it may stop
    /// (one in twenty a tick), and after five bamboo or cake is gone.
    fn panda_eat(&mut self, thundering: bool) {
        let scared = self.panda_scared(thundering);
        let holding = self.hand.is_some();
        if !self.panda.eating() && self.panda.sitting && !scared && holding && self.random.next_int(80) == 1 {
            self.panda.eat_counter = 1;
        } else if !holding || !self.panda.sitting {
            self.panda.eat_counter = 0;
        }
        if !self.panda.eating() {
            return;
        }
        if self.panda.eat_counter % 5 == 0 {
            let volume = 0.5 + 0.5 * self.random.next_int(2) as f32;
            let pitch = (self.random.next_float() - self.random.next_float()) * 0.2 + 1.0;
            self.voices.push((Voice::Event("entity.panda.eat", volume, pitch), self.body.position));
        }
        if self.panda.eat_counter > 80 && self.random.next_int(20) == 1 {
            if self.panda.eat_counter > 100 && self.hand.as_deref().is_some_and(eats_from_ground) {
                self.hand = None;
                self.panda.hand_count = 0;
                self.panda.sitting = false;
            }
            self.panda.eat_counter = 0;
            return;
        }
        self.panda.eat_counter += 1;
    }
}

impl EntityWorld {
    /// The bamboo and cake on the ground within eight blocks of `id`.
    pub(super) fn panda_items(&self, id: u64, world: &impl World) -> Vec<DVec3> {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return Vec::new() };
        let p = e.body.position;
        let half = DVec3::new(f64::from(e.body.width) / 2.0 + 8.0, 8.0, f64::from(e.body.width) / 2.0 + 8.0);
        world.items_in(p - half, p + half + DVec3::new(0.0, f64::from(e.body.height), 0.0)).into_iter().filter(|i| i.pickup_delay <= 0 && i.count > 0 && eats_from_ground(&i.item)).map(|i| i.position).collect()
    }

    /// After its tick: the sneeze (`afterSneeze`: the grown pandas about hop,
    /// and out comes a slime ball) and what its sit goal let fall.
    pub(super) fn panda_step(&mut self, id: u64, thundering: bool) {
        let Some(e) = self.mob_mut(id) else { return };
        let dropped = e.panda.dropped.take();
        let sneezed = std::mem::take(&mut e.panda.sneezed);
        let p = e.body.position;
        if sneezed {
            e.voices.push((Voice::Event("entity.panda.sneeze", 1.0, 1.0), p));
        }
        let (half, height) = (f64::from(e.body.width) / 2.0, f64::from(e.body.height));
        if let Some((item, count)) = dropped {
            self.spawned.items.push((p, ItemStack::new(&item, count.clamp(1, 64) as u8)));
        }
        if !sneezed {
            return;
        }
        for other in self.mobs.iter_mut().filter(|o| o.kind == Kind::Panda) {
            let q = other.body.position;
            let near = (q.x - p.x).abs() < half + 10.0 + f64::from(other.body.width) / 2.0 && (q.z - p.z).abs() < half + 10.0 + f64::from(other.body.width) / 2.0 && q.y < p.y + height + 10.0 && q.y + f64::from(other.body.height) > p.y - 10.0;
            if near && !other.age.baby() && other.body.on_ground && !other.body.touching_water && other.panda_can_act(thundering) {
                other.body.velocity.y = 0.42;
                other.body.needs_sync = true;
            }
        }
        if self.mob_drops {
            self.spawned.items.push((p, ItemStack::new("minecraft:slime_ball", 1)));
        }
    }

    /// `Panda.mobInteract` before the breeding: one on its back gets up;
    /// a grown one that cannot fall in love sits down to eat the bamboo
    /// given, dropping what it held. Whether the use was taken, and whether
    /// one bamboo is spent.
    pub fn panda_interact(&mut self, id: u64, held: Option<&str>) -> Option<bool> {
        let e = self.mobs.iter_mut().find(|e| e.id == id && e.kind == Kind::Panda && e.health > 0.0)?;
        if e.panda_scared(e.panda.thundering) {
            return Some(false);
        }
        if e.panda.on_back {
            e.panda.on_back = false;
            return Some(false);
        }
        if held != Some("minecraft:bamboo") {
            return None;
        }
        if e.ai.state.target.is_some() {
            e.panda.got_bamboo = true;
        }
        let may_love = e.age.ticks == 0 && e.in_love == 0;
        if e.age.baby() || may_love || e.panda.sitting || e.body.touching_water {
            return None;
        }
        // `tryToSit` and `eat(true)`.
        e.ai.state.navigation.stop();
        e.panda.sitting = true;
        e.panda.eat_counter = 1;
        let p = e.body.position;
        if let Some(old) = e.hand.replace("minecraft:bamboo".to_owned()) {
            let count = e.panda.hand_count.max(1);
            self.spawned.items.push((p, ItemStack::new(&old, count.clamp(1, 64) as u8)));
        }
        if let Some(e) = self.mob_mut(id) {
            e.panda.hand_count = 1;
        }
        Some(true)
    }
}
