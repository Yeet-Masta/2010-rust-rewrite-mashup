//! Silverfish and endermites (26.3 `Silverfish`,
//! `SilverfishWakeUpFriendsGoal`, `SilverfishMergeWithStoneGoal`,
//! `Endermite`).
//!
//! - A silverfish hurt by something calls its friends half a second later:
//!   it breaks the infested blocks about it (out to ten across and five up
//!   and down, nearest first, each time with an even chance of stopping),
//!   each letting out another silverfish (with mob griefing off they only
//!   turn back to stone).
//! - With nothing to chase it now and then burrows into the stone, cobble,
//!   stone bricks or deepslate beside it, which become infested, and is
//!   gone; otherwise it wanders.
//! - An endermite lives two minutes unless kept.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// The blocks an infested block hides in (`InfestedBlock.BLOCK_BY_HOST_BLOCK`).
const HOSTS: [&str; 7] = ["stone", "cobblestone", "stone_bricks", "mossy_stone_bricks", "cracked_stone_bricks", "chiseled_stone_bricks", "deepslate"];

/// `InfestedBlock.isCompatibleHostBlock`.
fn host(block: &minecraftoss_player::Block) -> bool {
    block.id.strip_prefix("minecraft:").is_some_and(|name| HOSTS.contains(&name))
}

/// `infestedStateByHost`, its properties kept (deepslate's axis).
fn infested(block: &minecraftoss_player::Block) -> minecraftoss_player::Block {
    let mut out = block.clone();
    out.id = format!("minecraft:infested_{}", block.id.trim_start_matches("minecraft:"));
    out
}

/// `hostStateByInfested`.
fn uninfested(block: &minecraftoss_player::Block) -> Option<minecraftoss_player::Block> {
    let name = block.id.strip_prefix("minecraft:infested_")?;
    let mut out = block.clone();
    out.id = format!("minecraft:{name}");
    Some(out)
}

/// A silverfish's calls for its friends.
#[derive(Clone, Debug, Default)]
pub struct SilverfishState {
    /// `lookForFriends`.
    pub look_for_friends: i32,
    /// The hurt it last answered.
    pub hurt_seen: Option<i32>,
    /// The infested blocks its call reached this tick, and the block it
    /// burrowed into.
    pub woken: Vec<Cell>,
    pub merge: Option<(Cell, minecraftoss_player::Block)>,
    /// `Endermite.life`.
    pub life: i32,
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct SilverfishCtx {
    pub look_for_friends: i32,
    pub woken: Vec<Cell>,
    pub merge: Option<(Cell, minecraftoss_player::Block)>,
}

/// `Silverfish.registerGoals`.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, crate::monster_ai::FloatGoal);
    selector.add(3, WakeUpFriendsGoal);
    selector.add(4, crate::monster_ai::MeleeAttackGoal { hits: true, raises_arms: false, follow_unseen: false, gives_up_in_light: false });
    selector.add(5, MergeWithStoneGoal { merge: None, wanted: None });
    selector
}

/// `SilverfishWakeUpFriendsGoal`.
#[derive(Clone)]
struct WakeUpFriendsGoal;

impl Goal<MonsterGoalContext> for WakeUpFriendsGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.silverfish.look_for_friends > 0
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.silverfish.look_for_friends > 0
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        ctx.silverfish.look_for_friends -= 1;
        if ctx.silverfish.look_for_friends > 0 {
            return;
        }
        let p = ctx.body.position;
        let base = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        // Out from its block: 0, 1, -1, 2, -2…
        let next = |o: i32| (if o <= 0 { 1 } else { 0 }) - o;
        let mut y = 0;
        while (-5..=5).contains(&y) {
            let mut x = 0;
            while (-10..=10).contains(&x) {
                let mut z = 0;
                while (-10..=10).contains(&z) {
                    let c = (base.0 + x, base.1 + y, base.2 + z);
                    if world.block(c).is_some_and(|b| b.id.starts_with("minecraft:infested_")) {
                        ctx.silverfish.woken.push(c);
                        if ctx.random.next_boolean() {
                            return;
                        }
                    }
                    z = next(z);
                }
                x = next(x);
            }
            y = next(y);
        }
    }
}

/// `SilverfishMergeWithStoneGoal`: `RandomStrollGoal(1.0, 10)` that, with
/// no target and its path done, now and then (mob griefing on) picks a
/// side and burrows into the host block there.
#[derive(Clone)]
struct MergeWithStoneGoal {
    merge: Option<(Cell, minecraftoss_player::Block)>,
    wanted: Option<DVec3>,
}

impl Goal<MonsterGoalContext> for MergeWithStoneGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.target.is_some() || !ctx.navigation.is_done() {
            return false;
        }
        self.merge = None;
        if ctx.mob_griefing && ctx.random.next_int(5) == 0 {
            // `Direction.getRandom`: down, up, north, south, west, east.
            let side = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)][ctx.random.next_int(6) as usize];
            let p = ctx.body.position;
            let c = ((p.x.floor() as i32) + side.0, ((p.y + 0.5).floor() as i32) + side.1, (p.z.floor() as i32) + side.2);
            if let Some(block) = world.block(c).filter(host) {
                self.merge = Some((c, block));
                return true;
            }
        }
        // `RandomStrollGoal.canUse` with its interval of ten.
        if ctx.no_action_time >= 100 || ctx.random.next_int(5) != 0 {
            return false;
        }
        let profile = ctx.profile();
        self.wanted = crate::stroll::default_random_position(world, &profile, ctx.body.position, 10, 7, &mut ctx.random);
        self.wanted.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        self.merge.is_none() && !ctx.navigation.is_done()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if let Some(merge) = self.merge.take() {
            ctx.silverfish.merge = Some(merge);
            return;
        }
        if let Some(to) = self.wanted.take() {
            let profile = ctx.profile();
            let _ = crate::navigation::navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 1.0, 0);
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.navigation.stop();
    }
}

impl MobEntity {
    /// Before its goals: a hurt by something sets its call going
    /// (`notifyHurt`).
    pub(super) fn silverfish_prepare(&mut self) {
        if self.kind != Kind::Silverfish {
            return;
        }
        let s = &mut self.silverfish;
        let hurt = self.ai.state.hurt_by.map(|(_, when)| when);
        if hurt.is_some() && hurt != s.hurt_seen && s.look_for_friends == 0 {
            s.look_for_friends = 10;
        }
        s.hurt_seen = hurt;
        self.ai.state.silverfish = SilverfishCtx { look_for_friends: s.look_for_friends, ..SilverfishCtx::default() };
    }

    /// After its goals: the call's countdown and what it reached.
    pub(super) fn silverfish_settle(&mut self) {
        if self.kind != Kind::Silverfish {
            return;
        }
        let ctx = std::mem::take(&mut self.ai.state.silverfish);
        self.silverfish.look_for_friends = ctx.look_for_friends.max(0);
        self.silverfish.woken = ctx.woken;
        self.silverfish.merge = ctx.merge;
    }
}

impl EntityWorld {
    /// The friends it woke break out (or, without mob griefing, turn back
    /// to stone); a burrowing silverfish infests its block and is gone with
    /// a puff (`spawnAnim`). An endermite's life runs out.
    pub(super) fn silverfish_step(&mut self, id: u64, world: &mut impl World) {
        let griefing = self.mob_griefing;
        let Some(e) = self.mob_mut(id) else { return };
        if e.kind == Kind::Endermite {
            // `Endermite.aiStep`: two minutes, unless kept.
            if !e.persistence_required {
                e.silverfish.life += 1;
            }
            if e.silverfish.life >= 2400 {
                let _ = self.discard_mob(id);
            }
            return;
        }
        let woken = std::mem::take(&mut e.silverfish.woken);
        let merge = e.silverfish.merge.take();
        for c in woken {
            if griefing {
                world.destroy_block(c);
            } else if let Some(stone) = world.block(c).as_ref().and_then(uninfested) {
                world.set_block(c, Some(stone));
            }
        }
        if let Some((c, block)) = merge {
            if world.block(c).is_some_and(|b| b.id == block.id) {
                world.set_block(c, Some(infested(&block)));
                // Entity event 20: the poof of `spawnAnim`.
                self.entity_events.push((id, 20));
                let _ = self.discard_mob(id);
            }
        }
    }
}
