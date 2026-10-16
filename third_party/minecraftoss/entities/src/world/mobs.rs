//! The generic mob entity: every kind in `species` ticks here as the mobs
//! on the goal framework do (`Entity.baseTick`'s fluids, fire and walls,
//! `LivingEntity.baseTick`'s air, the hurt and death clocks, effects, the
//! ambient roll, then `aiStep`), over the goal sets of `mob_ai`. Walkers
//! move as the monsters do (`monster_ai_step`); fliers and swimmers steer
//! at their goals' wanted position through the air or water
//! (`FlyingMoveControl`, a `FlyingMob`'s and the fish's move controls) and
//! a fish out of water flops. Melee hits land with each kind's extra
//! (`CaveSpider`'s poison, `WitherSkeleton`'s wither, a hoglin's toss), and
//! the shooters loose their projectiles (a blaze's volley of three, the
//! ghast's fireball, a breeze's wind charge, arrows, shulker bullets,
//! spit, snowballs, the guardian's beam, the evoker's fangs).
use super::emissions::{base_tick_fluid, play_movement, MovementSounds};
use super::living::{breathe, dying_travel, fall_damage};
use super::missiles::MissileKind;
use super::*;
use crate::monster_ai::{MobCandidate, MonsterKind, Target};
use crate::species::{AttackKind, Kind, Motion, Species};
use minecraftoss_player::survival::EffectKind;

/// A mob of one of the generic kinds.
#[derive(Clone)]
pub struct MobEntity {
    pub id: u64,
    pub kind: Kind,
    pub body: Body,
    pub health: f32,
    pub damage: DamageState,
    /// Its status effects (`activeEffects`).
    pub effects: crate::effects::MobEffects,
    pub persistence_required: bool,
    /// Its body's facing (`yRot`) and pitch (`xRot`).
    pub yaw: f32,
    pub pitch: f32,
    /// `AgeableMob`'s age: below zero, a baby growing up; above, an adult
    /// resting after breeding.
    pub age: crate::age::Age,
    /// `Animal.inLove`: ticks of love left.
    pub in_love: i32,
    /// `TamableAnimal`'s owner (a player's UUID) and whether it was told to
    /// sit (cats and parrots).
    pub owner: Option<u128>,
    pub ordered_to_sit: bool,
    /// `Ocelot.isTrusting`: fed by a player, it no longer flees and breeds.
    pub trusting: bool,
    /// Its look (`Variant`, a texture's name), where it has one.
    pub variant: String,
    /// The key its type saves the look under, and whether as a number.
    pub variant_key: Option<(&'static str, bool)>,
    /// What it holds in its main hand (`equipment.mainhand`).
    pub hand: Option<String>,
    /// And in its off hand: the gold a piglin admires.
    pub off_hand: Option<String>,
    /// `ADMIRING_ITEM`: ticks a piglin has left looking at its gold.
    pub admire_ticks: i32,
    /// `AbstractPiglin`/`Hoglin.timeInOverworld`: ticks spent where
    /// piglins zombify, and `IsImmuneToZombification`.
    pub time_in_overworld: i32,
    pub zombify_immune: bool,
    /// `Pufferfish`'s `PUFF_STATE` (0 to 2) and its `inflateCounter` and
    /// `deflateTimer`.
    pub puff: i32,
    inflate: i32,
    deflate: i32,
    /// `Strider.isSuffocating`: out of the warmth of lava, cold and slow.
    pub cold: bool,
    /// `WitherBoss.getInvulnerableTicks` (`Invul`): its spawn-in.
    pub invulnerable_ticks: i32,
    /// A spellcaster's casting (`SpellcasterIllager`).
    pub spells: super::illagers::Spellcaster,
    /// A vex's evoker (`Vex.owner`), its `boundOrigin` and its limited
    /// life (`life_ticks`).
    pub owner_mob: Option<u64>,
    pub bound_origin: Option<(i32, i32, i32)>,
    pub life_ticks: Option<i32>,
    /// The illusioner's images.
    pub illusion: super::illagers::Illusion,
    pub ravager: super::illagers::RavagerState,
    pub shulker: super::shulkers::ShulkerState,
    pub squid: super::squids::SquidState,
    pub dolphin: super::dolphins::DolphinState,
    pub guardian: super::guardians::GuardianState,
    pub turtle: super::turtles::TurtleState,
    pub axolotl: super::axolotls::AxolotlState,
    pub frog: super::frogs::FrogState,
    pub bee: super::bees::BeeState,
    pub fox: super::foxes::FoxState,
    pub goat: super::goats::GoatState,
    pub trader: super::traders::TraderState,
    pub llama: super::llamas::LlamaState,
    pub rabbit: super::rabbits::RabbitState,
    pub polar: super::polar_bears::PolarState,
    pub silverfish: super::silverfish::SilverfishState,
    pub hoglin: super::hoglins::HoglinState,
    pub armadillo: super::armadillos::ArmadilloState,
    pub parrot: super::parrots::ParrotState,
    pub panda: super::pandas::PandaState,
    pub breeze: super::breezes::BreezeState,
    pub sniffer: super::sniffers::SnifferState,
    pub no_ai: bool,
    pub tick_count: i32,
    pub ambient_sound_time: i32,
    /// Sounds it made since the world last collected them.
    pub voices: Vec<(Voice, DVec3)>,
    pub no_action_time: i32,
    pub random: LegacyRandom,
    pub previous_position: DVec3,
    pub ai: Box<MonsterAi>,
    /// What it wears on its head, if anything: whether that can be worn
    /// down (`burnUndead`: the sun wears a helmet instead of burning it).
    pub head_item: Option<bool>,
    /// A blaze's volley: shots left, ticks to the next, and at whom.
    burst: (i32, i32, Option<Target>),
}

impl MobEntity {
    pub fn species(&self) -> &'static Species {
        self.kind.species()
    }

    pub fn baby(&self) -> bool {
        self.age.baby()
    }

    /// A strider on lava: a lava source bears it on its lower half when it
    /// came from above (`LiquidBlock.STABLE_SHAPE` for `canStandOnFluid`),
    /// standing there it is on the ground, and deeper in it rises
    /// (`floatStrider`). Lava, at its feet or below them, keeps it warm.
    fn float_strider(&mut self, world: &impl World) {
        let position = self.body.position;
        let cell = (position.x.floor() as i32, position.y.floor() as i32, position.z.floor() as i32);
        let lava = |pos| world.block(pos).is_some_and(|b| b.id == "minecraft:lava");
        let source = |pos| world.block(pos).is_some_and(|b| b.id == "minecraft:lava" && b.properties.get("level").is_none_or(|level| level == "0"));
        let top = f64::from(cell.1) + 0.5;
        if source(cell) && self.previous_position.y >= top - 1.0e-5 && position.y < top {
            self.body.position.y = top;
            self.body.velocity.y = self.body.velocity.y.max(0.0);
            self.body.on_ground = true;
        }
        let fluid = FluidFrame::sample(world, self.body.position, self.body.width, self.body.height);
        if fluid.in_lava() {
            let above = (cell.0, cell.1 + 1, cell.2);
            if source(cell) && self.body.position.y >= top - 1.0e-5 && !lava(above) {
                self.body.on_ground = true;
            } else {
                self.body.velocity = self.body.velocity * 0.5 + DVec3::new(0.0, 0.05, 0.0);
            }
        }
        let below = (cell.0, cell.1 - 1, cell.2);
        self.cold = !(lava(cell) || lava(below) || fluid.lava_height > 0.0);
    }

    /// `Pufferfish.setPuffState`: its size follows (`getScale`: half, 0.7
    /// and whole of its full size).
    pub fn set_puff(&mut self, puff: i32) {
        self.puff = puff.clamp(0, 2);
        let scale = [0.5_f32, 0.7, 1.0][self.puff as usize];
        let (width, height) = self.species().dimensions(false);
        self.body.width = width * scale;
        self.body.height = height * scale;
    }

    /// `PufferfishPuffGoal` (anything scary within two blocks, short of
    /// creative players and `#not_scary_for_pufferfish`), then
    /// `Pufferfish.tick`'s puffing up and down with its sounds; returns
    /// the players it stings (`playerTouch`: their touch box meets it).
    fn puff_step(&mut self, players: &[PlayerCandidate]) -> Vec<u64> {
        const NOT_SCARY: [&str; 14] = ["minecraft:turtle", "minecraft:guardian", "minecraft:elder_guardian", "minecraft:cod", "minecraft:pufferfish", "minecraft:salmon", "minecraft:tropical_fish", "minecraft:dolphin", "minecraft:squid", "minecraft:glow_squid", "minecraft:tadpole", "minecraft:nautilus", "minecraft:zombie_nautilus", "minecraft:sulfur_cube"];
        if self.kind != Kind::Pufferfish || self.health <= 0.0 || self.no_ai {
            return Vec::new();
        }
        let position = self.body.position;
        let half = f64::from(self.body.width) / 2.0;
        let (min, max) = (position - DVec3::new(half, 0.0, half), position + DVec3::new(half, f64::from(self.body.height), half));
        let meets = |at: DVec3, width: f64, height: f64, grow: DVec3| {
            let (lo, hi) = (min - grow, max + grow);
            at.x - width / 2.0 < hi.x && at.x + width / 2.0 > lo.x && at.y < hi.y && at.y + height > lo.y && at.z - width / 2.0 < hi.z && at.z + width / 2.0 > lo.z
        };
        let scary = players.iter().any(|p| p.alive && !p.spectator && p.attackable && meets(p.position, 0.6, 1.8, DVec3::splat(2.0)))
            || self.ai.state.mobs.iter().any(|m| m.id != self.id && m.alive && !NOT_SCARY.contains(&m.kind) && meets(m.position, f64::from(m.width), f64::from(m.height), DVec3::splat(2.0)));
        if scary {
            if self.inflate == 0 {
                self.inflate = 1;
                self.deflate = 0;
            }
        } else {
            self.inflate = 0;
        }
        let mut sound = None;
        if self.inflate > 0 {
            if self.puff == 0 {
                sound = Some("entity.puffer_fish.blow_up");
                self.set_puff(1);
            } else if self.inflate > 40 && self.puff == 1 {
                sound = Some("entity.puffer_fish.blow_up");
                self.set_puff(2);
            }
            self.inflate += 1;
        } else if self.puff != 0 {
            if self.deflate > 60 && self.puff == 2 {
                sound = Some("entity.puffer_fish.blow_out");
                self.set_puff(1);
            } else if self.deflate > 100 && self.puff == 1 {
                sound = Some("entity.puffer_fish.blow_out");
                self.set_puff(0);
            }
            self.deflate += 1;
        }
        if let Some(event) = sound {
            let pitch = voice_pitch(&mut self.random, false);
            self.voices.push((Voice::Event(event, 1.0, pitch), position));
        }
        if self.puff == 0 {
            return Vec::new();
        }
        players.iter().filter(|p| p.alive && !p.spectator && p.attackable && meets(p.position, 0.6, 1.8, DVec3::new(1.0, 0.5, 1.0))).map(|p| p.id).collect()
    }

    /// `AbstractPiglin.customServerAiStep` and `Hoglin`'s: the clock to
    /// zombification where piglins zombify (past 300 ticks it converts),
    /// then the end of admiring (`StopHoldingItemIfNoLongerAdmiring`): a
    /// grown piglin barters its gold ingot, throwing what it gives toward
    /// the nearest player, or about itself (`throwItems`).
    fn piglin_step(&mut self, zombify: bool, players: &[PlayerCandidate]) -> (Option<(DVec3, DVec3)>, Option<Kind>) {
        let into = match self.kind {
            Kind::Piglin | Kind::PiglinBrute => Kind::ZombifiedPiglin,
            Kind::Hoglin => Kind::Zoglin,
            _ => return (None, None),
        };
        let mut convert = None;
        if zombify && !self.zombify_immune && !self.no_ai {
            self.time_in_overworld += 1;
            if self.time_in_overworld > 300 {
                convert = Some(into);
            }
        } else {
            self.time_in_overworld = 0;
        }
        let mut barter = None;
        if self.admire_ticks > 0 {
            self.admire_ticks -= 1;
            if self.admire_ticks == 0 {
                let item = self.off_hand.take();
                if item.as_deref() == Some("minecraft:gold_ingot") && !self.baby() {
                    let position = self.position();
                    let nearest = players.iter().filter(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 16.0 * 16.0).min_by(|a, b| a.position.distance_squared(position).total_cmp(&b.position.distance_squared(position)));
                    let toward = match nearest {
                        Some(player) => player.position + DVec3::Y,
                        None => {
                            let (dx, dz) = (self.random.next_int(9) as f64 - 4.0, self.random.next_int(9) as f64 - 4.0);
                            position + DVec3::new(dx, 1.0, dz)
                        }
                    };
                    let from = position + DVec3::Y * (f64::from(self.eye_height()) - 0.3);
                    barter = Some((from, toward));
                }
            }
        }
        (barter, convert)
    }

    /// Where it stands (its feet).
    pub fn position(&self) -> DVec3 {
        self.body.position
    }

    pub fn eye_height(&self) -> f32 {
        self.species().eye_height(self.baby())
    }

    pub fn max_health(&self) -> f32 {
        // `Panda.setAttributes`: a weak panda has ten.
        if self.kind == Kind::Panda && self.panda.ready && self.panda.variant() == super::pandas::Gene::Weak {
            return 10.0;
        }
        self.species().health
    }

    pub fn set_random_seed(&mut self, seed: u64) {
        self.random = LegacyRandom::new(seed);
    }

    /// Its shooter is taking aim (the blaze alight, the ghast's open mouth,
    /// the shulker open).
    pub fn charging(&self) -> bool {
        self.ai.state.charging
    }

    /// `MeleeAttackGoal`'s swings so far, for the arm swing.
    pub fn swings(&self) -> u32 {
        self.ai.state.melee.swings
    }

    /// Sitting where its owner told it to (`isInSittingPose`).
    pub fn sitting(&self) -> bool {
        self.ai.state.wolf.sitting
    }

    /// Chasing a target (`isAggressive`).
    pub fn aggressive(&self) -> bool {
        self.ai.state.melee.aggressive || self.ai.state.target.is_some()
    }

    /// Its head's look (yaw, pitch) as its look control keeps it.
    pub fn look_control(&self) -> &crate::look::LookControl {
        &self.ai.state.look_control
    }

    /// `LivingEntity.hurtServer` for the generic mob: the idle clock, the
    /// damage cooldown, the hurt or death sound on a full hit.
    pub fn hurt(&mut self, amount: f32) -> DamageResult {
        // `WitherBoss.hurtServer`: nothing lands while it gathers itself.
        if self.invulnerable() {
            return super::withers::UNHURT;
        }
        if self.health > 0.0 && !self.damage.dead {
            self.no_action_time = 0;
        }
        let amount = self.armadillo_damage(amount);
        let max = self.max_health();
        let result = self.damage.hurt_generic(&mut self.health, max, amount);
        let position = self.position();
        self.damage.place_death(result, position, self.body.fire_ticks > 0);
        if result.applied && result.full {
            if !result.died {
                self.ambient_sound_time = -self.species().ambient_interval();
            }
            let baby = self.baby();
            let voice = hurt_voice(&mut self.random, result.died, baby);
            self.voices.push((voice, position));
        }
        if result.applied {
            self.shulker_hurt();
            let by_something = self.ai.state.hurt_by.is_some();
            self.squid_hurt(by_something);
            self.guardian_hurt();
        }
        result
    }

    /// A hit after its armor that remembers who dealt it.
    pub(super) fn hurt_by(&mut self, amount: f32, by: Option<Target>) -> DamageResult {
        let amount = self.species().after_armor(amount);
        self.axolotl_hurt(amount, by.is_some());
        // `setLastHurtByMob` comes before the hit's own reactions (a
        // squid's ink), and only for a hit that lands.
        let had = self.ai.state.hurt_by;
        if let Some(by) = by {
            self.ai.state.hurt_by = Some((by, self.tick_count));
        }
        let hit = self.hurt(amount);
        if !hit.applied {
            self.ai.state.hurt_by = had;
        }
        hit
    }

    pub(super) fn movement_sounds(&self) -> MovementSounds {
        let s = self.species();
        let step = s.step_sound();
        if s.category == crate::species::Category::Monster {
            MovementSounds::monster(step)
        } else {
            MovementSounds::creature(step)
        }
    }

    /// Whether it breathes water and dries out in air.
    fn dries_out(&self) -> bool {
        let s = self.species();
        s.aquatic && !matches!(self.kind, Kind::Axolotl | Kind::Turtle | Kind::ZombieNautilus | Kind::ElderGuardian | Kind::Guardian | Kind::Dolphin)
    }

    /// `aiStep` for a flier, floater or swimmer: the goals, then the steer
    /// at the wanted position, the move with its drag, the body turn.
    fn tick_free(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[MobCandidate], game_time: i64, difficulty: i32) {
        if self.kind == Kind::Vex {
            return self.tick_vex(world, players, villagers, game_time, difficulty);
        }
        if self.kind == Kind::Phantom {
            return self.tick_phantom(world, players, villagers, game_time, difficulty);
        }
        let s = self.species();
        let sounds = self.movement_sounds();
        let ai = &mut self.ai;
        let body = &mut self.body;
        let position = body.position;
        if players.iter().any(|p| p.alive && !p.spectator && p.position.distance_squared(position) < 32.0 * 32.0) {
            self.no_action_time = 0;
        }
        body.trim_small_velocity();
        self.no_action_time += 1;
        let fluid = FluidFrame::sample(world, position, body.width, body.height);
        ai.state.body = body.clone();
        ai.state.health = self.health;
        ai.state.game_time = game_time;
        ai.state.difficulty = difficulty;
        ai.state.players = players.to_vec();
        ai.state.villagers = villagers.to_vec();
        ai.state.fluid = fluid;
        ai.state.no_action_time = self.no_action_time;
        ai.state.tick_count = self.tick_count;
        ai.state.on_fire = body.fire_ticks > 0;
        ai.state.random = std::mem::take(&mut self.random);
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut ai.state.random);
        let in_water = fluid.in_water();
        let swims = s.motion == Motion::Swim;
        // How hard it pushes towards its goal each tick, by its move control.
        let push = match self.kind {
            _ if swims => 0.01 * s.speed,
            Kind::Ghast | Kind::HappyGhast => 0.025,
            Kind::Phantom => 0.06,
            Kind::Wither => 0.04,
            Kind::EnderDragon => 0.06,
            _ => 0.02 * s.flying_speed.max(0.1) * 2.0,
        };
        let mut steering = false;
        if let Some((target, modifier)) = ai.state.fly_target {
            let delta = target - body.position;
            let reach = if matches!(self.kind, Kind::Ghast | Kind::HappyGhast | Kind::EnderDragon) { 4.0 } else { 1.0 };
            if delta.length_squared() < reach || (swims && !in_water) {
                ai.state.fly_target = None;
            } else {
                steering = true;
                let direction = delta.normalize_or_zero();
                body.velocity += direction * (push * modifier);
                // `MoveControl.rotlerp` towards its heading, 90 degrees a tick
                // (a fish's 10).
                let heading = (delta.z.atan2(delta.x).to_degrees() as f32) - 90.0;
                let turn = if swims { 10.0 } else { 90.0 };
                ai.yaw = rotlerp(ai.yaw, heading, turn);
                let horizontal = (delta.x * delta.x + delta.z * delta.z).sqrt();
                self.pitch = -(delta.y.atan2(horizontal).to_degrees() as f32);
            }
        }
        // A floater faces where it drifts; one with a target faces it.
        if matches!(s.motion, Motion::Hover) {
            if let Some(info) = ai.state.target() {
                let d = info.position - body.position;
                ai.yaw = (d.z.atan2(d.x).to_degrees() as f32) - 90.0;
            } else if body.velocity.x.abs() + body.velocity.z.abs() > 1.0e-3 {
                ai.yaw = (-body.velocity.x.atan2(body.velocity.z).to_degrees()) as f32;
            }
        }
        // Weightless while it flies or swims; a parrot or fish at rest falls.
        let weightless = match s.motion {
            Motion::Hover => true,
            Motion::Fly => steering || matches!(self.kind, Kind::Bee | Kind::Allay | Kind::Vex),
            Motion::Swim => in_water,
            _ => false,
        };
        let gravity = body.gravity;
        if weightless {
            body.gravity = 0.0;
        }
        // `AbstractFish.aiStep`: stranded, it flops.
        if swims && !in_water && body.on_ground && body.vertical_collision && s.fish {
            let (a, b) = (self.random.next_float(), self.random.next_float());
            body.velocity += DVec3::new(f64::from(a * 2.0 - 1.0) * 0.05, 0.4, f64::from(b * 2.0 - 1.0) * 0.05);
            body.on_ground = false;
            body.needs_sync = true;
            let pitch = voice_pitch(&mut self.random, false);
            let flop = match self.kind {
                Kind::Cod => "entity.cod.flop",
                Kind::Salmon => "entity.salmon.flop",
                Kind::Pufferfish => "entity.puffer_fish.flop",
                Kind::TropicalFish => "entity.tropical_fish.flop",
                _ => "entity.tadpole.flop",
            };
            self.voices.push((Voice::Event(flop, s.sound_volume(), pitch), body.position));
        }
        let landed = body.travel_air_jumping(world, DVec3::ZERO, 0.0, ai.yaw, false);
        body.gravity = gravity;
        // The water and air drag the free movers feel past the plain air's.
        if swims && in_water {
            body.velocity *= 0.9 / 0.91;
        } else if s.motion == Motion::Hover {
            body.velocity.y *= 0.91 / 0.98;
        }
        if weightless {
            body.fall_distance = 0.0;
        } else if let Some(damage) = landed.and_then(|fallen| fall_damage(body, world, fallen, s.category == crate::species::Category::Monster, &mut self.voices)) {
            let _ = damage;
        }
        play_movement(body, self.tick_count, &mut self.random, &mut self.voices, sounds);
        let eye = s.eye_height(self.age.baby());
        ai.state.look_control.tick(body.position, eye, ai.body_rotation.body_yaw, steering);
        ai.body_rotation.tick(ai.yaw, &mut ai.state.look_control, self.previous_position, body.position);
        self.yaw = ai.yaw;
    }

    /// `aiStep` for a walker on the monster step; returns the fall damage
    /// a landing deals.
    fn tick_walk(&mut self, world: &impl World, players: &[PlayerCandidate], villagers: &[MobCandidate], game_time: i64, difficulty: i32) -> Option<f32> {
        let s = self.species();
        // A cold strider walks a third slower (`SUFFOCATING_MODIFIER`).
        let cold = if self.kind == Kind::Strider && self.cold { 0.66 } else { 1.0 };
        // A lazy panda ambles (`setAttributes`).
        let base = if self.kind == Kind::Panda && self.panda.variant() == super::pandas::Gene::Lazy { 0.07 } else { s.speed };
        let speed = self.effects.movement_speed(base * cold * if self.baby() && matches!(self.kind, Kind::ZombifiedPiglin | Kind::Piglin) { 1.5 } else { 1.0 });
        let step = MonsterStep { players, villagers, game_time, difficulty, movement_speed: speed, sounds: self.movement_sounds(), stands_on_lava: self.kind == Kind::Strider };
        self.ai.state.on_fire = self.body.fire_ticks > 0;
        self.ai.state.tick_count = self.tick_count;
        let eye = self.eye_height();
        self.ai.state.eye_height = eye;
        self.turtle_prepare();
        let landed = monster_ai_step(&mut self.ai, &mut self.body, self.health, &mut self.random, &mut self.voices, &mut self.no_action_time, self.previous_position, self.tick_count, self.id, world, &step);
        self.turtle_settle(world);
        if self.kind == Kind::Strider {
            self.float_strider(world);
        }
        self.yaw = self.ai.yaw;
        self.pitch = 0.0;
        // `Blaze.aiStep`: it falls slowly, and rises towards a target above.
        if self.kind == Kind::Blaze {
            if !self.body.on_ground && self.body.velocity.y < 0.0 {
                self.body.velocity.y *= 0.6;
            }
            if let Some(info) = self.ai.state.target() {
                if info.position.y + f64::from(info.eye_height) > self.body.position.y + f64::from(eye) {
                    self.body.velocity.y += (0.3 - self.body.velocity.y) * 0.3;
                    self.body.needs_sync = true;
                }
            }
        }
        let hostile = s.category == crate::species::Category::Monster;
        // Blazes, magma cubes and the like never take a fall (`#fall_damage_immune` and slow fallers).
        if matches!(self.kind, Kind::Blaze | Kind::MagmaCube | Kind::SulfurCube | Kind::SnowGolem | Kind::Cat | Kind::Ocelot | Kind::CopperGolem | Kind::Strider | Kind::Camel | Kind::CamelHusk) {
            return None;
        }
        landed.and_then(|fallen| fall_damage(&self.body, world, fallen, hostile, &mut self.voices))
    }

    /// A stationary mob's step: goals and its look, nothing else.
    fn tick_still(&mut self, world: &impl World, players: &[PlayerCandidate], game_time: i64, difficulty: i32) {
        let eye = self.species().eye_height(false);
        let ai = &mut self.ai;
        ai.state.body = self.body.clone();
        ai.state.health = self.health;
        ai.state.game_time = game_time;
        ai.state.difficulty = difficulty;
        ai.state.players = players.to_vec();
        ai.state.tick_count = self.tick_count;
        ai.state.random = std::mem::take(&mut self.random);
        let full = self.tick_count <= 1 || (self.tick_count + self.id as i32) % 2 == 0;
        ai.tick_goals(world, full);
        self.random = std::mem::take(&mut ai.state.random);
        self.body.velocity = DVec3::ZERO;
        ai.state.look_control.tick(self.body.position, eye, ai.body_rotation.body_yaw, false);
        self.no_action_time += 1;
    }
}

/// `MoveControl.rotlerp`: from `from` towards `to`, at most `max` degrees.
fn rotlerp(from: f32, to: f32, max: f32) -> f32 {
    let mut delta = (to - from) % 360.0;
    if delta >= 180.0 {
        delta -= 360.0;
    }
    if delta < -180.0 {
        delta += 360.0;
    }
    let mut out = from + delta.clamp(-max, max);
    if out < 0.0 {
        out += 360.0;
    } else if out > 360.0 {
        out -= 360.0;
    }
    out
}

/// A weapon's `attack_damage` modifier for its holder.
pub fn weapon_damage(item: &str) -> f32 {
    match item.trim_start_matches("minecraft:") {
        "wooden_sword" | "golden_sword" => 3.0,
        "stone_sword" => 4.0,
        "iron_sword" => 5.0,
        "diamond_sword" => 6.0,
        "netherite_sword" => 7.0,
        "wooden_axe" | "golden_axe" => 6.0,
        "stone_axe" | "iron_axe" | "diamond_axe" => 8.0,
        "netherite_axe" => 9.0,
        _ => 0.0,
    }
}

impl EntityWorld {
    /// A generic mob of `kind` at `position`, facing `yaw`, full of health;
    /// a baby when `age` is below zero.
    pub fn spawn_mob(&mut self, kind: Kind, position: DVec3, yaw: f32, age: i32, no_ai: bool) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let s = kind.species();
        // (Piglins and zoglins are babies by `IsBaby`, not by age.)
        let baby = age < 0;
        let (width, height) = s.dimensions(baby);
        let mut body = Body::new(position, width, height);
        body.step_height = s.step_height as f32;
        if s.motion == Motion::Hover {
            body.gravity = 0.0;
        }
        let mut ai = MonsterAi::of_kind(MonsterKind::Mob(kind), &body, yaw);
        ai.state.eye_height = s.eye_height(baby);
        ai.state.max_health = s.health;
        ai.state.health = s.health;
        let random = LegacyRandom::new(self.seed_uniquifier ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        self.mobs.push(MobEntity {
            id,
            kind,
            previous_position: position,
            body,
            health: s.health,
            damage: DamageState::default(),
            effects: Default::default(),
            persistence_required: false,
            yaw,
            pitch: 0.0,
            age: crate::age::Age { ticks: if baby { age } else { 0 }, ..Default::default() },
            in_love: 0,
            owner: None,
            ordered_to_sit: false,
            trusting: false,
            variant: String::new(),
            variant_key: None,
            hand: None,
            off_hand: None,
            admire_ticks: 0,
            time_in_overworld: 0,
            zombify_immune: false,
            puff: 0,
            inflate: 0,
            deflate: 0,
            cold: false,
            invulnerable_ticks: 0,
            spells: Default::default(),
            owner_mob: None,
            bound_origin: None,
            life_ticks: None,
            illusion: Default::default(),
            ravager: Default::default(),
            shulker: Default::default(),
            squid: Default::default(),
            dolphin: Default::default(),
            guardian: Default::default(),
            turtle: Default::default(),
            axolotl: Default::default(),
            frog: Default::default(),
            bee: Default::default(),
            fox: Default::default(),
            goat: Default::default(),
            trader: Default::default(),
            llama: Default::default(),
            rabbit: Default::default(),
            polar: Default::default(),
            silverfish: Default::default(),
            hoglin: Default::default(),
            armadillo: Default::default(),
            parrot: Default::default(),
            panda: Default::default(),
            breeze: Default::default(),
            sniffer: Default::default(),
            no_ai,
            tick_count: 0,
            ambient_sound_time: 0,
            voices: Vec::new(),
            no_action_time: 0,
            random,
            ai: Box::new(ai),
            head_item: None,
            burst: (0, 0, None),
        });
        if kind == Kind::Pufferfish {
            if let Some(e) = self.mobs.last_mut() {
                e.set_puff(0);
            }
        }
        self.order.push(EntityKey::Mob(id));
        self.file_in_section(EntityKey::Mob(id));
        id
    }

    /// The generic mobs.
    pub fn mobs(&self) -> &[MobEntity] {
        &self.mobs
    }

    pub fn mob_mut(&mut self, id: u64) -> Option<&mut MobEntity> {
        self.mobs.iter_mut().find(|e| e.id == id)
    }

    /// The players holding what tempts a kind (its tempt tag).
    fn tempters(&self, kind: Kind) -> Vec<u64> {
        let Some(tag) = kind.species().tempt_tag() else { return Vec::new() };
        let Some(recipes) = self.recipes.as_ref() else { return Vec::new() };
        self.player_main_hands.iter().filter(|(_, item)| recipes.item_in_tag(tag, item)).map(|(&id, _)| id).collect()
    }

    /// One generic mob's tick, as the spider's (`LivingEntity.tick`).
    pub(super) fn tick_mob(&mut self, id: u64, world: &mut impl World, players: &[PlayerCandidate], ticks: &dyn Fn(DVec3) -> bool) {
        let (game_time, difficulty, monsters_burn) = (self.game_time, self.difficulty, self.monsters_burn);
        let Some(kind) = self.mobs.iter().find(|e| e.id == id).map(|e| e.kind) else { return };
        let mobs = self.mob_candidates();
        let tempters = self.tempters(kind);
        let gilded = self.gilded_players.clone();
        let vitals = if kind == Kind::Dolphin { self.player_vitals.clone() } else { Vec::new() };
        let frog_food = if kind == Kind::Frog { self.frog_food() } else { Vec::new() };
        let llama = if matches!(kind, Kind::Llama | Kind::TraderLlama) { self.llama_context(id) } else { Default::default() };
        let cub_near = kind == Kind::PolarBear && self.polar_cub_near(id);
        let (piglin_seen, hoglin_seen) = if kind == Kind::Hoglin { self.hoglin_sights(id) } else { (None, None) };
        // The armadillo's scare sensor, four times a second.
        let scary = (kind == Kind::Armadillo && game_time % 5 == 0).then(|| self.armadillo_scared_by(id));
        let leashed = self.leash_holder(id).is_some();
        let thundering = world.thundering();
        let panda_items = if kind == Kind::Panda { self.panda_items(id, &*world) } else { Vec::new() };
        let bright_now = self.bright_outside;
        let (hives, bees_stay_in, zones) = if kind == Kind::Bee { (self.hives.clone(), self.bees_stay_in, self.bee_anger_zones.clone()) } else { (Vec::new(), false, Vec::new()) };
        let villagers: Vec<MobCandidate> = self
            .villagers
            .iter()
            .map(|v| MobCandidate { id: v.id, position: v.villager.body.position, eye_height: v.villager.eye_height(), width: v.villager.body.width, height: v.villager.body.height, alive: v.villager.health > 0.0, kind: "minecraft:villager", tame: false })
            .collect();
        let s = kind.species();
        // Its kind about it: those in love (for `BreedGoal`) and, for a
        // baby, the nearest grown-up (`FollowParentGoal`).
        let (position, baby) = self.mobs.iter().find(|e| e.id == id).map(|e| (e.body.position, e.baby())).unwrap();
        let mates: Vec<(u64, DVec3)> = self.mobs.iter().filter(|o| o.id != id && o.kind == kind && o.health > 0.0 && o.in_love > 0 && o.age.ticks == 0).map(|o| (o.id, o.body.position)).collect();
        let parent = baby
            .then(|| self.mobs.iter().filter(|o| o.kind == kind && !o.baby() && o.health > 0.0 && o.body.position.distance_squared(position) <= 64.0).map(|o| o.body.position).min_by(|a, b| a.distance_squared(position).total_cmp(&b.distance_squared(position))))
            .flatten();
        // Its owner, while that player is in the level.
        let owner_uuid = self.mobs.iter().find(|e| e.id == id).and_then(|e| e.owner);
        let owner = owner_uuid.and_then(|uuid| players.iter().find(|p| self.uuid_of(PLAYER_TARGET + p.id) == uuid)).map(|p| p.id);
        let piglins_zombify = self.piglins_zombify;
        if kind == Kind::Shulker {
            self.shulker_prepare(id, &*world);
        }
        // A vex's owner's target (`VexCopyOwnerTargetGoal`).
        let owner_target = self.mobs.iter().find(|e| e.id == id).and_then(|e| e.owner_mob).and_then(|o| self.mobs.iter().find(|m| m.id == o && m.health > 0.0)).and_then(|m| m.ai.state.target);
        let (melee, shot, stepped, bred, barter, convert, sting, spawn_blast) = {
            let e = self.mob_mut(id).unwrap();
            e.ai.state.owner_target = owner_target;
            e.ai.state.bound_origin = e.bound_origin;
            let was_charging = e.ai.state.charging;
            e.ai.state.wolf.tame = e.owner.is_some();
            e.ai.state.wolf.owner = owner;
            e.ai.state.wolf.ordered_to_sit = e.ordered_to_sit;
            e.ai.state.mobs = mobs;
            e.ai.state.tempters = tempters;
            e.ai.state.gilded = gilded.clone();
            if kind == Kind::Dolphin {
                e.ai.state.vitals = vitals;
            }
            if kind == Kind::Frog {
                e.frog_prepare(frog_food);
            }
            if kind == Kind::Fox {
                let (bright, thundering) = (bright_now, world.thundering());
                let p = e.body.position;
                let mut items: Vec<DVec3> = world.items_in(p - DVec3::splat(8.0), p + DVec3::splat(8.0)).into_iter().filter(|i| i.pickup_delay <= 0 && i.count > 0).map(|i| i.position).collect();
                items.sort_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)));
                e.fox_prepare(bright, thundering, items);
            }
            e.goat_prepare();
            e.trader_prepare();
            e.llama_prepare(llama);
            e.polar_prepare(cub_near);
            e.silverfish_prepare();
            e.hoglin_prepare(&*world, piglin_seen, hoglin_seen);
            e.armadillo_prepare(scary, leashed);
            e.panda_prepare(thundering, panda_items);
            e.breeze_prepare();
            e.sniffer_prepare(&*world, leashed);
            if kind == Kind::Bee && e.health > 0.0 {
                let raining = world.raining();
                e.bee_prepare(&hives, bees_stay_in, raining, &zones);
            }
            e.tadpole_grow();
            // A turtle with an egg is in no mood (`canFallInLove`, `TurtleBreedGoal`).
            e.ai.state.in_love = e.in_love > 0 && e.age.ticks == 0 && !e.turtle.has_egg;
            e.ai.state.mates = mates;
            e.ai.state.parent = parent;
            e.previous_position = e.body.position;
            e.tick_count += 1;
            // `AgeableMob.aiStep`: the age runs towards zero; a baby grown
            // up takes its full size.
            let was_baby = e.baby();
            e.age.tick(e.health > 0.0);
            if was_baby && !e.baby() {
                e.turtle.grew = kind == Kind::Turtle;
                let (w, h) = s.dimensions(false);
                e.body.width = w;
                e.body.height = h;
            }
            // `Animal.aiStep`: love runs out, and never lasts past the age.
            if e.age.ticks != 0 {
                e.in_love = 0;
            } else if e.in_love > 0 {
                e.in_love -= 1;
            }
            let sounds = e.movement_sounds();
            base_tick_fluid(&mut e.body, &*world, e.tick_count == 1, &mut e.random, &mut e.voices, sounds);
            hazards::burn(e, &*world, game_time);
            let eye = e.eye_height();
            // A vex passes through blocks (`noPhysics`: `isInWall` is false).
            if kind != Kind::Vex {
                hazards::suffocate(e, &*world, eye, game_time);
            }
            if e.health > 0.0 {
                if e.dries_out() {
                    // `WaterAnimal.handleAirSupply`: out of water it dries.
                    if FluidFrame::sample(&*world, e.body.position, e.body.width, e.body.height).in_water() {
                        e.body.air = 300;
                    } else {
                        e.body.air -= 1;
                        if e.body.air <= -20 {
                            e.body.air = 0;
                            e.hurt(2.0);
                        }
                    }
                } else if kind == Kind::Axolotl {
                    // `Axolotl.handleAirSupply`: water or rain keeps it wet.
                    let p = e.body.position;
                    let feet = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
                    let top = (feet.0, (p.y + f64::from(e.body.height)).floor() as i32, feet.2);
                    let wet = e.body.touching_water || world.rain_at(feet) || world.rain_at(top);
                    if e.axolotl_air(wet) {
                        e.hurt(2.0);
                    }
                } else if !s.aquatic && !s.undead && !matches!(kind, Kind::Blaze | Kind::Wither | Kind::EnderDragon | Kind::SnowGolem | Kind::CopperGolem | Kind::Creaking) && breathe(&mut e.body, &*world, eye, false) {
                    e.hurt(2.0);
                }
                // `isSensitiveToWater`: blazes, snow golems and striders
                // hurt in water.
                if matches!(kind, Kind::Blaze | Kind::SnowGolem | Kind::Strider | Kind::Breeze) && e.body.touching_water {
                    e.hurt(1.0);
                }
                // `Phantom.aiStep`: the sun burns it.
                if kind == Kind::Phantom {
                    let head = e.head_item;
                    burn_undead(&*world, &mut e.random, &mut e.body, eye, monsters_burn, head);
                }
            }
            let removed = e.damage.tick();
            if e.ai.state.hurt_by.is_some_and(|(_, when)| e.tick_count - when > 100) {
                e.ai.state.hurt_by = None;
            }
            let max = e.max_health();
            for work in e.effects.tick(s.undead) {
                match work.resolve(e.health, max) {
                    Some(EffectWork::Heal(amount)) => heal(&mut e.health, max, amount),
                    Some(EffectWork::HurtMagic(amount)) => {
                        e.hurt(amount);
                    }
                    _ => {}
                }
            }
            let digging = e.kind == Kind::Sniffer && matches!(e.sniffer.phase, super::sniffers::SnifferPhase::Digging | super::sniffers::SnifferPhase::Searching);
            if e.health > 0.0 && s.has_ambient() && !e.armadillo_scared() && !digging {
                if (e.random.next_int(1000) as i32) < e.ambient_sound_time {
                    e.ambient_sound_time = -s.ambient_interval();
                    let baby = e.baby();
                    let voice = Voice::Ambient(voice_pitch(&mut e.random, baby));
                    e.voices.push((voice, e.position()));
                } else {
                    e.ambient_sound_time += 1;
                }
            }
            let mut melee = None;
            let mut shot = None;
            let mut spawn_blast = false;
            if !removed {
                if e.health <= 0.0 && !e.no_ai {
                    let _ = dying_travel(&mut e.body, &*world, e.yaw, e.tick_count, &mut e.random, &mut e.voices, sounds, Some(s.category == crate::species::Category::Monster));
                } else if e.health > 0.0 && !e.no_ai && e.invulnerable() {
                    spawn_blast = e.wither_spawn_in(&*world);
                } else if e.health > 0.0 && !e.no_ai {
                    let position = e.body.position;
                    // A piglin admiring its gold stands still (`stopWalking`),
                    // as a spellcaster does while it casts.
                    // An axolotl playing dead lies still.
                    let still = e.axolotl_step_still();
                    let motion = if still || e.admire_ticks > 0 || e.spells.casting() || e.ravager.immobile() || e.armadillo_scared() || e.kind == Kind::Sniffer && e.sniffer.busy() { Motion::Stationary } else { s.motion };
                    match motion {
                        _ if matches!(e.kind, Kind::Guardian | Kind::ElderGuardian) => {
                            if let Some(damage) = e.tick_guardian(&*world, players, &villagers, game_time, difficulty) {
                                e.hurt(damage);
                            }
                        }
                        _ if e.kind == Kind::Dolphin => {
                            if let Some(damage) = e.tick_dolphin(&*world, players, &villagers, game_time, difficulty) {
                                e.hurt(damage);
                            }
                        }
                        _ if e.kind == Kind::Rabbit => {
                            if let Some(damage) = e.tick_rabbit(&*world, players, &villagers, game_time, difficulty) {
                                e.hurt(damage);
                            }
                        }
                        _ if matches!(e.kind, Kind::Squid | Kind::GlowSquid) => {
                            if let Some(damage) = e.tick_squid(&*world, players, &villagers, game_time, difficulty) {
                                e.hurt(damage);
                            }
                        }
                        Motion::Ground | Motion::Amphibious => {
                            if let Some(damage) = e.tick_walk(&*world, players, &villagers, game_time, difficulty) {
                                e.hurt(damage);
                            }
                        }
                        Motion::Stationary => e.tick_still(&*world, players, game_time, difficulty),
                        _ => e.tick_free(&*world, players, &villagers, game_time, difficulty),
                    }
                    e.axolotl_settle(game_time);
                    if kind == Kind::Frog {
                        e.frog_settle();
                    }
                    if kind == Kind::Bee {
                        e.bee_settle(&*world);
                    }
                    if kind == Kind::Fox {
                        e.fox_settle();
                    }
                    e.goat_settle();
                    e.trader_settle();
                    e.llama_settle();
                    e.polar_settle();
                    e.silverfish_settle();
                    e.hoglin_settle();
                    e.parrot_flap();
                    e.panda_settle(thundering);
                    e.breeze_settle();
                    e.sniffer_settle();
                    melee = e.ai.state.attack.take().map(|target| (target, position));
                    // A blaze's volley carries on a shot every six ticks.
                    if e.burst.0 > 0 {
                        e.burst.1 -= 1;
                        if e.burst.1 <= 0 {
                            e.burst.0 -= 1;
                            e.burst.1 = 6;
                            if let Some(target) = e.burst.2 {
                                shot = Some(target);
                            }
                        }
                    }
                    if let Some((target, _)) = e.ai.state.shoot.take() {
                        if kind == Kind::Blaze {
                            e.burst = (2, 6, Some(target));
                        }
                        shot = Some(target);
                    }
                } else {
                    e.body.trim_small_velocity();
                    monster_idle_without_ai(&*world, players, e.body.position, e.eye_height(), &mut e.no_action_time);
                }
                let old = e.previous_position;
                hazards::blocks_act(e, &*world, old, game_time);
            }
            let bred = e.ai.state.breed_with.take();
            if !removed {
                e.illusion_step();
            }
            if kind == Kind::Vex && !removed {
                // `VexChargeAttackGoal.start`'s sound, and its limited life.
                if e.ai.state.charging && !was_charging {
                    let at = e.position();
                    e.voices.push((Voice::Event("entity.vex.charge", 1.0, 1.0), at));
                }
                if e.health > 0.0 {
                    e.vex_life();
                }
            }
            let (barter, convert) = if removed { (None, None) } else { e.piglin_step(piglins_zombify, players) };
            let stung = if removed { Vec::new() } else { e.puff_step(players) };
            if !stung.is_empty() {
                let pitch = 1.0;
                let at = e.position();
                e.voices.push((Voice::Event("entity.puffer_fish.sting", 1.0, pitch), at));
            }
            let sting = (stung, e.puff, e.body.position);
            (melee, shot, !removed, bred, barter, convert, sting, spawn_blast)
        };
        if stepped && s.motion != Motion::Stationary {
            self.push_entities(EntityKey::Mob(id), &*world, game_time, ticks);
        }
        if let Some(e) = self.mob_mut(id) {
            if s.climber {
                e.body.climbing = e.body.horizontal_collision;
            }
        }
        if let Some((target, from)) = melee {
            self.mob_melee(id, kind, target, from);
        }
        if let Some(target) = shot {
            self.mob_shoot(id, kind, target, players);
        }
        if let Some(partner) = bred {
            self.breed_mobs(id, partner);
        }
        if let Some(barter) = barter {
            self.spawned.barters.push(barter);
        }
        if spawn_blast {
            self.wither_spawn_blast(id, &*world, players);
        }
        if matches!(kind, Kind::Evoker | Kind::Illusioner) {
            self.spellcaster_step(id, &*world, players);
        }
        if kind == Kind::Ravager {
            self.ravager_step(id, world, players);
        }
        if kind == Kind::Shulker {
            self.shulker_step(id, &*world);
        }
        if kind == Kind::Dolphin {
            self.dolphin_step(id);
        }
        if matches!(kind, Kind::Guardian | Kind::ElderGuardian) {
            self.guardian_step(id, kind, players);
        }
        if kind == Kind::Turtle {
            self.turtle_step(id, world);
        }
        if kind == Kind::Axolotl {
            self.axolotl_step(id, players);
        }
        if matches!(kind, Kind::Frog | Kind::Tadpole) {
            self.frog_step(id, world);
        }
        if kind == Kind::Bee {
            self.bee_step(id, world);
        }
        if kind == Kind::Fox {
            self.fox_step(id, world);
        }
        if kind == Kind::Goat {
            self.goat_step(id, world);
        }
        if kind == Kind::WanderingTrader {
            self.trader_step(id);
        }
        if kind == Kind::TraderLlama {
            self.trader_llama_step(id);
        }
        if kind == Kind::Rabbit {
            self.rabbit_step(id, world);
        }
        if matches!(kind, Kind::Silverfish | Kind::Endermite) {
            self.silverfish_step(id, world);
        }
        if kind == Kind::Hoglin {
            self.hoglin_step(id);
        }
        if kind == Kind::Armadillo {
            self.armadillo_step(id);
        }
        if kind == Kind::Parrot {
            self.parrot_step(id);
        }
        if kind == Kind::Panda {
            self.panda_step(id, world.thundering());
        }
        if kind == Kind::Sniffer {
            self.sniffer_step(id);
        }
        // `HurtByTargetGoal.alertOthers`.
        if let Some(attacker) = self.mob_mut(id).and_then(|e| e.ai.state.alert.take()) {
            self.alert_others(id, attacker);
        }
        self.mob_pick_up(id, world);
        // `Pufferfish.playerTouch`: 1 more than its puff, and poison for
        // three seconds a puff step.
        let (stung, puff, at) = sting;
        for player_id in stung {
            let effect = Some((EffectKind::Poison, 60 * puff as u32));
            self.player_hits.push(PlayerHit { player_id, damage: 1.0 + puff as f32, kind: PlayerHitKind::Melee { attacker: at, hunger_ticks: 0, lift: 0.0, weapon: Default::default(), effect }, source: Some(id) });
        }
        if let Some(into) = convert {
            self.zombify_mob(id, into);
        }
    }

    /// `AbstractPiglin.finishConversion`, `Hoglin.finishConversion`: the
    /// mob becomes its zombified kind where it stands, keeping its age,
    /// equipment and persistence, with the converted sound.
    fn zombify_mob(&mut self, id: u64, into: Kind) {
        let Some(old) = self.discard_mob(id) else { return };
        let event = match old.kind {
            Kind::PiglinBrute => "entity.piglin_brute.converted_to_zombified",
            Kind::Hoglin => "entity.hoglin.converted_to_zombified",
            _ => "entity.piglin.converted_to_zombified",
        };
        let new = self.spawn_mob(into, old.body.position, old.yaw, old.age.ticks, old.no_ai);
        if let Some(e) = self.mob_mut(new) {
            e.hand = old.hand;
            e.head_item = old.head_item;
            e.persistence_required = old.persistence_required;
            e.age.locked = old.age.locked;
            e.body.velocity = old.body.velocity;
            e.body.on_ground = old.body.on_ground;
            e.random = old.random;
            let baby = e.baby();
            let pitch = voice_pitch(&mut e.random, baby);
            let at = e.position();
            e.voices.push((Voice::Event(event, 1.0, pitch), at));
        }
    }

    /// `PiglinAi.mobInteract`: a grown piglin not already admiring takes
    /// one gold ingot from the hand into its off hand and admires it for
    /// 119 ticks. Whether it took it.
    pub fn piglin_take_gold(&mut self, id: u64, item: &str) -> bool {
        let Some(e) = self.mob_mut(id) else { return false };
        if e.kind != Kind::Piglin || e.baby() || e.admire_ticks > 0 || e.health <= 0.0 || item != "minecraft:gold_ingot" {
            return false;
        }
        e.off_hand = Some(item.to_owned());
        e.admire_ticks = 119;
        e.ai.state.target = None;
        true
    }

    /// `Animal.spawnChildFromBreeding`: both parents rest five minutes and
    /// lose their love, a baby of their kind appears where the first stands
    /// with one parent's look, and experience is left (1 to 7).
    fn breed_mobs(&mut self, first: u64, second: u64) {
        let Some(a) = self.mobs.iter().position(|e| e.id == first) else { return };
        let Some(b) = self.mobs.iter().position(|e| e.id == second) else { return };
        if self.mobs[b].in_love <= 0 || self.mobs[a].in_love <= 0 {
            return;
        }
        let kind = self.mobs[a].kind;
        let (position, yaw) = (self.mobs[a].body.position, self.mobs[a].yaw);
        let pick_first = self.mobs[a].random.next_boolean();
        let variant = if pick_first { self.mobs[a].variant.clone() } else { self.mobs[b].variant.clone() };
        let variant_key = self.mobs[a].variant_key;
        let xp = self.mobs[a].random.next_int(7) as i32 + 1;
        for i in [a, b] {
            self.mobs[i].age.set(6000);
            self.mobs[i].in_love = 0;
        }
        // `Frog.spawnChildFromBreeding`: spawn to lay, not a baby.
        if kind == Kind::Frog {
            self.mobs[a].frog.pregnant = true;
            self.spawned.experience.push((position, xp));
            self.entity_events.push((first, 18));
            return;
        }
        // `Sniffer.spawnChildFromBreeding`: an egg dropped, not a baby.
        if kind == Kind::Sniffer {
            self.spawned.items.push((position, ItemStack::new("minecraft:sniffer_egg", 1)));
            let pitch = (self.mobs[a].random.next_float() - self.mobs[a].random.next_float()) * 0.2 + 0.5;
            self.mobs[a].voices.push((Voice::Event("entity.sniffer.egg_plop", 1.0, pitch), position));
            self.spawned.experience.push((position, xp));
            self.entity_events.push((first, 18));
            return;
        }
        // `TurtleBreedGoal.breed`: an egg to carry home, not a baby.
        if kind == Kind::Turtle {
            self.mobs[a].turtle.has_egg = true;
            self.spawned.experience.push((position, xp));
            return;
        }
        // `Panda.setGeneFromParents`: a gene from each, one time in
        // thirty-two a fresh one.
        let genes = (kind == Kind::Panda).then(|| {
            let (pa, pb) = (self.mobs[a].panda.clone(), self.mobs[b].panda.clone());
            let r = &mut self.mobs[a].random;
            let pick = |p: &super::pandas::PandaState, r: &mut LegacyRandom| if r.next_boolean() { p.main } else { p.hidden };
            let (main, hidden) = if r.next_boolean() { (pick(&pa, r), pick(&pb, r)) } else { (pick(&pb, r), pick(&pa, r)) };
            (main, hidden)
        });
        let child = self.spawn_mob(kind, position, yaw, crate::age::Age::BABY_START, false);
        if let Some(c) = self.mob_mut(child) {
            c.variant = variant;
            c.variant_key = variant_key;
            if let Some((main, hidden)) = genes {
                let main = if c.random.next_int(32) == 0 { super::pandas::Gene::random_gene(&mut c.random) } else { main };
                let hidden = if c.random.next_int(32) == 0 { super::pandas::Gene::random_gene(&mut c.random) } else { hidden };
                c.panda.main = main;
                c.panda.hidden = hidden;
                c.panda.hidden_saved = true;
                c.panda.ready = true;
                c.variant = main.name().to_owned();
            }
        }
        self.spawned.experience.push((position, xp));
        self.entity_events.push((first, 18));
    }

    /// `Entity.discard` of a generic mob (scooped into a bucket, say): it
    /// leaves without dying or dropping anything.
    pub fn discard_mob(&mut self, id: u64) -> Option<MobEntity> {
        let index = self.mobs.iter().position(|e| e.id == id)?;
        self.order.retain(|key| !matches!(key, EntityKey::Mob(other) if *other == id));
        Some(self.mobs.remove(index))
    }

    /// Taming and orders (`Cat.mobInteract`, `Parrot.mobInteract`,
    /// `Ocelot.mobInteract`), before feeding: a wild cat eats raw cod or
    /// salmon and is tamed one time in three, a wild parrot eats seeds and is
    /// tamed one time in ten (a cookie kills it), a wary ocelot fed within
    /// three blocks comes to trust one time in three (hearts 7, smoke 6); a
    /// tame cat is healed by its food when hurt, and its owner's use (other
    /// than its food at full health) tells a cat or parrot to sit or stand.
    /// None when this is not for taming.
    pub fn tame_mob(&mut self, id: u64, player: u64, inventory: &mut minecraftoss_player::inventory::Inventory, hand: usize, infinite: bool) -> Option<Vec<(u64, u8)>> {
        let player_uuid = self.uuid_of(PLAYER_TARGET + player);
        let player_feet = self.pushing_players.iter().find(|p| p.id == player).map(|p| p.position);
        let recipes = self.recipes.clone();
        let e = self.mobs.iter_mut().find(|e| e.id == id)?;
        if e.health <= 0.0 || !matches!(e.kind, Kind::Cat | Kind::Parrot | Kind::Ocelot) {
            return None;
        }
        let item = inventory.slots.get(hand).and_then(Option::as_ref).map(|s| s.id.clone()).unwrap_or_default();
        let in_tag = |tag: &str| recipes.as_ref().is_some_and(|r| r.item_in_tag(tag, &item));
        let consume = |inventory: &mut minecraftoss_player::inventory::Inventory| {
            if !infinite {
                crate::animal::consume_one(inventory, hand);
            }
        };
        let position = e.body.position;
        let owned = e.owner == Some(player_uuid);
        match e.kind {
            Kind::Cat if e.owner.is_none() && in_tag("minecraft:cat_food") => {
                consume(inventory);
                let event = if e.random.next_int(3) == 0 {
                    e.owner = Some(player_uuid);
                    e.ordered_to_sit = true;
                    e.persistence_required = true;
                    e.ai.state.navigation.stop();
                    7
                } else {
                    6
                };
                e.voices.push((Voice::Event("entity.cat.eat", 1.0, 1.0), position));
                Some(vec![(id, event)])
            }
            Kind::Cat if owned => {
                if in_tag("minecraft:cat_food") && e.health < e.max_health() {
                    // `Cat.mobInteract`: its food heals it by the food's nutrition.
                    consume(inventory);
                    let heal = minecraftoss_player::food::catalog().get(&item).map_or(2.0, |f| f32::from(f.nutrition));
                    let max = e.max_health();
                    crate::effects::heal(&mut e.health, max, heal);
                    e.voices.push((Voice::Event("entity.cat.eat", 1.0, 1.0), position));
                    return Some(Vec::new());
                }
                if in_tag("minecraft:cat_food") {
                    return None;
                }
                e.ordered_to_sit = !e.ordered_to_sit;
                e.ai.state.navigation.stop();
                Some(Vec::new())
            }
            Kind::Parrot if e.owner.is_none() && in_tag("minecraft:parrot_food") => {
                consume(inventory);
                e.voices.push((Voice::Event("entity.parrot.eat", 1.0, 1.0 + (e.random.next_float() - e.random.next_float()) * 0.2), position));
                let event = if e.random.next_int(10) == 0 {
                    e.owner = Some(player_uuid);
                    e.ordered_to_sit = true;
                    e.persistence_required = true;
                    7
                } else {
                    6
                };
                Some(vec![(id, event)])
            }
            Kind::Parrot if in_tag("minecraft:parrot_poisonous_food") => {
                // A cookie: poison, and a hit that kills it.
                consume(inventory);
                e.effects.insert(crate::effects::EffectInstance::new(crate::effects::MobEffect::Poison, 900, 0));
                e.hurt(f32::MAX);
                Some(Vec::new())
            }
            Kind::Parrot if owned && e.body.on_ground => {
                e.ordered_to_sit = !e.ordered_to_sit;
                Some(Vec::new())
            }
            Kind::Ocelot if !e.trusting && in_tag("minecraft:ocelot_food") && player_feet.is_none_or(|feet| feet.distance_squared(position) < 9.0) => {
                consume(inventory);
                let event = if e.random.next_int(3) == 0 {
                    e.trusting = true;
                    e.persistence_required = true;
                    7
                } else {
                    6
                };
                e.voices.push((Voice::Event("entity.ocelot.ambient", 1.0, 1.0), position));
                Some(vec![(id, event)])
            }
            _ => None,
        }
    }

    /// `Animal.mobInteract` with the held item: its food puts an adult in
    /// love or grows a baby by a tenth (`ageUp`); a golden dandelion locks or
    /// unlocks a baby's age. Returns whether the item was used, and the
    /// events (hearts) to show. An axolotl's bucket of tropical fish leaves
    /// a water bucket.
    pub fn feed_mob(&mut self, id: u64, inventory: &mut minecraftoss_player::inventory::Inventory, hand: usize, infinite: bool) -> Option<(bool, Vec<crate::animal::AnimalEvent>)> {
        let recipes = self.recipes.clone();
        let e = self.mobs.iter_mut().find(|e| e.id == id)?;
        // `Tadpole.mobInteract`: a slime ball hurries its growing up.
        if e.kind == Kind::Tadpole {
            let held = inventory.slots.get(hand).and_then(Option::as_ref).map(|s| s.id.clone());
            if held.as_deref() != Some("minecraft:slime_ball") {
                return None;
            }
            if !infinite {
                if let Some(stack) = inventory.slots[hand].as_mut() {
                    stack.count -= 1;
                    if stack.count == 0 {
                        inventory.slots[hand] = None;
                    }
                }
            }
            e.feed_tadpole();
            return Some((true, Vec::new()));
        }
        let tag = e.species().breeding_food()?;
        // A wild cat or a wary ocelot does not breed (`TamableAnimal.canMate`,
        // `Ocelot.isTrusting`).
        if (e.kind == Kind::Cat && (e.owner.is_none() || e.ordered_to_sit)) || (e.kind == Kind::Ocelot && !e.trusting) {
            return None;
        }
        let is_food = |item: &str| recipes.as_ref().is_some_and(|r| r.item_in_tag(tag, item));
        let held = inventory.slots.get(hand).and_then(Option::as_ref).map(|s| s.id.clone());
        let was_baby = e.baby();
        let (result, events) = crate::animal::interact(&mut e.age, &mut e.in_love, &mut e.persistence_required, inventory, hand, infinite, is_food, false);
        let used = result != crate::cow::InteractionResult::Pass;
        if used && was_baby && !e.baby() {
            let (w, h) = e.species().dimensions(false);
            e.body.width = w;
            e.body.height = h;
        }
        if used && !infinite && e.kind == Kind::Axolotl && held.as_deref() == Some("minecraft:tropical_fish_bucket") && inventory.slots[hand].is_none() {
            inventory.slots[hand] = Some(minecraftoss_player::inventory::ItemStack::new("minecraft:water_bucket", 1));
        }
        Some((used, events))
    }

    /// `Mob.doHurtTarget` for a generic mob, with its kind's extra.
    pub(super) fn mob_melee(&mut self, id: u64, kind: Kind, target: Target, from: DVec3) {
        let s = kind.species();
        let difficulty = self.difficulty;
        // The base attack plus the held weapon's (`ATTACK_DAMAGE` with the
        // main hand's modifiers).
        let weapon = self.mobs.iter().find(|e| e.id == id).and_then(|e| e.hand.as_deref()).map_or(0.0, weapon_damage);
        // `Phantom.updatePhantomSizeInfo`: 6 and its size.
        let size = self.mobs.iter().find(|e| e.id == id).map_or(0, |e| e.phantom_size());
        let base = if kind == Kind::Phantom { 6.0 } else { s.attack };
        let mut damage = base + weapon + size as f32;
        let mut lift = 0.0_f32;
        // `Ravager.doHurtTarget`: it stops to bite.
        if kind == Kind::Ravager {
            if let Some(e) = self.mob_mut(id) {
                e.ravager.attack = 10;
            }
        }
        // `HoglinBase.hurtAndThrowTarget`: half its attack plus up to all
        // of it for an adult, tossing the target up.
        if matches!(kind, Kind::Hoglin | Kind::Zoglin) {
            let baby = self.mobs.iter().find(|e| e.id == id).is_some_and(|e| e.baby());
            if let Some(e) = self.mob_mut(id) {
                damage = if baby { 0.5 } else { s.attack / 2.0 + e.random.next_int(s.attack as u32) as f32 };
            }
            lift = if baby { 0.0 } else { 0.5 };
        }
        let effect = match kind {
            // `CaveSpider.doHurtTarget`: poison, 7 seconds on normal, 15 on hard.
            Kind::CaveSpider => match difficulty {
                2 => Some((EffectKind::Poison, 140)),
                3 => Some((EffectKind::Poison, 300)),
                _ => None,
            },
            // `WitherSkeleton.doHurtTarget`: ten seconds of wither.
            Kind::WitherSkeleton => Some((EffectKind::Wither, 200)),
            // `Bee.doHurtTarget`: poison, 10 seconds on normal, 18 on hard.
            Kind::Bee => match difficulty {
                2 => Some((EffectKind::Poison, 200)),
                3 => Some((EffectKind::Poison, 360)),
                _ => None,
            },
            _ => None,
        };
        let yaw = self.mobs.iter().find(|e| e.id == id).map_or(0.0, |e| e.yaw);
        let weapon = MeleeWeapon { knockback: s.attack_knockback as f32, yaw, ..MeleeWeapon::default() };
        let sound = match kind {
            Kind::Hoglin => Some("entity.hoglin.attack"),
            Kind::Zoglin => Some("entity.zoglin.attack"),
            Kind::Ravager => Some("entity.ravager.attack"),
            // `levelEvent(1039)`: the phantom's bite.
            Kind::Phantom => Some("entity.phantom.bite"),
            Kind::Warden => Some("entity.warden.attack_impact"),
            Kind::Bee => Some("entity.bee.sting"),
            Kind::Creaking => Some("entity.creaking.attack"),
            Kind::Goat => Some("entity.goat.ram_impact"),
            _ => None,
        };
        if let (Some(sound), Some(e)) = (sound, self.mob_mut(id)) {
            let position = e.position();
            e.voices.push((Voice::Event(sound, 1.0, 1.0), position));
            e.bee_stung();
        }
        match target {
            Target::Player(player_id) => {
                self.player_hits.push(PlayerHit { player_id, damage, kind: PlayerHitKind::Melee { attacker: from, hunger_ticks: 0, lift, weapon, effect }, source: Some(id) });
            }
            Target::Mob(victim) | Target::Villager(victim) => {
                self.mob_hits_mob(id, victim, damage, from, f64::from(lift));
            }
        }
    }

    /// A generic shooter's projectile at its target.
    fn mob_shoot(&mut self, id: u64, kind: Kind, target: Target, players: &[PlayerCandidate]) {
        let s = kind.species();
        let Some((target_feet, target_height)) = self.target_box(target, players) else { return };
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        let (feet, eye, yaw) = (e.body.position, f64::from(e.eye_height()), e.yaw);
        let height = f64::from(e.body.height);
        let aim = target_feet + DVec3::new(0.0, target_height * 0.5, 0.0);
        let difficulty = self.difficulty;
        let mut sound: Option<(&'static str, f32)> = None;
        match s.attack_kind {
            AttackKind::SmallFireball => {
                // `Blaze.BlazeAttackGoal`: from its middle half a block up,
                // spread by the distance's root.
                let start = feet + DVec3::new(0.0, height / 2.0 + 0.5, 0.0);
                let spread = (feet.distance(target_feet)).sqrt().sqrt() * 0.5;
                let e = self.mob_mut(id).unwrap();
                let jitter = |r: &mut LegacyRandom| r.next_gaussian() * 2.297 * spread * 0.5;
                let (jx, jz) = (jitter(&mut e.random), jitter(&mut e.random));
                let direction = DVec3::new(aim.x - start.x + jx, aim.y - start.y, aim.z - start.z + jz);
                self.launch_missile(MissileKind::SmallFireball, Some(id), start, direction, 0.1, None);
                sound = Some(("entity.blaze.shoot", 1.0));
            }
            AttackKind::LargeFireball => {
                // `Ghast.GhastShootFireballGoal`: four blocks ahead along its look.
                let look = DVec3::new(-f64::from(yaw.to_radians().sin()), 0.0, f64::from(yaw.to_radians().cos()));
                let start = feet + DVec3::new(0.0, height / 2.0 + 0.5, 0.0) + look * 4.0;
                self.launch_missile(MissileKind::LargeFireball, Some(id), start, aim - start, 0.1, None);
                sound = Some(("entity.ghast.shoot", 10.0));
            }
            AttackKind::WitherSkull => {
                let start = feet + DVec3::new(0.0, 3.0, 0.0);
                self.launch_missile(MissileKind::WitherSkull, Some(id), start, aim - start, 0.1, None);
                sound = Some(("entity.wither.shoot", 1.0));
            }
            AttackKind::ShulkerBullet => {
                let start = feet + DVec3::new(0.0, 0.5, 0.0);
                self.launch_missile(MissileKind::ShulkerBullet, Some(id), start, aim - start, 0.15, Some(target));
                sound = Some(("entity.shulker.shoot", 2.0));
            }
            AttackKind::Spit => {
                // `Llama.spit`: from its mouth, aimed a little high.
                let start = feet + DVec3::new(0.0, eye - 0.1, 0.0);
                let d = aim - start;
                let horizontal = (d.x * d.x + d.z * d.z).sqrt() * 0.2;
                self.launch_missile(MissileKind::LlamaSpit, Some(id), start, DVec3::new(d.x, d.y + horizontal, d.z), 1.5, None);
                if let Some(e) = self.mob_mut(id) {
                    e.llama.did_spit = true;
                }
                sound = Some(("entity.llama.spit", 1.0));
            }
            AttackKind::WindCharge => {
                // `Shoot.tick`: from `getFiringYPosition` to 0.3 of the
                // target's height.
                let start = feet + DVec3::new(0.0, height / 2.0 + f64::from(0.3_f32), 0.0);
                let aim = target_feet + DVec3::new(0.0, target_height * 0.3, 0.0);
                let direction = (aim - start).normalize_or_zero();
                self.add_wind_charge(start, direction * 0.7);
                sound = Some(("entity.breeze.shoot", 1.5));
            }
            AttackKind::Arrow => {
                let e = self.mob_mut(id).unwrap();
                let uncertainty = (14 - difficulty * 4) as f32;
                let arrow = crate::projectile::Arrow::skeleton_shot(feet, eye as f32, target_feet, target_height as f32, uncertainty, &mut e.random);
                self.spawn_owned_arrow(id, arrow);
                sound = Some((if kind == Kind::Pillager { "item.crossbow.shoot" } else { "entity.illusioner.cast_spell" }, 1.0));
            }
            AttackKind::Snowball => {
                // `SnowGolem.performRangedAttack`: aimed a little high, at 1.6 with 12 of spread.
                let start = feet + DVec3::new(0.0, eye - 0.1, 0.0);
                let d = aim - start;
                let horizontal = (d.x * d.x + d.z * d.z).sqrt() * 0.2;
                let seed = self.projectile_seed_random.next_long();
                let mut random = LegacyRandom::new(seed);
                let mut thrown = crate::potion::ThrownPotion::shoot(minecraftoss_player::effects::PotionContents::default(), id, start, DVec3::new(d.x, d.y + horizontal, d.z), 1.6, 12.0, &mut random);
                thrown.item = crate::potion::ThrownItem::Snowball;
                self.spawn_potion(thrown);
                sound = Some(("entity.snow_golem.shoot", 1.0));
            }
            AttackKind::Beam => {
                // `Guardian.GuardianAttackGoal`: magic for 1 (2 more on hard,
                // 2 more for an elder) and its attack.
                let damage = s.attack + if difficulty >= 3 { 2.0 } else { 0.0 } + if kind == Kind::ElderGuardian { 2.0 } else { 0.0 };
                self.direct_hit(id, target, damage, "minecraft:indirect_magic", None, feet);
                sound = Some(("entity.guardian.attack", 1.0));
            }
            // The evoker casts its fangs as a spell (`illagers.rs`).
            AttackKind::Fangs => {}
            AttackKind::Melee | AttackKind::None => {}
        }
        if let (Some((event, volume)), Some(e)) = (sound, self.mob_mut(id)) {
            let pitch = (e.random.next_float() - e.random.next_float()) * 0.2 + 1.0;
            let position = e.position();
            e.voices.push((Voice::Event(event, volume, pitch), position));
        }
    }

    /// A hit with no projectile in between (the guardian's beam, fangs).
    pub(super) fn direct_hit(&mut self, id: u64, target: Target, damage: f32, damage_type: &'static str, effect: Option<(EffectKind, u32)>, from: DVec3) {
        match target {
            Target::Player(player_id) => {
                self.player_hits.push(PlayerHit { player_id, damage, kind: PlayerHitKind::Projectile { velocity: DVec3::ZERO, damage_type, effect, fire_seconds: 0.0 }, source: Some(id) });
            }
            Target::Mob(victim) | Target::Villager(victim) => {
                self.mob_hits_mob(id, victim, damage, from, 0.0);
            }
        }
    }
}
