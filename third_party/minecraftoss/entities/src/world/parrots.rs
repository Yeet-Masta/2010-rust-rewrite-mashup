//! Parrots (26.3 `Parrot`, `ParrotWanderGoal`, `FollowMobGoal`).
//!
//! - One tick in four hundred it tries twice to mimic a mob within twenty
//!   blocks, each an even chance: the mob's own imitation sound, or a
//!   squawk for those it has none of.
//! - Out of the air it falls slowly, wings flapping (`calculateFlapping`).
//! - It wanders to a perch on leaves or a log close by, else somewhere
//!   ahead in the air, and keeps within three blocks of a mob of another
//!   kind that comes within seven.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;

/// The mobs a parrot can mimic (`Parrot.MOB_SOUND_MAP`'s keys), each with
/// its `entity.parrot.imitate.<name>`.
const IMITATED: [&str; 37] = [
    "blaze", "bogged", "breeze", "cave_spider", "creaking", "creeper", "drowned", "elder_guardian", "ender_dragon", "endermite", "evoker", "ghast", "guardian", "hoglin", "husk", "illusioner", "magma_cube", "phantom", "piglin", "piglin_brute", "pillager", "ravager", "shulker", "silverfish", "skeleton", "slime", "spider", "stray", "vex", "vindicator", "warden", "witch", "wither", "wither_skeleton", "zoglin", "zombie", "zombie_villager",
];

/// `getImitatedSound`.
fn imitated_sound(kind: &str) -> String {
    match kind.strip_prefix("minecraft:").filter(|name| IMITATED.contains(name)) {
        Some(name) => format!("entity.parrot.imitate.{name}"),
        None => "entity.parrot.ambient".to_owned(),
    }
}

/// A parrot's wings.
#[derive(Clone, Debug, Default)]
pub struct ParrotState {
    pub flap: f32,
    pub flap_o: f32,
    pub flap_speed: f32,
    pub flap_speed_o: f32,
    pub flapping: f32,
}

impl ParrotState {
    /// The wings' spread for a frame (`ParrotRenderer.getBob`).
    pub fn flap_angle(&self, partial: f32) -> f32 {
        let flap = self.flap_o + (self.flap - self.flap_o) * partial;
        let speed = self.flap_speed_o + (self.flap_speed - self.flap_speed_o) * partial;
        (flap.sin() + 1.0) * speed
    }
}

/// `Parrot.registerGoals` (its shoulder landing waits for shoulders).
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::mob_ai::PanicGoal::new(1.25));
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(1, crate::monster_ai::LookAtPlayerGoal { range: 8.0 });
    selector.add(2, crate::wolf_ai::SitGoal);
    selector.add(2, crate::wolf_ai::FollowOwnerGoal { speed: 1.0, start: 5.0, stop: 1.0 });
    selector.add(2, ParrotWanderGoal);
    selector.add(3, FollowMobGoal { following: None, recalculate: 0 });
    selector
}

/// `ParrotWanderGoal(parrot, 1.0)`: `WaterAvoidingRandomFlyingGoal` that
/// would rather perch.
#[derive(Clone)]
struct ParrotWanderGoal;

/// `getTreePos`: the first empty cell (with an empty cell above) on leaves
/// or a log within three blocks across and six up and down.
fn tree_pos(world: &dyn World, p: DVec3) -> Option<DVec3> {
    let here = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    let empty = |c: (i32, i32, i32)| world.block(c).is_none_or(|b| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"));
    let (x0, y0, z0) = ((p.x - 3.0).floor() as i32, (p.y - 6.0).floor() as i32, (p.z - 3.0).floor() as i32);
    let (x1, y1, z1) = ((p.x + 3.0).floor() as i32, (p.y + 6.0).floor() as i32, (p.z + 3.0).floor() as i32);
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = (x, y, z);
                if c == here {
                    continue;
                }
                let below = (x, y - 1, z);
                let perch = world.block_in_tag(below, "minecraft:leaves") || world.block_in_tag(below, "minecraft:logs");
                if perch && empty(c) && empty((x, y + 1, z)) {
                    return Some(DVec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5));
                }
            }
        }
    }
    None
}

impl Goal<MonsterGoalContext> for ParrotWanderGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        // `RandomStrollGoal.canUse` with its interval of 120.
        if ctx.no_action_time >= 100 || ctx.random.next_int(60) != 0 {
            return false;
        }
        let p = ctx.body.position;
        let mut to = None;
        if ctx.fluid.in_water() {
            let profile = ctx.profile();
            to = crate::stroll::land_random_position(world, &profile, p, 15, 15, &mut ctx.random);
        }
        if ctx.random.next_float() >= 0.001 {
            to = tree_pos(world, p);
        }
        let to = to.or_else(|| crate::mob_ai::random_open(world, ctx, 8, 7, false));
        match to {
            Some(to) => {
                ctx.fly_target = Some((to, 1.0));
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

/// `FollowMobGoal(parrot, 1.0, 3, 7)`: a mob of another kind within seven
/// blocks, kept within three.
#[derive(Clone)]
struct FollowMobGoal {
    following: Option<u64>,
    recalculate: i32,
}

impl Goal<MonsterGoalContext> for FollowMobGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let p = ctx.body.position;
        self.following = ctx.mobs.iter().find(|m| m.alive && m.kind != "minecraft:parrot" && (m.position - p).abs().max_element() <= 7.0 + f64::from(m.width)).map(|m| m.id);
        self.following.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let p = ctx.body.position;
        self.following.and_then(|id| ctx.mobs.iter().find(|m| m.id == id)).is_some_and(|m| m.alive && m.position.distance_squared(p) > 9.0) && ctx.fly_target.is_some()
    }
    fn start(&mut self, _ctx: &mut MonsterGoalContext) {
        self.recalculate = 0;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.following = None;
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(m) = self.following.and_then(|id| ctx.mobs.iter().find(|m| m.id == id)).cloned() else { return };
        let eye = m.position + DVec3::new(0.0, f64::from(m.eye_height), 0.0);
        let x_max = ctx.max_head_x_rot();
        ctx.look_control.set_look_at_with_limits(eye, 10.0, x_max);
        self.recalculate -= 1;
        if self.recalculate <= 0 {
            self.recalculate = 5;
            if m.position.distance_squared(ctx.body.position) > 9.0 {
                ctx.fly_target = Some((m.position, 1.0));
            } else {
                ctx.fly_target = None;
            }
        }
    }
}

impl MobEntity {
    /// After its move: `calculateFlapping`, which slows its fall.
    pub(super) fn parrot_flap(&mut self) {
        if self.kind != Kind::Parrot {
            return;
        }
        let air = !self.body.on_ground;
        let p = &mut self.parrot;
        p.flap_o = p.flap;
        p.flap_speed_o = p.flap_speed;
        p.flap_speed = (p.flap_speed + if air { 4.0 } else { -1.0 } * 0.3).clamp(0.0, 1.0);
        if air && p.flapping < 1.0 {
            p.flapping = 1.0;
        }
        p.flapping *= 0.9;
        if air && self.body.velocity.y < 0.0 {
            self.body.velocity.y *= 0.6;
        }
        p.flap += p.flapping * 2.0;
    }
}

impl EntityWorld {
    /// `Parrot.aiStep`'s mimicry: one tick in four hundred, two tries.
    pub(super) fn parrot_step(&mut self, id: u64) {
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
        if e.health <= 0.0 || e.random.next_int(400) != 0 {
            return;
        }
        let p = e.body.position;
        let near: Vec<&'static str> = self
            .mob_candidates()
            .into_iter()
            .filter(|m| m.id != id && m.alive && m.kind != "minecraft:parrot" && (m.position - p).abs().max_element() <= 20.0 + f64::from(m.width))
            .map(|m| m.kind)
            .collect();
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
        for _ in 0..2 {
            if e.random.next_int(2) != 0 || near.is_empty() {
                continue;
            }
            let kind = near[e.random.next_int(near.len() as u32) as usize];
            let pitch = (e.random.next_float() - e.random.next_float()) * 0.2 + 1.0;
            self.sounds.push(MobSound { event: imitated_sound(kind), position: p, volume: 0.7, pitch, category: "neutral" });
        }
    }
}
