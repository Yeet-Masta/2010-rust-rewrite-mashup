//! Llamas and trader llamas (26.3 `Llama`, `LlamaHurtByTargetGoal`,
//! `LlamaAttackWolfGoal`, `TraderLlama`).
//!
//! - A llama strikes back at what hurt it with one spit and then lets it
//!   be; it spits at wild wolves that come within a quarter of its follow
//!   range.
//! - A trader llama panics at twice its pace and, unless tamed, goes after
//!   its despawn delay (forty minutes) runs out.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{HurtByTargetGoal, MonsterGoalContext, Target};
use crate::species::Kind;

/// `TraderLlama.despawnDelay`'s start.
pub const TRADER_LLAMA_DESPAWN_DELAY: i32 = 47999;

/// A llama's spit, caravan and a trader llama's clock.
#[derive(Clone, Debug)]
pub struct LlamaState {
    /// `didSpit`: it spat since its hurt-by target last checked.
    pub did_spit: bool,
    pub despawn_delay: i32,
    /// `caravanHead`: the llama it follows.
    pub caravan_head: Option<u64>,
}

impl Default for LlamaState {
    fn default() -> Self {
        Self { did_spit: false, despawn_delay: TRADER_LLAMA_DESPAWN_DELAY, caravan_head: None }
    }
}

/// A llama near another, as `LlamaFollowCaravanGoal` sees it.
#[derive(Clone, Debug)]
pub struct CaravanLlama {
    pub id: u64,
    pub position: DVec3,
    pub in_caravan: bool,
    pub has_tail: bool,
    pub leashed: bool,
    /// `firstIsLeashed(it, 1)`.
    pub chain_leashed: bool,
}

/// What a llama's goals share.
#[derive(Clone, Debug, Default)]
pub struct LlamaCtx {
    pub did_spit: bool,
    pub leashed: bool,
    /// Its lead is on a fence knot.
    pub knotted: bool,
    /// The llama it follows: where, and whether alive.
    pub head: Option<(u64, DVec3, bool)>,
    /// `firstIsLeashed(self, 0)`.
    pub chain_leashed: bool,
    /// The llamas within its box grown 9 across and 4 up and down.
    pub near: Vec<CaravanLlama>,
    /// The trader it is leashed to: what last hurt it, and when.
    pub trader_hurt_by: Option<(Target, i32)>,
    pub join: Option<u64>,
    pub leave: bool,
}

/// `Llama.registerGoals` (a trader llama's adds its 2.0 panic first). The
/// rider's buck (`RunAroundLikeCrazyGoal`) and the caravan wait for riding
/// and leads.
pub fn goals(trader: bool) -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    if trader {
        selector.add(1, crate::mob_ai::PanicGoal::new(2.0));
    }
    selector.add(2, FollowCaravanGoal { speed: 2.1, dist_check: 0 });
    // `RangedAttackGoal(llama, 1.25, 40, 20)`.
    selector.add(3, crate::mob_ai::ShootGoal::new(20.0, 40, 1, 1.25));
    selector.add(3, crate::mob_ai::PanicGoal::new(1.2));
    selector.add(4, crate::mob_ai::BreedGoal::new());
    selector.add(5, crate::mob_ai::TemptGoal::new(1.25));
    selector.add(6, crate::mob_ai::FollowParentGoal::new(1.0));
    selector.add(7, crate::monster_ai::StrollGoal::new(0.7, 0.001));
    selector.add(8, crate::monster_ai::LookAtPlayerGoal { range: 6.0 });
    selector.add(9, crate::monster_ai::RandomLookGoal);
    selector
}

/// Its target goals (a trader llama's defence of its trader first).
pub fn targets(trader: bool) -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    if trader {
        selector.add(1, DefendTraderGoal { timestamp: 0, unseen_ticks: 0 });
    }
    selector.add(1, LlamaHurtByTargetGoal(HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: false }));
    selector.add(2, LlamaAttackWolfGoal { candidate: None });
    selector
}

/// `LlamaHurtByTargetGoal`: `HurtByTargetGoal`, ended by a spit.
#[derive(Clone)]
struct LlamaHurtByTargetGoal(HurtByTargetGoal);

impl Goal<MonsterGoalContext> for LlamaHurtByTargetGoal {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.llama.did_spit {
            ctx.llama.did_spit = false;
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

/// `LlamaAttackWolfGoal`: `NearestAttackableTargetGoal<Wolf>` one roll in
/// eight, for a wild wolf within a quarter of its follow range.
#[derive(Clone)]
struct LlamaAttackWolfGoal {
    candidate: Option<Target>,
}

impl Goal<MonsterGoalContext> for LlamaAttackWolfGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Target])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if ctx.random.next_int(8) != 0 {
            return false;
        }
        let range = (ctx.follow_range * 0.25).max(2.0);
        let eye = ctx.eye();
        let mut best: Option<(f64, Target)> = None;
        let wolves: Vec<Target> = ctx.mobs.iter().filter(|m| m.kind == "minecraft:wolf" && !m.tame).map(|m| Target::Mob(m.id)).collect();
        for target in wolves {
            let Some(info) = ctx.info(target) else { continue };
            if !ctx.can_attack(info) || ctx.body.position.distance_squared(info.position) > range * range {
                continue;
            }
            let distance = info.position.distance_squared(eye);
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, target));
            }
        }
        self.candidate = best.map(|(_, t)| t);
        self.candidate.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some(info) = ctx.target() else { return false };
        let range = ctx.follow_range * 0.25;
        ctx.can_attack(info) && ctx.body.position.distance_squared(info.position) <= range * range
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.set_target(self.candidate);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.clear_target();
    }
}

/// `TraderLlamaDefendWanderingTraderGoal`: what last hurt the trader it is
/// leashed to becomes its target, once per hurt.
#[derive(Clone)]
struct DefendTraderGoal {
    timestamp: i32,
    unseen_ticks: i32,
}

impl Goal<MonsterGoalContext> for DefendTraderGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Target])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some((attacker, when)) = ctx.llama.trader_hurt_by else { return false };
        when != self.timestamp && ctx.info(attacker).is_some_and(|i| ctx.can_attack(i))
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        crate::monster_ai::continue_target(ctx, world, None, &mut self.unseen_ticks, None)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        if let Some((attacker, when)) = ctx.llama.trader_hurt_by {
            ctx.set_target(Some(attacker));
            self.timestamp = when;
        }
        self.unseen_ticks = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.clear_target();
    }
}

/// `LlamaFollowCaravanGoal(llama, 2.1)`: an unleashed llama outside a
/// caravan joins the nearest caravan's tail (or a leashed llama with no
/// tail) four blocks or more away whose chain leads to a lead, and walks
/// to two blocks behind it, hurrying (up to three times, each 1.2 faster)
/// when more than 26 blocks behind and giving up if still that far two
/// seconds after.
#[derive(Clone)]
struct FollowCaravanGoal {
    speed: f64,
    dist_check: i32,
}

impl Goal<MonsterGoalContext> for FollowCaravanGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let l = &ctx.llama;
        if l.leashed || l.head.is_some() {
            return false;
        }
        let p = ctx.body.position;
        let nearest = |pick: &dyn Fn(&CaravanLlama) -> bool| {
            let mut best: Option<(f64, &CaravanLlama)> = None;
            for c in l.near.iter().filter(|c| pick(c)) {
                let d = c.position.distance_squared(p);
                if best.is_none_or(|(b, _)| d <= b) {
                    best = Some((d, c));
                }
            }
            best
        };
        let Some((distance, closest)) = nearest(&|c| c.in_caravan && !c.has_tail).or_else(|| nearest(&|c| c.leashed && !c.has_tail)) else { return false };
        if distance < 4.0 || !closest.leashed && !closest.chain_leashed {
            return false;
        }
        let (id, at) = (closest.id, closest.position);
        ctx.llama.join = Some(id);
        ctx.llama.head = Some((id, at, true));
        true
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some((_, at, alive)) = ctx.llama.head else { return false };
        if !alive || !ctx.llama.chain_leashed {
            return false;
        }
        if at.distance_squared(ctx.body.position) > 676.0 {
            if self.speed <= 3.0 {
                self.speed *= 1.2;
                self.dist_check = 20;
                return true;
            }
            if self.dist_check == 0 {
                return false;
            }
        }
        if self.dist_check > 0 {
            self.dist_check -= 1;
        }
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.llama.leave = true;
        self.speed = 2.1;
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some((_, at, _)) = ctx.llama.head else { return };
        if ctx.llama.knotted {
            return;
        }
        let p = ctx.body.position;
        let towards = (at - p).normalize_or_zero() * (p.distance(at) - 2.0).max(0.0);
        let profile = ctx.profile();
        let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, p + towards, self.speed, 1);
    }
}

impl MobEntity {
    /// Before its goals: whether it spat, and its caravan and leads.
    pub(super) fn llama_prepare(&mut self, mut ctx: LlamaCtx) {
        if matches!(self.kind, Kind::Llama | Kind::TraderLlama) {
            ctx.did_spit = self.llama.did_spit;
            self.ai.state.llama = ctx;
        }
    }

    /// After its goals: the spit its hurt-by goal took note of, and the
    /// caravan joined or left.
    pub(super) fn llama_settle(&mut self) {
        if matches!(self.kind, Kind::Llama | Kind::TraderLlama) {
            let ctx = std::mem::take(&mut self.ai.state.llama);
            self.llama.did_spit = ctx.did_spit;
            if let Some(head) = ctx.join {
                self.llama.caravan_head = Some(head);
            }
            if ctx.leave {
                self.llama.caravan_head = None;
            }
        }
    }
}

impl EntityWorld {
    /// `firstIsLeashed`: whether the caravan ahead of `id` (eight links at
    /// most, counting from `counter`) ends in a leashed llama.
    fn first_is_leashed(&self, id: u64, counter: i32) -> bool {
        if counter > 8 {
            return false;
        }
        let Some(head) = self.mobs.iter().find(|e| e.id == id).and_then(|e| e.llama.caravan_head) else { return false };
        self.leash_holder(head).is_some() || self.first_is_leashed(head, counter + 1)
    }

    /// What a llama's goals need of the world.
    pub(super) fn llama_context(&self, id: u64) -> LlamaCtx {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return LlamaCtx::default() };
        let holder = self.leash_holder(id);
        let head = e.llama.caravan_head.map(|h| match self.mobs.iter().find(|o| o.id == h) {
            Some(o) => (h, o.body.position, o.health > 0.0),
            None => (h, DVec3::ZERO, false),
        });
        let (p, half, height) = (e.body.position, f64::from(e.body.width) / 2.0, f64::from(e.body.height));
        let near = self
            .mobs
            .iter()
            .filter(|o| o.id != id && matches!(o.kind, Kind::Llama | Kind::TraderLlama))
            .filter(|o| {
                let (q, oh) = (o.body.position, f64::from(o.body.width) / 2.0);
                (q.x - p.x).abs() < half + 9.0 + oh && (q.z - p.z).abs() < half + 9.0 + oh && q.y < p.y + height + 4.0 && q.y + f64::from(o.body.height) > p.y - 4.0
            })
            .map(|o| CaravanLlama {
                id: o.id,
                position: o.body.position,
                in_caravan: o.llama.caravan_head.is_some(),
                has_tail: self.mobs.iter().any(|t| t.llama.caravan_head == Some(o.id)),
                leashed: self.leash_holder(o.id).is_some(),
                chain_leashed: self.first_is_leashed(o.id, 1),
            })
            .collect();
        let trader_hurt_by = match holder {
            Some(super::LeashHolder::Mob(t)) if e.kind == Kind::TraderLlama => self.mobs.iter().find(|o| o.id == t && o.kind == Kind::WanderingTrader).and_then(|o| o.ai.state.hurt_by),
            _ => None,
        };
        LlamaCtx {
            leashed: holder.is_some(),
            knotted: matches!(holder, Some(super::LeashHolder::Knot(_))),
            head,
            chain_leashed: self.first_is_leashed(id, 0),
            near,
            trader_hurt_by,
            ..LlamaCtx::default()
        }
    }

    /// `TraderLlama.maybeDespawn`: unless tamed or held by a player, its
    /// clock runs down (on its trader's, while leashed to one), and at
    /// nought it goes.
    pub(super) fn trader_llama_step(&mut self, id: u64) {
        let holder = self.leash_holder(id);
        if matches!(holder, Some(super::LeashHolder::Player(_))) {
            return;
        }
        let trader_delay = match holder {
            Some(super::LeashHolder::Mob(t)) => self.mobs.iter().find(|o| o.id == t && o.kind == Kind::WanderingTrader).map(|o| o.trader.despawn_delay),
            _ => None,
        };
        let Some(e) = self.mob_mut(id) else { return };
        if e.owner.is_some() {
            return;
        }
        e.llama.despawn_delay = match trader_delay {
            Some(delay) => delay - 1,
            None => e.llama.despawn_delay - 1,
        };
        if e.llama.despawn_delay <= 0 {
            let _ = self.discard_mob(id);
        }
    }
}
