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

/// A llama's spit and a trader llama's clock.
#[derive(Clone, Debug)]
pub struct LlamaState {
    /// `didSpit`: it spat since its hurt-by target last checked.
    pub did_spit: bool,
    pub despawn_delay: i32,
}

impl Default for LlamaState {
    fn default() -> Self {
        Self { did_spit: false, despawn_delay: TRADER_LLAMA_DESPAWN_DELAY }
    }
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

/// Its target goals.
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
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
        if ctx.llama_did_spit {
            ctx.llama_did_spit = false;
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

impl MobEntity {
    /// Before its goals: whether it spat.
    pub(super) fn llama_prepare(&mut self) {
        if matches!(self.kind, Kind::Llama | Kind::TraderLlama) {
            self.ai.state.llama_did_spit = self.llama.did_spit;
        }
    }

    /// After its goals: the spit its hurt-by goal took note of.
    pub(super) fn llama_settle(&mut self) {
        if matches!(self.kind, Kind::Llama | Kind::TraderLlama) {
            self.llama.did_spit = std::mem::take(&mut self.ai.state.llama_did_spit);
        }
    }
}

impl EntityWorld {
    /// `TraderLlama.maybeDespawn`: an untamed trader llama's clock runs
    /// down, and at nought it goes.
    pub(super) fn trader_llama_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        if e.owner.is_some() {
            return;
        }
        e.llama.despawn_delay -= 1;
        if e.llama.despawn_delay <= 0 {
            let _ = self.discard_mob(id);
        }
    }
}
