//! A wither's spawn-in (26.3 `WitherBoss` with `getInvulnerableTicks`
//! above zero, as `WitherSkullBlock.checkSpawn` makes it): for 220 ticks
//! `WitherDoNothingGoal` holds its moves and looks, nothing short of what
//! bypasses invulnerability hurts it (`hurtServer`), and it heals 10 every
//! ten ticks (`customServerAiStep`), from a third of its health to full.
//! When the count runs out it bursts with power 7 at its eyes
//! (`ExplosionInteraction.MOB`, no fire) and every player hears it arrive
//! (global level event 1023). The count is saved as `Invul`.
use super::mobs::MobEntity;
use super::*;
use crate::monster_ai::Target;
use crate::species::Kind;

/// `LevelEvent.SOUND_WITHER_BOSS_SPAWN` (a global event).
pub const WITHER_SPAWNED: i32 = 1023;
/// `WitherBoss.makeInvulnerable`'s count.
pub const INVULNERABLE_TICKS: i32 = 220;

/// What a hurt does that does not land.
pub(super) const UNHURT: DamageResult = DamageResult { applied: false, dealt: 0.0, died: false, full: false };

impl MobEntity {
    /// Still gathering its strength (`getInvulnerableTicks() > 0`).
    pub fn invulnerable(&self) -> bool {
        self.kind == Kind::Wither && self.invulnerable_ticks > 0
    }

    /// `WitherBossRenderer.scale` and `getTextureLocation` between ticks:
    /// twice a model's size, a quarter smaller as the spawn-in starts and
    /// growing through it, and whether it wears the invulnerable skin
    /// (throughout, blinking every five ticks over the last 80).
    pub fn wither_spawn_look(&self, partial: f32) -> (f32, bool) {
        let ticks = if self.invulnerable() { self.invulnerable_ticks as f32 - partial } else { 0.0 };
        let scale = 2.0 - if ticks > 0.0 { ticks / INVULNERABLE_TICKS as f32 * 0.5 } else { 0.0 };
        let whole = ticks.floor() as i32;
        (scale, whole > 0 && (whole > 80 || whole / 5 % 2 != 1))
    }

    /// One tick of the spawn-in, in place of its goals and flight: the
    /// count runs down (`customServerAiStep`), it heals 10 on every tenth
    /// tick, and it moves only as its motion and gravity take it
    /// (`aiStep`'s vertical damping by 0.6; `FlyingMoveControl` with
    /// nowhere to go lets it fall). True when the count ran out.
    pub(super) fn wither_spawn_in(&mut self, world: &impl World) -> bool {
        let left = self.invulnerable_ticks - 1;
        self.invulnerable_ticks = left;
        if self.tick_count % 10 == 0 {
            let max = self.max_health();
            heal(&mut self.health, max, 10.0);
        }
        self.body.trim_small_velocity();
        self.body.velocity.y *= 0.6;
        let gravity = self.body.gravity;
        self.body.gravity = 0.08;
        let _ = self.body.travel_air_jumping(world, DVec3::ZERO, 0.0, self.yaw, false);
        self.body.gravity = gravity;
        self.body.fall_distance = 0.0;
        self.no_action_time = 0;
        left <= 0
    }
}

impl EntityWorld {
    /// The end of a wither's spawn-in: `level.explode` at its eyes with
    /// power 7 (the blocks break where mobs may grief, as a creeper's do),
    /// and `globalLevelEvent(1023)` at its block for the clients' sound.
    pub(super) fn wither_spawn_blast(&mut self, id: u64, world: &impl World, players: &[PlayerCandidate]) {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        let feet = e.body.position;
        let eyes = feet + DVec3::new(0.0, f64::from(e.eye_height()), 0.0);
        let blast = crate::creeper::CreeperExplosion { position: eyes, radius: 7.0, powered: false, fire: false };
        self.apply_explosion(world, id, blast, players);
        self.explosions.push(blast);
        let cell = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
        self.spawned.global_events.push((WITHER_SPAWNED, cell, 0));
    }
}

/// `#wither_friends` (`#undead`): what a wither spares.
pub fn wither_friend(kind: &str) -> bool {
    matches!(kind.trim_start_matches("minecraft:"), "zombie" | "husk" | "drowned" | "zombie_villager" | "skeleton" | "stray" | "bogged" | "parched" | "wither_skeleton")
        || crate::species::ALL.iter().any(|k| {
            let s = k.species();
            s.undead && s.id == kind
        })
}

/// A wither's heads and its temper (`DATA_TARGET_A..C`, `nextHeadUpdate`,
/// `idleHeadUpdates`, `destroyBlocksTick`, the side heads' rotations).
#[derive(Clone, Debug, Default)]
pub struct WitherState {
    /// What each head looks at: the middle one its target, the side ones
    /// their own.
    pub heads: [Option<Target>; 3],
    next_head_update: [i32; 2],
    idle_head_updates: [i32; 2],
    destroy_blocks: i32,
    /// The side heads' yaw and pitch now and last tick.
    pub head_rot: [(f32, f32); 2],
    pub head_rot_o: [(f32, f32); 2],
    /// Out: skulls to fire (head, aim, dangerous), and a block-breaking
    /// fit.
    shots: Vec<(usize, DVec3, bool)>,
    break_blocks: bool,
}

/// `WitherBoss.rotlerp`.
fn rotlerp(a: f32, b: f32, max: f32) -> f32 {
    a + super::phantoms::wrap_degrees(b - a).clamp(-max, max)
}

impl MobEntity {
    /// `isPowered`: at half health or less (arrows glance off).
    pub fn wither_powered(&self) -> bool {
        self.kind == Kind::Wither && self.health <= self.max_health() / 2.0
    }

    /// `getHeadX/Y/Z`: the middle head above it, the side ones out to
    /// either side of its body.
    pub fn wither_head(&self, head: usize) -> DVec3 {
        let p = self.body.position;
        if head == 0 {
            return p + DVec3::new(0.0, 3.0, 0.0);
        }
        let angle = (self.ai.body_rotation.body_yaw + 180.0 * (head as f32 - 1.0)).to_radians();
        DVec3::new(p.x + f64::from(angle.cos()) * 1.3, p.y + 2.2, p.z + f64::from(angle.sin()) * 1.3)
    }

    /// `hurtServer` before the hit: it starts tearing at the blocks about
    /// it, and its side heads lose patience.
    pub(super) fn wither_hurt(&mut self) {
        if self.kind != Kind::Wither {
            return;
        }
        let w = &mut self.wither;
        if w.destroy_blocks <= 0 {
            w.destroy_blocks = 20;
        }
        for idle in &mut w.idle_head_updates {
            *idle += 3;
        }
    }

    /// `WitherBoss.aiStep` before its move: it slows its climb, rises to
    /// above its target (five above, unless powered) and closes on it
    /// across, facing its way.
    pub(super) fn wither_motion(&mut self) {
        if self.kind != Kind::Wither || self.invulnerable() || self.health <= 0.0 {
            return;
        }
        let mut v = self.body.velocity * DVec3::new(1.0, 0.6, 1.0);
        if let Some(t) = self.wither.heads[0].and_then(|t| self.ai.state.info(t)) {
            let p = self.body.position;
            if p.y < t.position.y || !self.wither_powered() && p.y < t.position.y + 5.0 {
                let y = v.y.max(0.0);
                v.y = y + (0.3 - y * f64::from(0.6_f32));
            }
            let across = DVec3::new(t.position.x - p.x, 0.0, t.position.z - p.z);
            if across.length_squared() > 9.0 {
                let n = across.normalize();
                v += DVec3::new(n.x * 0.3 - v.x * 0.6, 0.0, n.z * 0.3 - v.z * 0.6);
            }
        }
        self.body.velocity = v;
        if v.x * v.x + v.z * v.z > 0.05 {
            self.yaw = (v.z.atan2(v.x) as f32).to_degrees() - 90.0;
        }
    }

    /// `customServerAiStep` past the spawn-in, then the side heads' turn
    /// (`aiStep`): each side head keeps its own target, fires at it every
    /// two or three seconds while it sees it within thirty blocks, and on
    /// normal and hard fires a stray skull when idle too long; the middle
    /// head follows its target; a hurt wither breaks the blocks about it a
    /// second later; it heals one every second.
    pub(super) fn wither_settle(&mut self, world: &dyn World, difficulty: i32, mob_griefing: bool) {
        if self.kind != Kind::Wither || self.invulnerable() || self.health <= 0.0 {
            return;
        }
        let me = self.body.position;
        for i in 1..3 {
            if self.tick_count < self.wither.next_head_update[i - 1] {
                continue;
            }
            self.wither.next_head_update[i - 1] = self.tick_count + 10 + self.random.next_int(10) as i32;
            if difficulty >= 2 {
                let idle = self.wither.idle_head_updates[i - 1];
                self.wither.idle_head_updates[i - 1] = idle + 1;
                if idle > 15 {
                    let x = me.x - 10.0 + self.random.next_double() * 20.0;
                    let y = me.y - 5.0 + self.random.next_double() * 10.0;
                    let z = me.z - 10.0 + self.random.next_double() * 20.0;
                    self.wither.shots.push((i, DVec3::new(x, y, z), true));
                    self.wither.idle_head_updates[i - 1] = 0;
                }
            }
            if let Some(head_target) = self.wither.heads[i] {
                let info = self.ai.state.info(head_target);
                let fine = info.filter(|&t| self.ai.state.can_attack(t) && t.position.distance_squared(me) <= 900.0);
                match fine {
                    Some(t) if self.ai.state.sees(world, t) => {
                        let aim = t.position + DVec3::new(0.0, f64::from(t.eye_height) * 0.5, 0.0);
                        self.wither.shots.push((i, aim, false));
                        self.wither.next_head_update[i - 1] = self.tick_count + 40 + self.random.next_int(20) as i32;
                        self.wither.idle_head_updates[i - 1] = 0;
                    }
                    _ => self.wither.heads[i] = None,
                }
            } else {
                // `getNearbyEntities(TARGETING_CONDITIONS)` in the box grown
                // 20 across and 8 up and down.
                let (half, height) = (f64::from(self.body.width) / 2.0 + 20.0, f64::from(self.body.height));
                let ctx = &self.ai.state;
                let mut candidates: Vec<Target> = ctx.players.iter().map(|p| Target::Player(p.id)).collect();
                candidates.extend(ctx.mobs.iter().filter(|m| m.id != self.id && !wither_friend(m.kind)).map(|m| Target::Mob(m.id)));
                candidates.extend(ctx.villagers.iter().map(|v| Target::Villager(v.id)));
                let mut found = Vec::new();
                for t in candidates {
                    let Some(info) = self.ai.state.info(t) else { continue };
                    let d = info.position - me;
                    let inside = d.x.abs() <= half && d.z.abs() <= half && info.position.y + f64::from(info.height) >= me.y - 8.0 && info.position.y <= me.y + height + 8.0;
                    if inside && info.position.distance_squared(me) <= 400.0 && self.ai.state.can_attack(info) && self.ai.state.sees(world, info) {
                        found.push(t);
                    }
                }
                if !found.is_empty() {
                    let pick = self.random.next_int(found.len() as u32) as usize;
                    self.wither.heads[i] = Some(found[pick]);
                }
            }
        }
        self.wither.heads[0] = self.ai.state.target;
        if self.wither.destroy_blocks > 0 {
            self.wither.destroy_blocks -= 1;
            if self.wither.destroy_blocks == 0 && mob_griefing {
                self.wither.break_blocks = true;
            }
        }
        if self.tick_count % 20 == 0 {
            let max = self.max_health();
            heal(&mut self.health, max, 1.0);
        }
        // The side heads turn toward their targets, or back to the front.
        for i in 0..2 {
            self.wither.head_rot_o[i] = self.wither.head_rot[i];
            let head = self.wither_head(i + 1);
            let (yaw, pitch) = self.wither.head_rot[i];
            self.wither.head_rot[i] = match self.wither.heads[i + 1].and_then(|t| self.ai.state.info(t)) {
                Some(t) => {
                    let d = t.position + DVec3::new(0.0, f64::from(t.eye_height), 0.0) - head;
                    let flat = (d.x * d.x + d.z * d.z).sqrt();
                    let to_yaw = (d.z.atan2(d.x) as f32).to_degrees() - 90.0;
                    let to_pitch = -(d.y.atan2(flat) as f32).to_degrees();
                    (rotlerp(yaw, to_yaw, 10.0), rotlerp(pitch, to_pitch, 40.0))
                }
                None => (rotlerp(yaw, self.ai.body_rotation.body_yaw, 10.0), pitch),
            };
        }
    }
}

impl EntityWorld {
    /// After its tick: the side heads' skulls (`performRangedAttack`, with
    /// the shot's level event 1024), and the blocks it tears through
    /// (`destroyBlock` across its body and a block about, short of
    /// `#wither_immune`, with level event 1022 if any broke).
    pub(super) fn wither_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let shots = std::mem::take(&mut e.wither.shots);
        let breaking = std::mem::take(&mut e.wither.break_blocks);
        let feet = e.body.position;
        let heads: Vec<(DVec3, DVec3, bool)> = shots.into_iter().map(|(head, aim, dangerous)| (e.wither_head(head), aim, dangerous)).collect();
        let (width, height) = (e.body.width, e.body.height);
        let cell = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
        for (from, aim, dangerous) in heads {
            self.spawned.global_events.push((1024, cell, 0));
            let skull = self.launch_missile(MissileKind::WitherSkull, Some(id), from, aim - from, 0.1, None);
            if let Some(m) = self.missiles.iter_mut().find(|m| m.id == skull) {
                m.dangerous = dangerous;
            }
        }
        if breaking {
            let reach = (width / 2.0 + 1.0).floor() as i32;
            let up = height.floor() as i32;
            let mut destroyed = false;
            for x in cell.0 - reach..=cell.0 + reach {
                for y in cell.1..=cell.1 + up {
                    for z in cell.2 - reach..=cell.2 + reach {
                        let c = (x, y, z);
                        if world.block(c).is_some_and(|b| b.id != "minecraft:air" && b.id != "minecraft:cave_air" && b.id != "minecraft:void_air") && !world.block_in_tag(c, "minecraft:wither_immune") {
                            world.destroy_block(c);
                            destroyed = true;
                        }
                    }
                }
            }
            if destroyed {
                self.spawned.global_events.push((1022, cell, 0));
            }
        }
    }
}
