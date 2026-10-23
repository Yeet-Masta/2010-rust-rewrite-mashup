//! The player riding a mob (26.3 `Entity.startRiding`, `LivingEntity`'s
//! controlling passenger, `AbstractHorse`'s ridden movement and taming,
//! `DismountHelper`).
//!
//! - A grown horse or donkey used with an empty hand takes the player on.
//!   Untamed, it bucks a rider off now and then, a little calmer each
//!   time, until one time it is tamed (hearts). A tame one with a saddle is
//!   steered by the rider: forward and to the sides (backward at a quarter),
//!   turned to the rider's facing, and it leaps with the jump the rider
//!   charged.
//! - A saddled pig takes the player on too, and goes where its rider
//!   looks while the rider holds a carrot on a stick; using the stick
//!   spurs it on for a while.
//! - Sneaking gets the rider off beside the mount, where there is room.
//!
//! The rider's own client moves the vehicle in vanilla; here the client
//! sends its input and the server moves the vehicle for it.
use super::*;
use crate::horse::HorseKind;
use crate::species::Kind;

/// What the riding player does this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RideInput {
    /// `xxa` and `zza`.
    pub forward: f32,
    pub strafe: f32,
    /// A charged jump let go (`onPlayerJump`, 0 to 100), and one begun
    /// (`handleStartJump`).
    pub jump: Option<i32>,
    pub jump_started: bool,
    /// The sneak key: get off.
    pub dismount: bool,
    /// The jump key held (a happy ghast rises with it).
    pub jumping: bool,
    /// A carrot on a stick used (`FoodOnAStickItem.use`).
    pub boost: bool,
    /// The sprint key (a camel's quicker gait).
    pub sprint: bool,
    pub yaw: f32,
    pub pitch: f32,
}

/// `PLAYER.vehicleAttachment` (`Avatar.DEFAULT_VEHICLE_ATTACHMENT`).
const PLAYER_VEHICLE_ATTACHMENT: f64 = 0.6;

/// `getPlayerJumpPendingScale`.
fn jump_scale(amount: i32) -> f32 {
    if amount >= 90 {
        1.0
    } else {
        0.4 + 0.4 * amount as f32 / 90.0
    }
}

impl EntityWorld {
    /// The riding player's input for the tick to come.
    pub fn set_ride_input(&mut self, input: RideInput) {
        self.ride_input = input;
    }

    /// What the player rides.
    pub fn player_vehicle(&self) -> Option<MobHit> {
        match self.player_vehicle? {
            EntityKey::Cow(id) => Some(MobHit::Cow(id)),
            EntityKey::Pig(id) => Some(MobHit::Pig(id)),
            EntityKey::Mob(id) => Some(MobHit::Mob(id)),
            _ => None,
        }
    }

    /// Where the riding player's feet go (`positionRider`): the vehicle's
    /// passenger attachment, less the player's own.
    pub fn player_seat(&self) -> Option<DVec3> {
        let (position, attach) = match self.player_vehicle? {
            EntityKey::Cow(id) => {
                let e = self.cows.iter().find(|e| e.id == id)?;
                let attach = match e.horse.as_ref()?.kind {
                    HorseKind::Horse => 1.44375,
                    HorseKind::Donkey => 1.1125,
                };
                (e.cow.body.position, attach)
            }
            EntityKey::Pig(id) => (self.pigs.iter().find(|e| e.id == id)?.pig.body.position, 0.86875),
            EntityKey::Mob(id) => {
                let e = self.mobs.iter().find(|e| e.id == id)?;
                let (up, ahead) = mounts::seat(e.kind)?;
                let yaw = f64::from(e.yaw.to_radians());
                (e.body.position + DVec3::new(-yaw.sin() * ahead, 0.0, yaw.cos() * ahead), up)
            }
            _ => return None,
        };
        Some(position + DVec3::new(0.0, attach - PLAYER_VEHICLE_ATTACHMENT, 0.0))
    }

    /// `PlayerRideableJumping.canJump`: the rider's mount leaps with a
    /// charged jump (a saddled horse).
    pub fn vehicle_jumps(&self) -> bool {
        match self.player_vehicle {
            Some(EntityKey::Cow(id)) => self.cows.iter().find(|e| e.id == id).and_then(|e| e.horse.as_ref()).is_some_and(|h| h.saddled),
            Some(EntityKey::Mob(id)) => self.mob_steered(id) && self.mobs.iter().any(|e| e.id == id && !matches!(e.kind, Kind::Strider | Kind::HappyGhast)),
            _ => false,
        }
    }

    /// `Pig.getControllingPassenger`: a saddled pig's rider steers it with
    /// a carrot on a stick in hand.
    pub(super) fn pig_controlled(&self, id: u64) -> bool {
        self.player_vehicle == Some(EntityKey::Pig(id))
            && self.pigs.iter().any(|e| e.id == id && e.pig.saddled)
            && self.player_main_hands.get(&0).is_some_and(|i| i == "minecraft:carrot_on_a_stick")
    }

    /// Whether using a carrot on a stick now spurs the rider's pig on
    /// (`ItemSteerable.boost`): one it steers, not spurred already.
    pub fn vehicle_boostable(&self) -> bool {
        match self.player_vehicle {
            Some(EntityKey::Pig(id)) => self.pig_controlled(id) && self.pigs.iter().any(|e| e.id == id && !e.pig.boosting),
            Some(EntityKey::Mob(id)) => self.mob_steered(id) && self.mobs.iter().any(|e| e.id == id && e.kind == Kind::Strider && !e.mount.boosting),
            _ => false,
        }
    }

    /// The mount's health and its most, for the hearts that stand in for
    /// the rider's food (`Gui.getVehicleMaxHearts`).
    pub fn vehicle_health(&self) -> Option<(f32, f32)> {
        match self.player_vehicle? {
            EntityKey::Cow(id) => self.cows.iter().find(|e| e.id == id).map(|e| (e.cow.health, e.horse.as_ref().map_or(e.cow.max_health, |h| h.max_health))),
            EntityKey::Pig(id) => self.pigs.iter().find(|e| e.id == id).map(|e| (e.pig.health, 10.0)),
            EntityKey::Mob(id) => self.mobs.iter().find(|e| e.id == id).map(|e| (e.health, e.max_health())),
            _ => None,
        }
    }

    /// `getJumpCooldown`: a camel rests after a dash, its jump bar grey.
    pub fn vehicle_jump_cooldown(&self) -> bool {
        match self.player_vehicle {
            Some(EntityKey::Mob(id)) => self.mobs.iter().any(|e| e.id == id && e.mount.dash_cooldown > 0),
            _ => false,
        }
    }

    /// The vehicle's yaw, for the rider's view as it mounts.
    pub fn vehicle_yaw(&self) -> Option<f32> {
        match self.player_vehicle? {
            EntityKey::Cow(id) => self.cows.iter().find(|e| e.id == id).map(|e| e.cow.yaw),
            EntityKey::Pig(id) => self.pigs.iter().find(|e| e.id == id).map(|e| e.pig.yaw),
            EntityKey::Mob(id) => self.mobs.iter().find(|e| e.id == id).map(|e| e.yaw),
            _ => None,
        }
    }

    /// `doPlayerRide`: the player gets on (and a horse stops eating and
    /// rearing).
    pub fn mount_player(&mut self, hit: MobHit) {
        let key = match hit {
            MobHit::Cow(id) | MobHit::Mooshroom(id) => EntityKey::Cow(id),
            MobHit::Pig(id) => EntityKey::Pig(id),
            MobHit::Mob(id) => EntityKey::Mob(id),
            _ => return,
        };
        if self.player_vehicle.is_some() {
            return;
        }
        if let EntityKey::Cow(id) = key {
            if let Some(horse) = self.cows.iter_mut().find(|e| e.id == id).and_then(|e| e.horse.as_mut()) {
                horse.eating = false;
                horse.standing = false;
                horse.stand_counter = 0;
            }
        }
        self.player_vehicle = Some(key);
        self.dismounted = None;
    }

    /// Where the player got off since the last call, for its client to put
    /// it there.
    pub fn take_dismount(&mut self) -> Option<DVec3> {
        self.dismounted.take()
    }

    fn vehicle_body(&self, key: EntityKey) -> Option<&Body> {
        match key {
            EntityKey::Cow(id) => self.cows.iter().find(|e| e.id == id && e.cow.health > 0.0).map(|e| &e.cow.body),
            EntityKey::Pig(id) => self.pigs.iter().find(|e| e.id == id && e.pig.health > 0.0).map(|e| &e.pig.body),
            EntityKey::Mob(id) => self.mobs.iter().find(|e| e.id == id && e.health > 0.0).map(|e| &e.body),
            _ => None,
        }
    }

    /// `removeVehicle` with `getDismountLocationForPassenger`
    /// (`DismountHelper`): beside the vehicle to either side, then before
    /// and behind it, where two blocks are clear over something to stand
    /// on; else on top of it.
    pub(super) fn eject_player(&mut self, world: &dyn World) {
        let Some(key) = self.player_vehicle.take() else { return };
        // `HappyGhast.removePassenger`: up go its goggles.
        if let EntityKey::Mob(id) = key {
            if let Some(e) = self.mobs.iter_mut().find(|e| e.id == id && e.kind == Kind::HappyGhast) {
                let position = e.body.position;
                e.voices.push((Voice::Event("entity.happy_ghast.harness_goggles_up", 1.0, 1.0), position));
            }
        }
        let Some(body) = self.vehicle_body(key) else {
            self.dismounted = Some(self.player_seat_fallback());
            return;
        };
        let (p, width, height) = (body.position, f64::from(body.width), f64::from(body.height));
        let yaw = self.ride_input.yaw.to_radians();
        let (sin, cos) = (f64::from(yaw.sin()), f64::from(yaw.cos()));
        let reach = (width + 0.6) / 2.0 + 0.05;
        // The rider's right and left, then ahead and behind.
        let sides = [(cos, sin), (-cos, -sin), (-sin, cos), (sin, -cos)];
        let clear = |at: DVec3| {
            let feet = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
            !world.solid(feet) && !world.solid((feet.0, feet.1 + 1, feet.2)) && world.solid((feet.0, feet.1 - 1, feet.2))
        };
        let spot = sides.iter().flat_map(|&(dx, dz)| [0.0, 1.0, -1.0].map(|dy| p + DVec3::new(dx * reach, dy, dz * reach))).find(|&at| clear(at));
        self.dismounted = Some(spot.map_or(p + DVec3::new(0.0, height, 0.0), |at| DVec3::new(at.x, at.y.floor(), at.z)));
    }

    fn player_seat_fallback(&self) -> DVec3 {
        self.pushing_players.iter().find(|b| b.id == 0).map_or(DVec3::ZERO, |b| b.position)
    }

    /// Before the mobs move: the rider gets off when it sneaks, or when its
    /// mount is gone.
    pub(super) fn tick_rider(&mut self, world: &dyn World) {
        let Some(key) = self.player_vehicle else { return };
        if self.ride_input.dismount || self.vehicle_body(key).is_none() {
            self.eject_player(world);
        } else if let EntityKey::Mob(id) = key {
            self.buck_mob(id, world);
        }
    }

    /// `AbstractHorse.tickRidden` and its ridden travel for a tame,
    /// saddled horse under the player: turned to the rider, moved by its
    /// input at its own speed, leaping with a charged jump.
    pub(super) fn ride_horse(&mut self, index: usize, world: &impl World) -> Option<f32> {
        let input = self.ride_input;
        let entity = &mut self.cows[index];
        let horse = entity.horse.as_mut()?;
        let cow = &mut entity.cow;
        // `onPlayerJump`, then `handleStartJump`.
        if let Some(amount) = input.jump {
            if amount > 0 {
                horse.allow_stand_sliding = true;
                horse.stand_if_possible();
            }
            horse.jump_pending = jump_scale(amount.max(0));
        }
        if input.jump_started {
            horse.allow_stand_sliding = true;
            horse.stand_if_possible();
            entity.voices.push((Voice::Event("entity.horse.jump", 0.4, 1.0), cow.body.position));
        }
        cow.yaw = input.yaw;
        entity.body_rotation.body_yaw = input.yaw;
        entity.look_control.head_yaw = input.yaw;
        entity.look_control.pitch = input.pitch * 0.5;
        // `getRiddenInput`.
        let still = cow.body.on_ground && horse.jump_pending == 0.0 && horse.standing && !horse.allow_stand_sliding;
        let (strafe, forward) = if still {
            (0.0, 0.0)
        } else {
            let forward = if input.forward <= 0.0 { input.forward * 0.25 } else { input.forward };
            (input.strafe * 0.5, forward)
        };
        let travel = DVec3::new(f64::from(strafe), 0.0, f64::from(forward));
        // `executeRidersJump` from the ground.
        if cow.body.on_ground {
            if horse.jump_pending > 0.0 && !horse.jumping {
                let amount = horse.jump_pending;
                cow.body.velocity.y = horse.jump_strength * f64::from(amount);
                horse.jumping = true;
                if forward > 0.0 {
                    let yaw = cow.yaw.to_radians();
                    cow.body.velocity.x += f64::from(-0.4 * yaw.sin() * amount);
                    cow.body.velocity.z += f64::from(0.4 * yaw.cos() * amount);
                }
            }
            horse.jump_pending = 0.0;
        }
        let speed = entity.effects.movement_speed(horse.movement_speed) as f32;
        let was_water = entity.fluid.in_water();
        let mut fall = None;
        if was_water {
            cow.body.travel_water(world, travel, cow.yaw);
        } else if entity.fluid.in_lava() {
            cow.body.travel_lava(world, travel, cow.yaw, entity.fluid.lava_height);
        } else if let Some(fallen) = cow.body.travel_air(world, travel, speed, cow.yaw) {
            fall = fall_damage(&cow.body, &*world, fallen, false, &mut entity.voices);
        }
        // `AbstractHorse.travel`: back on the ground, the jump is spent.
        if cow.body.on_ground {
            horse.jumping = false;
            horse.allow_stand_sliding = false;
        }
        cow.forward = forward;
        cow.sideways = strafe;
        if !was_water {
            entity.fluid = FluidFrame::sample(world, cow.body.position, cow.body.width, cow.body.height);
            entity.was_touching_water = cow.body.touching_water;
        }
        fall
    }

    /// `Pig.tickRidden` and its ridden travel: turned to the rider, always
    /// walking ahead at its speed times 0.225, faster through a spurt
    /// (`ItemBasedSteering`).
    pub(super) fn ride_pig(&mut self, index: usize, world: &impl World) -> Option<f32> {
        let input = self.ride_input;
        let entity = &mut self.pigs[index];
        let pig = &mut entity.pig;
        // `boost`.
        if input.boost && !pig.boosting {
            pig.boosting = true;
            pig.boost_time = 0;
            pig.boost_total = entity.random.next_int(841) as i32 + 140;
        }
        pig.yaw = input.yaw;
        entity.body_rotation.body_yaw = input.yaw;
        entity.look_control.head_yaw = input.yaw;
        entity.look_control.pitch = input.pitch * 0.5;
        // `tickBoost`.
        if pig.boosting {
            let past = pig.boost_time > pig.boost_total;
            pig.boost_time += 1;
            if past {
                pig.boosting = false;
            }
        }
        let factor = if pig.boosting { 1.0 + 1.15 * (pig.boost_time as f32 / pig.boost_total as f32 * std::f32::consts::PI).sin() } else { 1.0 };
        let speed = (entity.effects.movement_speed(0.25) * 0.225) as f32 * factor;
        let travel = DVec3::new(0.0, 0.0, 1.0);
        let was_water = entity.fluid.in_water();
        let mut fall = None;
        if was_water {
            pig.body.travel_water(world, travel, pig.yaw);
        } else if entity.fluid.in_lava() {
            pig.body.travel_lava(world, travel, pig.yaw, entity.fluid.lava_height);
        } else if let Some(fallen) = pig.body.travel_air(world, travel, speed, pig.yaw) {
            fall = fall_damage(&pig.body, &*world, fallen, false, &mut entity.voices);
        }
        pig.forward = 1.0;
        pig.sideways = 0.0;
        if !was_water {
            entity.fluid = FluidFrame::sample(world, pig.body.position, pig.body.width, pig.body.height);
            entity.was_touching_water = pig.body.touching_water;
        }
        fall
    }

    /// `RunAroundLikeCrazyGoal.tick` for an untamed horse under the
    /// player: now and then it either takes to the rider (more likely the
    /// calmer it has grown) or throws it, a little calmer for it.
    pub(super) fn buck_or_tame(&mut self, index: usize, world: &dyn World) {
        let player_uuid = self.uuid_of(PLAYER_TARGET);
        let id = self.cows[index].id;
        let entity = &mut self.cows[index];
        let Some(horse) = entity.horse.as_mut() else { return };
        if horse.tame || entity.random.next_int(50) != 0 {
            return;
        }
        let max_temper = 100;
        if (entity.random.next_int(max_temper as u32) as i32) < horse.temper {
            // `tameWithName`.
            horse.tame = true;
            horse.owner = Some(player_uuid);
            self.entity_events.push((id, 7));
            return;
        }
        horse.temper = (horse.temper + 5).clamp(0, max_temper);
        // `makeMad`.
        if !horse.standing {
            horse.stand_if_possible();
            let pitch = (entity.random.next_float() - entity.random.next_float()) * 0.2 + 1.0;
            let event = if horse.kind == HorseKind::Donkey { "entity.donkey.angry" } else { "entity.horse.angry" };
            entity.voices.push((Voice::Event(event, crate::horse::SOUND_VOLUME, pitch), entity.cow.body.position));
        }
        self.entity_events.push((id, 6));
        self.eject_player(world);
    }
}
