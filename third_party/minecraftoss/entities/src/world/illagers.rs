//! The evoker's magic and the creatures of it (26.3 `SpellcasterIllager`,
//! `Evoker`, `EvokerFangs`, `Vex`).
//!
//! - A spellcaster casts one spell at a time: its use goal starts the cast
//!   (`spellCastingTickCount`, the prepare sound, the arms raised), works
//!   the spell after its warm-up (the cast sound), and waits out its
//!   interval before that spell again. While the count runs it stands and
//!   watches its target (`SpellcasterCastingSpellGoal`).
//! - The evoker summons three vexes when it has a target and few vexes are
//!   about (`EvokerSummonSpellGoal`), raises fangs at its target, a line of
//!   sixteen toward one far off or two rings about itself when close
//!   (`EvokerAttackSpellGoal`), and with no target turns a blue sheep red
//!   (`EvokerWololoSpellGoal`, where mobs may grief).
//! - Fangs wait out their delay, snap shut eight ticks later for 6 magic
//!   to whatever living thing stands in them but their caster's allies,
//!   and sink away (`EvokerFangs.tick`).
//! - A vex flies straight through blocks to where its move control wants
//!   (`VexMoveControl`), and one an evoker summoned hurts 1 a second once
//!   its 30 to 119 seconds of life are out.
//! - A ravager's bite stops it for half a second; a shield that blocks it
//!   stuns it two seconds half the time, after which it roars, hurting and
//!   flinging all about it (`Ravager.aiStep`, `blockedByShield`, `roar`). It
//!   tramples through leaves where mobs may grief.
use super::*;
use super::mobs::MobEntity;
use crate::species::Kind;

/// `SpellcasterIllager.IllagerSpell`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Spell {
    #[default]
    None,
    SummonVex,
    Fangs,
    Wololo,
    Disappear,
    Blindness,
}

impl Spell {
    /// The colour of the motes at its caster's hands (`spellColor`).
    pub fn color(self) -> [f32; 3] {
        match self {
            Spell::None => [0.0, 0.0, 0.0],
            Spell::SummonVex => [0.7, 0.7, 0.8],
            Spell::Fangs => [0.4, 0.3, 0.35],
            Spell::Wololo => [0.7, 0.5, 0.2],
            Spell::Disappear => [0.3, 0.3, 0.8],
            Spell::Blindness => [0.1, 0.1, 0.2],
        }
    }
}

/// The spellcasters' `SpellcasterUseSpellGoal`s: the evoker's and the
/// illusioner's (`IllusionerMirrorSpellGoal`, `IllusionerBlindnessSpellGoal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpellGoal {
    Summon,
    Fangs,
    Wololo,
    Mirror,
    Blindness,
}

impl SpellGoal {
    /// Each caster's use goals, in their goal order.
    fn of(kind: Kind) -> &'static [SpellGoal] {
        match kind {
            Kind::Evoker => &[SpellGoal::Summon, SpellGoal::Fangs, SpellGoal::Wololo],
            Kind::Illusioner => &[SpellGoal::Mirror, SpellGoal::Blindness],
            _ => &[],
        }
    }

    /// `getCastingTime`, `getCastingInterval` and `getCastWarmupTime`.
    fn timings(self) -> (i32, i32, i32) {
        match self {
            SpellGoal::Summon => (100, 340, 20),
            SpellGoal::Fangs => (40, 100, 20),
            SpellGoal::Wololo => (60, 140, 40),
            SpellGoal::Mirror => (20, 340, 20),
            SpellGoal::Blindness => (20, 180, 20),
        }
    }

    fn prepare_sound(self) -> &'static str {
        match self {
            SpellGoal::Summon => "entity.evoker.prepare_summon",
            SpellGoal::Fangs => "entity.evoker.prepare_attack",
            SpellGoal::Wololo => "entity.evoker.prepare_wololo",
            SpellGoal::Mirror => "entity.illusioner.prepare_mirror",
            SpellGoal::Blindness => "entity.illusioner.prepare_blindness",
        }
    }

    fn spell(self) -> Spell {
        match self {
            SpellGoal::Summon => Spell::SummonVex,
            SpellGoal::Fangs => Spell::Fangs,
            SpellGoal::Wololo => Spell::Wololo,
            SpellGoal::Mirror => Spell::Disappear,
            SpellGoal::Blindness => Spell::Blindness,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// `SpellcasterIllager`'s state: the spell its arms show, the cast's count
/// (`SpellTicks`), the use goal at work with its warm-up left (in goal
/// ticks, every other tick), each goal's `nextAttackTickCount`, and the
/// evoker's `wololoTarget`.
#[derive(Clone, Debug, Default)]
pub struct Spellcaster {
    pub spell: Spell,
    pub casting_ticks: i32,
    running: Option<(SpellGoal, i32)>,
    next: [i32; 5],
    wololo: Option<u64>,
    /// `IllusionerBlindnessSpellGoal.lastTargetId`.
    last_target: Option<crate::monster_ai::Target>,
}

/// `Illusioner`'s `clientSideIllusionOffsets` and `clientSideIllusionTicks`:
/// where its four images stand off it while unseen, easing over three
/// ticks from the last set; `moved` on the tick they jumped (with a puff of
/// cloud).
#[derive(Clone, Debug, Default)]
pub struct Illusion {
    previous: [[f32; 3]; 4],
    current: [[f32; 3]; 4],
    pub ticks: i32,
    pub moved: bool,
}

impl Illusion {
    /// `getIllusionOffsets` between ticks.
    pub fn offsets(&self, partial: f32) -> [[f32; 3]; 4] {
        if self.ticks <= 0 {
            return self.current;
        }
        let scale = (self.ticks as f32 - partial) / 3.0;
        let mut out = self.current;
        for (i, o) in out.iter_mut().enumerate() {
            for axis in 0..3 {
                o[axis] = self.current[i][axis] * (1.0 - scale) + self.previous[i][axis] * scale;
            }
        }
        out
    }
}

impl Spellcaster {
    /// `isCastingSpell` on the server.
    pub fn casting(&self) -> bool {
        self.casting_ticks > 0
    }
}

/// An evoker's fangs (`EvokerFangs`): where they rise, facing, their
/// `warmupDelayTicks` and `lifeTicks`, whether they have struck (the
/// spike event sent), and who raised them.
#[derive(Clone, Debug)]
pub struct FangsEntity {
    pub id: u64,
    pub position: DVec3,
    pub yaw: f32,
    warmup: i32,
    pub life_ticks: i32,
    pub biting: bool,
    owner: Option<u64>,
    pub alive: bool,
}

impl FangsEntity {
    /// `getAnimationProgress`: 0 before the bite, then rising to 1 over
    /// the twenty ticks it lingers.
    pub fn bite_progress(&self, partial: f32) -> f32 {
        if !self.biting {
            return 0.0;
        }
        let left = self.life_ticks - 2;
        if left <= 0 {
            1.0
        } else {
            1.0 - (left as f32 - partial) / 20.0
        }
    }
}

/// `TargetingConditions.range(16)` on a box grown by 16: within 16 blocks.
const SPELL_REACH: f64 = 16.0;

impl EntityWorld {
    pub fn fangs(&self) -> &[FangsEntity] {
        &self.fangs
    }

    /// One tick of an evoker's spellcasting, after its goals and moves:
    /// the cast's count runs down (`customServerAiStep`), the casting goal
    /// ends with it, the use goal at work warms up and works its spell, and
    /// on goal ticks a new one may start.
    pub(super) fn spellcaster_step(&mut self, id: u64, world: &impl World, players: &[PlayerCandidate]) {
        let (griefing, hard) = (self.mob_griefing, self.difficulty >= 3);
        let Some(index) = self.mobs.iter().position(|e| e.id == id) else { return };
        let e = &self.mobs[index];
        let goals = SpellGoal::of(e.kind);
        if goals.is_empty() || e.health <= 0.0 || e.no_ai {
            return;
        }
        let cast_sound = if e.kind == Kind::Illusioner { "entity.illusioner.cast_spell" } else { "entity.evoker.cast_spell" };
        let position = e.body.position;
        let tick = e.tick_count;
        let full = tick <= 1 || (tick + id as i32) % 2 == 0;
        let target = e.ai.state.target().filter(|t| t.alive);
        // What the goals would test: the vexes about it and the blue sheep.
        let vexes = self.mobs.iter().filter(|m| m.kind == Kind::Vex && m.health > 0.0 && m.body.position.distance(position) <= SPELL_REACH).count();
        let blue: Vec<u64> = self
            .sheep
            .iter()
            .filter(|s| s.health > 0.0 && s.sheep.wool.color() == "blue" && (s.body.position.y - position.y).abs() <= 4.0 + 2.0 && s.body.position.distance(position) <= SPELL_REACH)
            .map(|s| s.id)
            .collect();
        let sheep_alive = |id: u64, world: &EntityWorld| world.sheep.iter().any(|s| s.id == id && s.health > 0.0);
        let e = &mut self.mobs[index];
        let caster = &mut e.spells;
        if caster.casting_ticks > 0 {
            caster.casting_ticks -= 1;
        }
        // `SpellcasterCastingSpellGoal.stop`: the arms come down.
        if caster.casting_ticks == 0 {
            caster.spell = Spell::None;
        }
        let mut work = None;
        if let Some((goal, warmup)) = caster.running {
            let wololo = caster.wololo;
            let able = match goal {
                SpellGoal::Wololo => wololo.is_some(),
                _ => target.is_some(),
            };
            if !able || warmup <= 0 {
                caster.running = None;
                if goal == SpellGoal::Wololo {
                    caster.wololo = None;
                }
            } else if full {
                let left = warmup - 1;
                caster.running = Some((goal, left));
                if left == 0 {
                    work = Some(goal);
                }
            }
        }
        let mut prepared = None;
        if full && e.spells.running.is_none() && !e.spells.casting() {
            for &goal in goals {
                if tick < e.spells.next[goal.index()] {
                    continue;
                }
                let able = match goal {
                    // `IllusionerMirrorSpellGoal`: not already unseen.
                    SpellGoal::Mirror => target.is_some() && !e.effects.has(crate::effects::MobEffect::Invisibility),
                    // `IllusionerBlindnessSpellGoal`: a new target, where the
                    // local difficulty is above normal's (hard's from the
                    // start; normal's only after many days, not counted here).
                    SpellGoal::Blindness => target.is_some_and(|t| e.spells.last_target != Some(t.target)) && hard,
                    SpellGoal::Summon => target.is_some() && e.random.next_int(8) as usize + 1 > vexes,
                    SpellGoal::Fangs => target.is_some(),
                    SpellGoal::Wololo => {
                        if target.is_some() || !griefing || blue.is_empty() {
                            false
                        } else {
                            let pick = blue[e.random.next_int(blue.len() as u32) as usize];
                            e.spells.wololo = Some(pick);
                            true
                        }
                    }
                };
                if able {
                    let caster = &mut e.spells;
                    let (casting, interval, warmup) = goal.timings();
                    // `adjustedTickDelay`: the warm-up in goal ticks.
                    caster.running = Some((goal, (warmup + 1) / 2));
                    caster.casting_ticks = casting;
                    caster.next[goal.index()] = tick + interval;
                    caster.spell = goal.spell();
                    if goal == SpellGoal::Blindness {
                        caster.last_target = target.map(|t| t.target);
                    }
                    prepared = Some(goal.prepare_sound());
                    break;
                }
            }
        }
        // `SpellcasterCastingSpellGoal.tick`: it watches its target, or the
        // sheep it is turning.
        if e.spells.casting() {
            let look = target.map(|t| t.position + DVec3::new(0.0, f64::from(t.eye_height), 0.0));
            if let Some(at) = look {
                e.ai.state.look_control.set_look_at_with_limits(at, 30.0, 30.0);
            }
        }
        if let Some(event) = prepared {
            e.voices.push((Voice::Event(event, 1.0, 1.0), position));
        }
        let Some(goal) = work else { return };
        e.voices.push((Voice::Event(cast_sound, 1.0, 1.0), position));
        match goal {
            SpellGoal::Mirror => {
                let _ = self.mobs[index].effects.add(crate::effects::EffectInstance::new(crate::effects::MobEffect::Invisibility, 1200, 0));
            }
            SpellGoal::Blindness => {
                // Blindness for 400 ticks; mobs carry no blindness here.
                if let Some(crate::monster_ai::Target::Player(player)) = target.map(|t| t.target) {
                    if players.iter().any(|p| p.id == player && p.alive) {
                        let effect = minecraftoss_player::effects::EffectInstance::new(minecraftoss_player::effects::MobEffect::Blindness, 400, 0);
                        self.player_splashes.push((player, PlayerSplash { effects: vec![effect], instant: Vec::new(), extinguish: false, at: position }));
                    }
                }
            }
            SpellGoal::Summon => self.summon_vexes(index),
            SpellGoal::Fangs => {
                if let Some(target) = target {
                    self.raise_fangs(index, target.position, world);
                }
            }
            SpellGoal::Wololo => {
                let sheep = self.mobs[index].spells.wololo;
                if let Some(sheep) = sheep.filter(|&s| sheep_alive(s, self)) {
                    if let Some(s) = self.sheep.iter_mut().find(|s| s.id == sheep) {
                        // `DyeColor.RED`.
                        s.sheep.wool.set_color(14);
                    }
                }
            }
        }
    }

    /// `EvokerSummonSpellGoal.performSpellCasting`: three vexes a block up
    /// from cells up to two off its own, each bound there, owned by it,
    /// with an iron sword and 30 to 119 seconds to live.
    fn summon_vexes(&mut self, index: usize) {
        let (owner, feet) = (self.mobs[index].id, self.mobs[index].body.position);
        let base = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
        for _ in 0..3 {
            let random = &mut self.mobs[index].random;
            let (dx, dz) = (random.next_int(5) as i32 - 2, random.next_int(5) as i32 - 2);
            let cell = (base.0 + dx, base.1 + 1, base.2 + dz);
            let life = 20 * (30 + random.next_int(90) as i32);
            let at = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1), f64::from(cell.2) + 0.5);
            let vex = self.spawn_mob(Kind::Vex, at, 0.0, 0, false);
            if let Some(v) = self.mob_mut(vex) {
                v.hand = Some("minecraft:iron_sword".to_owned());
                v.owner_mob = Some(owner);
                v.bound_origin = Some(cell);
                v.life_ticks = Some(life);
            }
        }
    }

    /// `EvokerAttackSpellGoal.performSpellCasting`: fangs between the
    /// lower and a block above the higher of the two, two rings (five at
    /// 1.5, eight at 2.5 a little later) when the target is within three
    /// blocks, else a line of sixteen 1.25 apart, a tick apart.
    fn raise_fangs(&mut self, index: usize, target: DVec3, world: &impl World) {
        let (owner, feet) = (self.mobs[index].id, self.mobs[index].body.position);
        let min_y = target.y.min(feet.y);
        let max_y = target.y.max(feet.y) + 1.0;
        let angle = ((target.z - feet.z).atan2(target.x - feet.x)) as f32;
        let mut fangs = Vec::new();
        if feet.distance_squared(target) < 9.0 {
            for i in 0..5 {
                let a = angle + i as f32 * std::f32::consts::PI * 0.4;
                fangs.push((feet.x + f64::from(a.cos()) * 1.5, feet.z + f64::from(a.sin()) * 1.5, a, 0));
            }
            for i in 0..8 {
                let a = angle + i as f32 * std::f32::consts::PI * 2.0 / 8.0 + std::f32::consts::PI * 2.0 / 5.0;
                fangs.push((feet.x + f64::from(a.cos()) * 2.5, feet.z + f64::from(a.sin()) * 2.5, a, 3));
            }
        } else {
            for i in 0..16 {
                let reach = 1.25 * f64::from(i + 1);
                fangs.push((feet.x + f64::from(angle.cos()) * reach, feet.z + f64::from(angle.sin()) * reach, angle, i));
            }
        }
        for (x, z, a, delay) in fangs {
            self.create_fangs(x, z, min_y, max_y, a, delay, owner, world);
        }
    }

    /// `createSpellEntity`: down from the top of the span to the first
    /// cell over a sturdy top, the fangs standing on whatever shape fills
    /// it; none if the span ends first.
    #[allow(clippy::too_many_arguments)]
    fn create_fangs(&mut self, x: f64, z: f64, min_y: f64, max_y: f64, angle: f32, delay: i32, owner: u64, world: &impl World) {
        let (bx, bz) = (x.floor() as i32, z.floor() as i32);
        let mut y = max_y.floor() as i32;
        let bottom = min_y.floor() as i32 - 1;
        let mut top = None;
        loop {
            if world.solid((bx, y - 1, bz)) {
                let offset = world.collision_boxes((bx, y, bz)).iter().map(|b| b[4]).fold(0.0_f64, f64::max);
                top = Some(f64::from(y) + offset);
                break;
            }
            y -= 1;
            if y < bottom {
                break;
            }
        }
        let Some(floor) = top else { return };
        self.next_id += 1;
        let id = self.next_id;
        self.fangs.push(FangsEntity { id, position: DVec3::new(x, floor, z), yaw: angle.to_degrees(), warmup: delay, life_ticks: 22, biting: false, owner: Some(owner), alive: true });
    }

    /// `EvokerFangs.tick` on the server: past the warm-up, eight ticks on
    /// they bite everything living in their box grown 0.2 across (6 magic,
    /// sparing their caster and its fellow illagers), the client hears
    /// them snap, and they linger out their life.
    pub(super) fn tick_fangs(&mut self, players: &[PlayerCandidate]) {
        for index in 0..self.fangs.len() {
            let mut fangs = self.fangs[index].clone();
            fangs.warmup -= 1;
            if fangs.warmup < 0 {
                if fangs.warmup == -8 {
                    self.fangs_bite(&fangs, players);
                }
                if !fangs.biting {
                    fangs.biting = true;
                    let pitch = self.mob_owner_pitch(fangs.owner);
                    self.sounds.push(MobSound { event: "entity.evoker_fangs.attack".into(), position: fangs.position, volume: 1.0, pitch, category: "hostile_volume" });
                }
                fangs.life_ticks -= 1;
                if fangs.life_ticks < 0 {
                    fangs.alive = false;
                }
            }
            self.fangs[index] = fangs;
        }
        self.fangs.retain(|f| f.alive);
    }

    /// The bite's pitch: `nextFloat() * 0.2 + 0.85` from the caster's random.
    fn mob_owner_pitch(&mut self, owner: Option<u64>) -> f32 {
        owner.and_then(|o| self.mob_mut(o)).map_or(1.0, |e| e.random.next_float() * 0.2 + 0.85)
    }

    /// `dealDamageTo` for each living thing in the fangs' box.
    fn fangs_bite(&mut self, fangs: &FangsEntity, players: &[PlayerCandidate]) {
        // `EntityTypes.EVOKER_FANGS`: 0.5 across, 0.8 tall.
        let (min, max) = (fangs.position - DVec3::new(0.25 + 0.2, 0.0, 0.25 + 0.2), fangs.position + DVec3::new(0.25 + 0.2, 0.8, 0.25 + 0.2));
        let owner = fangs.owner.filter(|&o| self.mobs.iter().any(|m| m.id == o && m.health > 0.0));
        let allied = |kind: Option<&str>| matches!(kind, Some("minecraft:evoker" | "minecraft:illusioner" | "minecraft:pillager" | "minecraft:vindicator"));
        for target in self.potion_targets(owner, players) {
            let meets = target.min.x < max.x && target.max.x > min.x && target.min.y < max.y && target.max.y > min.y && target.min.z < max.z && target.max.z > min.z;
            if !meets || target.owner {
                continue;
            }
            if let Some(player_id) = target.id.checked_sub(PLAYER_TARGET) {
                self.player_hits.push(PlayerHit { player_id, damage: 6.0, kind: PlayerHitKind::Projectile { velocity: DVec3::ZERO, damage_type: if owner.is_some() { "minecraft:indirect_magic" } else { "minecraft:magic" }, effect: None, fire_seconds: 0.0 }, source: owner });
            } else {
                if owner.is_some() && allied(self.entity_type(target.id)) {
                    continue;
                }
                let attacker = owner.unwrap_or(fangs.id);
                let _ = self.mob_hits_mob(attacker, target.id, 6.0, fangs.position, 0.0);
            }
        }
    }
}

impl MobEntity {
    /// `Illusioner.aiStep`'s images while unseen: they jump to new spots
    /// when it is hurt or every minute (`mirror_move`), and fold back into
    /// it as the hurt wears off. Drawn from a random of their own, as the
    /// client's is.
    pub(super) fn illusion_step(&mut self) {
        if self.kind != Kind::Illusioner || !self.effects.has(crate::effects::MobEffect::Invisibility) {
            return;
        }
        let illusion = &mut self.illusion;
        illusion.moved = false;
        illusion.ticks = (illusion.ticks - 1).max(0);
        let hurt = self.damage.hurt_ticks;
        if hurt == 1 || self.tick_count % 1200 == 0 {
            illusion.ticks = 3;
            illusion.moved = true;
            let mut random = LegacyRandom::new(self.id ^ (self.tick_count as u64) << 16);
            illusion.previous = illusion.current;
            for offset in &mut illusion.current {
                let x = (-6.0 + random.next_int(13) as f32) * 0.5;
                let y = (random.next_int(6) as i32 - 4).max(0) as f32;
                let z = (-6.0 + random.next_int(13) as f32) * 0.5;
                *offset = [x, y, z];
            }
            let at = self.body.position;
            self.voices.push((Voice::Event("entity.illusioner.mirror_move", 1.0, 1.0), at));
        } else if hurt == 9 {
            illusion.ticks = 3;
            illusion.previous = illusion.current;
            illusion.current = [[0.0; 3]; 4];
        }
    }

    /// Unseen (`isInvisible`).
    pub fn invisible(&self) -> bool {
        self.effects.has(crate::effects::MobEffect::Invisibility)
    }

    /// `VexMoveControl.tick`: while it wants somewhere it pushes toward it
    /// at a twentieth of its speed modifier a tick, facing the way it
    /// moves (or its target), until it is nearer than its box's size, when
    /// it rests and halves its motion. Whether it is steering.
    pub(super) fn vex_move_control(&mut self) -> bool {
        let Some((wanted, speed)) = self.ai.state.fly_target else { return false };
        let delta = wanted - self.body.position;
        let length = delta.length();
        // `AABB.getSize`: the mean of its edges.
        let size = f64::from(self.body.width * 2.0 + self.body.height) / 3.0;
        if length < size {
            self.ai.state.fly_target = None;
            self.body.velocity *= 0.5;
            return false;
        }
        self.body.velocity += delta * (speed * 0.05 / length);
        let toward = match self.ai.state.target() {
            Some(t) => t.position - self.body.position,
            None => self.body.velocity,
        };
        self.ai.yaw = -(toward.x.atan2(toward.z).to_degrees() as f32);
        self.ai.body_rotation.body_yaw = self.ai.yaw;
        true
    }

    /// A vex's living tick: its goals, `VexMoveControl`, a move through
    /// whatever is in the way, and its look and body turning.
    pub(super) fn tick_vex(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[crate::monster_ai::MobCandidate], game_time: i64, difficulty: i32) {
        let position = self.body.position;
        if players.iter().any(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 32.0 * 32.0) {
            self.no_action_time = 0;
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
        ai.state.tick_count = self.tick_count;
        ai.state.on_fire = self.body.fire_ticks > 0;
        ai.state.random = std::mem::take(&mut self.random);
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut self.ai.state.random);
        let steering = self.vex_move_control();
        self.vex_travel();
        let eye = self.species().eye_height(false);
        let ai = &mut self.ai;
        ai.state.look_control.tick(self.body.position, eye, ai.body_rotation.body_yaw, steering);
        ai.body_rotation.tick(ai.yaw, &mut ai.state.look_control, self.previous_position, self.body.position);
        self.yaw = ai.yaw;
    }

    /// A vex's move (`noPhysics`, no gravity): straight through whatever is
    /// there, with the air's drag (`travelInAir`: 0.91 across, 0.98 up).
    pub(super) fn vex_travel(&mut self) {
        let body = &mut self.body;
        body.position += body.velocity;
        body.horizontal_collision = false;
        body.vertical_collision = false;
        body.on_ground = false;
        body.velocity = DVec3::new(body.velocity.x * 0.91, body.velocity.y * 0.98, body.velocity.z * 0.91);
        body.fall_distance = 0.0;
    }

    /// `Vex.tick`'s limited life: once out, 1 damage every second.
    pub(super) fn vex_life(&mut self) {
        let Some(left) = self.life_ticks else { return };
        let left = left - 1;
        if left <= 0 {
            self.life_ticks = Some(20);
            self.hurt(1.0);
        } else {
            self.life_ticks = Some(left);
        }
    }
}

/// `Ravager`'s `attackTick`, `stunnedTick` and `roarTick`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RavagerState {
    pub attack: i32,
    pub stun: i32,
    pub roar: i32,
}

impl RavagerState {
    /// `isImmobile`: biting, stunned or roaring, it does not move.
    pub fn immobile(&self) -> bool {
        self.attack > 0 || self.stun > 0 || self.roar > 0
    }
}

impl EntityWorld {
    /// `Ravager.aiStep` after the living step: it breaks the leaves it ran
    /// into (or jumps), and its roar, bite and stun count down; the stun's
    /// end starts the roar, whose middle is the blast.
    pub(super) fn ravager_step(&mut self, id: u64, world: &mut impl World, players: &[PlayerCandidate]) {
        let griefing = self.mob_griefing;
        let Some(e) = self.mob_mut(id) else { return };
        if e.kind != Kind::Ravager || e.health <= 0.0 {
            return;
        }
        if griefing && e.body.horizontal_collision {
            let half = f64::from(e.body.width) / 2.0 + 0.2;
            let p = e.body.position;
            let (min, max) = (p - DVec3::new(half, 0.2, half), p + DVec3::new(half, f64::from(e.body.height) + 0.2, half));
            let mut destroyed = false;
            for x in min.x.floor() as i32..=max.x.floor() as i32 {
                for y in min.y.floor() as i32..=max.y.floor() as i32 {
                    for z in min.z.floor() as i32..=max.z.floor() as i32 {
                        let Some(block) = world.block((x, y, z)) else { continue };
                        // `LeavesBlock`: `destroyBlock(pos, true, this)`.
                        if block.id.ends_with("_leaves") {
                            let drops = world.block_drops((x, y, z), &block);
                            world.set_block((x, y, z), None);
                            let center = DVec3::new(f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5);
                            for stack in drops {
                                world.spawn_popped_item(center, &stack);
                            }
                            destroyed = true;
                        }
                    }
                }
            }
            if !destroyed && e.body.on_ground {
                e.body.velocity.y = 0.42;
            }
        }
        let state = &mut e.ravager;
        let mut blast = false;
        if state.roar > 0 {
            state.roar -= 1;
            blast = state.roar == 10;
        }
        if state.attack > 0 {
            state.attack -= 1;
        }
        if state.stun > 0 {
            state.stun -= 1;
            if state.stun == 0 {
                state.roar = 20;
                let at = e.body.position;
                e.voices.push((Voice::Event("entity.ravager.roar", 1.0, 1.0), at));
            }
        }
        if blast {
            self.ravager_roar(id, players);
        }
    }

    /// `Ravager.roar`: everything living in its box grown by 4 but other
    /// ravagers takes 6 (illagers spared), and all but players are flung
    /// away, hardest up close (`strongKnockback`); the clients see the
    /// puff (event 69).
    fn ravager_roar(&mut self, id: u64, players: &[PlayerCandidate]) {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        let (p, half, height) = (e.body.position, f64::from(e.body.width) / 2.0 + 4.0, f64::from(e.body.height));
        let (min, max) = (p - DVec3::new(half, 4.0, half), p + DVec3::new(half, height + 4.0, half));
        let illager = |kind: Option<&str>| matches!(kind, Some("minecraft:evoker" | "minecraft:illusioner" | "minecraft:pillager" | "minecraft:vindicator"));
        for target in self.potion_targets(Some(id), players) {
            let meets = target.min.x < max.x && target.max.x > min.x && target.min.y < max.y && target.max.y > min.y && target.min.z < max.z && target.max.z > min.z;
            if !meets || target.owner {
                continue;
            }
            if let Some(player_id) = target.id.checked_sub(PLAYER_TARGET) {
                self.player_hits.push(PlayerHit { player_id, damage: 6.0, kind: PlayerHitKind::Melee { attacker: p, hunger_ticks: 0, lift: 0.2, weapon: Default::default(), effect: None }, source: Some(id) });
                continue;
            }
            let kind = self.entity_type(target.id);
            if kind == Some("minecraft:ravager") {
                continue;
            }
            if !illager(kind) {
                let _ = self.mob_hits_mob(id, target.id, 6.0, p, 0.0);
            }
            if let Some(body) = self.body_mut(target.id) {
                let (dx, dz) = (body.position.x - p.x, body.position.z - p.z);
                let distance = (dx * dx + dz * dz).max(0.001);
                body.velocity += DVec3::new(dx / distance * 4.0, 0.2, dz / distance * 4.0);
            }
        }
        self.entity_events.push((id, 69));
    }

    /// `Ravager.blockedByShield`: half the time it is stunned for two
    /// seconds (`RAVAGER_STUNNED`); the other half it would fling the
    /// player away (not felt here).
    pub(super) fn ravager_blocked(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        if e.kind != Kind::Ravager {
            return;
        }
        if e.random.next_double() < 0.5 {
            e.ravager.stun = 40;
            let at = e.body.position;
            e.voices.push((Voice::Event("entity.ravager.stunned", 1.0, 1.0), at));
        }
    }
}
