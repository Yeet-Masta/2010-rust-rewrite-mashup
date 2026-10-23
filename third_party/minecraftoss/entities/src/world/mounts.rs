//! The generic mobs a player rides (26.3 `AbstractHorse` for mules,
//! skeleton and zombie horses and llamas, `Camel`, `Strider`).
//!
//! - A grown one used with an empty hand takes the player on (a skeleton
//!   horse only once tame, a sneaking player never). One tamed by riding
//!   (a mule, a zombie horse, a llama) bucks the rider off now and then
//!   until it takes to it.
//! - A saddle goes on a tame horse, any grown camel or strider. A saddled
//!   one is steered: a horse or camel by the rider's keys, with the
//!   charged jump (a camel's is a dash, then a rest); a strider walks
//!   where its rider looks while it holds a warped fungus on a stick,
//!   spurred on by using it. A llama is never steered.
use super::*;
use super::mobs::MobEntity;
use crate::species::Kind;

/// A mount's riding gear and temper.
#[derive(Clone, Debug, Default)]
pub struct MountState {
    pub saddled: bool,
    /// `temper`: how near an untamed one is to taking to its rider.
    pub temper: i32,
    /// `JUMP_STRENGTH`, rolled the first time it is wanted.
    pub jump_strength: f64,
    /// `playerJumpPendingScale` and `isJumping`.
    pub jump_pending: f32,
    pub jumping: bool,
    /// `ItemBasedSteering` (a strider).
    pub boosting: bool,
    pub boost_time: i32,
    pub boost_total: i32,
    /// `Camel.dashCooldown` and `DASH`.
    pub dash_cooldown: i32,
    pub dashing: bool,
}

/// What using a mount did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountUse {
    Pass,
    /// The saddle went on (consume it).
    Saddled,
    /// The player got on.
    Rode,
    /// An untamed horse shown an item it does not eat (`makeMad`).
    Mad,
}

fn horse_like(kind: Kind) -> bool {
    matches!(kind, Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse | Kind::Llama | Kind::TraderLlama)
}

fn camel(kind: Kind) -> bool {
    matches!(kind, Kind::Camel | Kind::CamelHusk)
}

/// The passenger attachment (up, forward) of a mount, if it is one.
pub fn seat(kind: Kind) -> Option<(f64, f64)> {
    Some(match kind {
        Kind::Mule => (1.2125, 0.0),
        Kind::SkeletonHorse | Kind::ZombieHorse => (1.31875, 0.0),
        Kind::Llama | Kind::TraderLlama => (1.37, -0.3),
        // `getBodyAnchorAnimationYOffset` standing: its height less 0.375.
        Kind::Camel | Kind::CamelHusk => (2.0, 0.5),
        Kind::Strider => (1.7, 0.0),
        _ => return None,
    })
}

impl MobEntity {
    fn tame(&self) -> bool {
        self.owner.is_some()
    }

    /// `getControllingPassenger` with the player on it.
    fn steered(&self, main_hand: Option<&str>) -> bool {
        match self.kind {
            Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => self.tame() && self.mount.saddled,
            Kind::Camel | Kind::CamelHusk => self.mount.saddled,
            Kind::Strider => self.mount.saddled && main_hand == Some("minecraft:warped_fungus_on_a_stick"),
            _ => false,
        }
    }

    fn jump_strength(&mut self) -> f64 {
        if self.mount.jump_strength == 0.0 {
            self.mount.jump_strength = match self.kind {
                // `generateJumpStrength` and `generateZombieHorseJumpStrength`.
                Kind::SkeletonHorse => 0.4 + (0..3).map(|_| self.random.next_double() * 0.2).sum::<f64>(),
                Kind::ZombieHorse => 0.5 + (0..3).map(|_| self.random.next_double() * (0.2 / 3.0)).sum::<f64>(),
                Kind::Camel | Kind::CamelHusk => 0.42,
                _ => 0.5,
            };
        }
        self.mount.jump_strength
    }

    /// The ridden travel of a steered mount (`tickRidden`, `travelRidden`):
    /// turned to the rider, then moved by its input at its speed. The fall
    /// damage a landing deals.
    pub(super) fn tick_ridden(&mut self, world: &impl World, input: RideInput) -> Option<f32> {
        let s = self.species();
        let kind = self.kind;
        let position = self.body.position;
        // `Camel.tick`: the dash ends back on the ground, the rest after it
        // runs down.
        if self.mount.dashing && self.mount.dash_cooldown < 50 && self.body.on_ground {
            self.mount.dashing = false;
        }
        if self.mount.dash_cooldown > 0 {
            self.mount.dash_cooldown -= 1;
            if self.mount.dash_cooldown == 0 {
                let ready = if kind == Kind::CamelHusk { "entity.camel_husk.dash_ready" } else { "entity.camel.dash_ready" };
                self.voices.push((Voice::Event(ready, 1.0, 1.0), position));
            }
        }
        let charges = horse_like(kind) || camel(kind) && self.mount.dash_cooldown <= 0 && self.body.on_ground;
        if charges {
            if let Some(amount) = input.jump {
                self.mount.jump_pending = if amount >= 90 { 1.0 } else { 0.4 + 0.4 * amount.max(0) as f32 / 90.0 };
            }
            if input.jump_started && camel(kind) {
                // `handleStartJump`.
                let dash = if kind == Kind::CamelHusk { "entity.camel_husk.dash" } else { "entity.camel.dash" };
                self.voices.push((Voice::Event(dash, s.sound_volume(), 1.0), position));
                self.mount.dashing = true;
            }
        }
        if kind == Kind::Strider {
            if input.boost && !self.mount.boosting {
                self.mount.boosting = true;
                self.mount.boost_time = 0;
                self.mount.boost_total = self.random.next_int(841) as i32 + 140;
            }
            if self.mount.boosting {
                let past = self.mount.boost_time > self.mount.boost_total;
                self.mount.boost_time += 1;
                if past {
                    self.mount.boosting = false;
                }
            }
        }
        self.ai.yaw = input.yaw;
        self.yaw = input.yaw;
        self.ai.body_rotation.body_yaw = input.yaw;
        self.ai.state.look_control.head_yaw = input.yaw;
        self.ai.state.look_control.pitch = input.pitch * 0.5;
        let base = self.effects.movement_speed(s.speed);
        let (strafe, forward, speed) = if kind == Kind::Strider {
            let boost = if self.mount.boosting { 1.0 + 1.15 * (self.mount.boost_time as f32 / self.mount.boost_total as f32 * std::f32::consts::PI).sin() } else { 1.0 };
            (0.0, 1.0, (base * if self.cold { 0.35 } else { 0.55 }) as f32 * boost)
        } else {
            let forward = if input.forward <= 0.0 { input.forward * 0.25 } else { input.forward };
            let dash = if camel(kind) && input.sprint && self.mount.dash_cooldown == 0 { 0.1 } else { 0.0 };
            (input.strafe * 0.5, forward, base as f32 + dash)
        };
        // `executeRidersJump` from the ground.
        if self.body.on_ground {
            if self.mount.jump_pending > 0.0 && !self.mount.jumping {
                let amount = self.mount.jump_pending;
                let power = self.jump_strength();
                let yaw = self.yaw.to_radians();
                if camel(kind) {
                    let push = 22.2222 * f64::from(amount) * base;
                    self.body.velocity += DVec3::new(-f64::from(yaw.sin()) * push, 1.4285 * f64::from(amount) * power, f64::from(yaw.cos()) * push);
                    self.mount.dash_cooldown = 55;
                    self.mount.dashing = true;
                } else {
                    self.body.velocity.y = power * f64::from(amount);
                    if forward > 0.0 {
                        self.body.velocity.x += f64::from(-0.4 * yaw.sin() * amount);
                        self.body.velocity.z += f64::from(0.4 * yaw.cos() * amount);
                    }
                }
                self.mount.jumping = true;
                self.body.needs_sync = true;
            }
            self.mount.jump_pending = 0.0;
        }
        let fluid = FluidFrame::sample(world, self.body.position, self.body.width, self.body.height);
        let travel = DVec3::new(f64::from(strafe), 0.0, f64::from(forward));
        let landed = if fluid.in_water() {
            self.body.travel_water(world, travel, self.yaw);
            None
        } else if fluid.in_lava() && kind != Kind::Strider {
            self.body.travel_lava(world, travel, self.yaw, fluid.lava_height);
            None
        } else {
            self.body.travel_air_jumping(world, travel, speed, self.yaw, false)
        };
        if kind == Kind::Strider {
            self.float_strider(world);
        }
        if self.body.on_ground {
            self.mount.jumping = false;
        }
        self.ai.forward = forward;
        self.ai.sideways = strafe;
        let sounds = self.movement_sounds();
        play_movement(&mut self.body, self.tick_count, &mut self.random, &mut self.voices, sounds);
        landed.and_then(|fallen| fall_damage(&self.body, world, fallen, false, &mut self.voices))
    }
}

impl EntityWorld {
    /// Whether the player steers the mob it rides.
    pub(super) fn mob_steered(&self, id: u64) -> bool {
        self.player_vehicle == Some(EntityKey::Mob(id))
            && self.mobs.iter().find(|e| e.id == id).is_some_and(|e| e.steered(self.player_main_hands.get(&0).map(String::as_str)))
    }

    /// `mobInteract` for a mount, once feeding it was tried: a saddle put
    /// on, or the player getting on.
    pub fn mount_interact(&mut self, id: u64, held: Option<&str>, sneaking: bool) -> MountUse {
        if self.player_vehicle.is_some() {
            return MountUse::Pass;
        }
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id && e.health > 0.0) else { return MountUse::Pass };
        let kind = e.kind;
        if seat(kind).is_none() || e.baby() || kind == Kind::SkeletonHorse && !e.tame() {
            return MountUse::Pass;
        }
        // A tame horse or a camel opens its inventory to a sneaking
        // player (not simulated); a strider is not got on sneaking.
        if sneaking {
            return MountUse::Pass;
        }
        let position = e.body.position;
        if held == Some("minecraft:saddle") && !e.mount.saddled {
            let takes = match kind {
                Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => e.tame(),
                Kind::Camel | Kind::CamelHusk | Kind::Strider => true,
                _ => false,
            };
            if takes {
                e.mount.saddled = true;
                let sound = match kind {
                    Kind::Camel | Kind::CamelHusk => "entity.camel.saddle",
                    Kind::Strider => "entity.strider.saddle",
                    _ => "entity.horse.saddle",
                };
                e.voices.push((Voice::Event(sound, 0.5, 1.0), position));
                return MountUse::Saddled;
            }
        }
        // `ZombieHorse.mobInteract`: an untamed one shown any other item
        // rears.
        if kind == Kind::ZombieHorse && held.is_some() && !e.tame() {
            e.voices.push((Voice::Event("entity.zombie_horse.angry", 0.8, 1.0), position));
            return MountUse::Mad;
        }
        if kind == Kind::Strider && !e.mount.saddled {
            return MountUse::Pass;
        }
        self.player_vehicle = Some(EntityKey::Mob(id));
        self.dismounted = None;
        MountUse::Rode
    }

    /// `RunAroundLikeCrazyGoal.tick` for an untamed mount under the player:
    /// now and then it takes to the rider, or throws it a little calmer.
    pub(super) fn buck_mob(&mut self, id: u64, world: &dyn World) {
        let player_uuid = self.uuid_of(PLAYER_TARGET);
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
        if !matches!(e.kind, Kind::Mule | Kind::ZombieHorse | Kind::Llama | Kind::TraderLlama) || e.tame() || e.random.next_int(50) != 0 {
            return;
        }
        // Llamas are quicker to take to a rider (`getMaxTemper` 30).
        let max = if matches!(e.kind, Kind::Llama | Kind::TraderLlama) { 30 } else { 100 };
        if (e.random.next_int(max as u32) as i32) < e.mount.temper {
            e.owner = Some(player_uuid);
            self.entity_events.push((id, 7));
            return;
        }
        e.mount.temper = (e.mount.temper + 5).clamp(0, max);
        let angry = match e.kind {
            Kind::Mule => "entity.mule.angry",
            Kind::ZombieHorse => "entity.zombie_horse.angry",
            _ => "entity.llama.angry",
        };
        let pitch = (e.random.next_float() - e.random.next_float()) * 0.2 + 1.0;
        let position = e.body.position;
        e.voices.push((Voice::Event(angry, 0.8, pitch), position));
        self.entity_events.push((id, 6));
        self.eject_player(world);
    }
}
