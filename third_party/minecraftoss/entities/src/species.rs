//! The mobs that share the generic mob entity (`world::mobs`): one row per
//! 26.3 entity type with what sets it apart. The sizes, fire immunity and
//! tracking come from `EntityType`'s builder; the numbers from each type's
//! `createAttributes` (as `DefaultAttributes` holds them, read out of the
//! 26.3 jar by `minecraft/tools/DumpEntityTypes.java`); the rest (how it
//! moves, how it takes to players, how it attacks) from its class.

/// `MobCategory`: which spawn cap it counts towards, and how far away it
/// may despawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
    Misc,
}

impl Category {
    /// `MobCategory.getName`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Monster => "monster",
            Self::Creature => "creature",
            Self::Ambient => "ambient",
            Self::Axolotls => "axolotls",
            Self::UndergroundWaterCreature => "underground_water_creature",
            Self::WaterCreature => "water_creature",
            Self::WaterAmbient => "water_ambient",
            Self::Misc => "misc",
        }
    }

    /// `MobCategory.getDespawnDistance` (128, or 64 for water ambients).
    pub fn despawn_distance(self) -> f64 {
        if self == Self::WaterAmbient { 64.0 } else { 128.0 }
    }

    /// `MobCategory.isFriendly`: whether it may live in peaceful.
    pub fn friendly(self) -> bool {
        self != Self::Monster
    }
}

/// How it gets about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// On foot (`GroundPathNavigation`, the plain `MoveControl`).
    Ground,
    /// `FlyingMoveControl` over `FlyingPathNavigation`: no gravity while it
    /// moves, and climbing or sinking towards its goal.
    Fly,
    /// A `FlyingMob` (ghast, phantom) or the dragon: always weightless,
    /// with `travelFlying`'s drag.
    Hover,
    /// In water (`WaterBoundPathNavigation`); stranded out of it.
    Swim,
    /// Walks on land and swims in water (`AmphibiousPathNavigation`).
    Amphibious,
    /// Never moves of its own accord (a shulker on its block).
    Stationary,
}

/// How it takes to players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Temper {
    /// Flees when hurt (`PanicGoal`).
    Passive,
    /// Leaves players be until one hurts it (`HurtByTargetGoal`).
    Neutral,
    /// Hunts players (`NearestAttackableTargetGoal<Player>`).
    Hostile,
    /// Neither flees nor hunts players.
    Inert,
}

/// How it attacks, when it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackKind {
    None,
    /// `MeleeAttackGoal` and `Mob.doHurtTarget`.
    Melee,
    /// `SmallFireball`s in volleys of three (`Blaze.BlazeAttackGoal`).
    SmallFireball,
    /// A `LargeFireball` that bursts (`Ghast.GhastShootFireballGoal`).
    LargeFireball,
    /// A `WitherSkull` from a head.
    WitherSkull,
    /// A `WindCharge` (`BreezeShoot`).
    WindCharge,
    /// An arrow from a bow or crossbow (`RangedBowAttackGoal`,
    /// `RangedCrossbowAttackGoal`).
    Arrow,
    /// A homing `ShulkerBullet`.
    ShulkerBullet,
    /// `LlamaSpit` at whoever hurt it.
    Spit,
    /// Snowballs at monsters (`SnowGolem.performRangedAttack`).
    Snowball,
    /// The guardian's beam (`GuardianAttackGoal`).
    Beam,
    /// `EvokerFangs` and vexes (`SpellcasterIllager`).
    Fangs,
}

/// One kind of mob.
#[derive(Clone, Copy, Debug)]
pub struct Species {
    pub kind: Kind,
    pub id: &'static str,
    /// Its path, which names its sounds and loot table.
    pub name: &'static str,
    pub category: Category,
    /// Width, height and eye height (an adult's).
    pub size: [f32; 3],
    /// `EntityDimensions.fixed`: its size does not scale.
    pub fixed: bool,
    pub health: f32,
    pub speed: f64,
    pub flying_speed: f64,
    pub attack: f32,
    pub attack_knockback: f64,
    pub armor: f32,
    pub follow_range: f64,
    pub knockback_resistance: f64,
    pub step_height: f64,
    pub fire_immune: bool,
    pub motion: Motion,
    pub temper: Temper,
    pub attack_kind: AttackKind,
    /// An `AgeableMob`: babies at half size.
    pub ageable: bool,
    /// `#undead` (smite, healing hurts, harming heals, burns by day).
    pub undead: bool,
    /// `#sensitive_to_bane_of_arthropods`.
    pub arthropod: bool,
    /// `#illager`.
    pub illager: bool,
    /// Breathes water, not air.
    pub aquatic: bool,
    /// `AbstractFish`: flops about on land.
    pub fish: bool,
    /// Climbs walls like a spider.
    pub climber: bool,
    /// A boss, with a boss bar.
    pub boss: bool,
    /// `xpReward`: a fixed reward, -1 for an animal's 1 to 3, -2 for a
    /// water animal's 1 to 3.
    pub xp: i32,
    /// `clientTrackingRange` (chunks) and `updateInterval` (ticks).
    pub tracking: [i32; 2],
}

impl Kind {
    pub fn species(self) -> &'static Species {
        &SPECIES[self as usize]
    }

    /// The kind whose entity type this is.
    pub fn of_id(id: &str) -> Option<Kind> {
        let path = id.strip_prefix("minecraft:").unwrap_or(id);
        SPECIES.iter().find(|s| s.name == path).map(|s| s.kind)
    }
}

impl Species {
    /// Its eye height at a size (a baby's is half).
    pub fn eye_height(&self, baby: bool) -> f32 {
        if baby && !self.fixed { self.size[2] * 0.5 } else { self.size[2] }
    }

    /// Width and height at a size.
    pub fn dimensions(&self, baby: bool) -> (f32, f32) {
        if baby && !self.fixed { (self.size[0] * 0.5, self.size[1] * 0.5) } else { (self.size[0], self.size[1]) }
    }

    /// `Mob.getAmbientSoundInterval`.
    pub fn ambient_interval(&self) -> i32 {
        match self.kind {
            Kind::Turtle | Kind::Sniffer | Kind::Camel | Kind::CamelHusk => 120,
            Kind::Strider | Kind::Ghast | Kind::HappyGhast => 120,
            Kind::Squid | Kind::GlowSquid | Kind::Dolphin | Kind::Nautilus | Kind::ZombieNautilus => 120,
            _ => 80,
        }
    }

    /// The sound family its ambient, hurt and death sounds are named by
    /// (`entity.<family>.ambient`), or none for the silent.
    pub fn sound_family(&self, baby: bool) -> &'static str {
        match self.kind {
            Kind::Mule if baby => "baby_mule",
            Kind::SkeletonHorse if baby => "baby_skeleton_horse",
            Kind::ZombieHorse if baby => "baby_zombie_horse",
            Kind::TraderLlama => "llama",
            Kind::Giant => "zombie",
            Kind::Illusioner => "illusioner",
            _ => self.name,
        }
    }

    /// Whether it makes an ambient sound at all.
    pub fn has_ambient(&self) -> bool {
        // `Giant` overrides none of `Monster`'s silence.
        self.kind != Kind::Giant
    }

    /// `playStepSound`'s own step, where it has one.
    pub fn step_sound(&self) -> Option<&'static str> {
        Some(match self.kind {
            Kind::CaveSpider => "entity.spider.step",
            Kind::Silverfish => "entity.silverfish.step",
            Kind::Endermite => "entity.endermite.step",
            Kind::WitherSkeleton => "entity.wither_skeleton.step",
            Kind::Giant => "entity.zombie.step",
            Kind::ZombifiedPiglin | Kind::Piglin | Kind::PiglinBrute => return Some(match self.kind {
                Kind::Piglin => "entity.piglin.step",
                Kind::PiglinBrute => "entity.piglin_brute.step",
                _ => "entity.zombified_piglin.step",
            }),
            Kind::Hoglin => "entity.hoglin.step",
            Kind::Zoglin => "entity.zoglin.step",
            Kind::Ravager => "entity.ravager.step",
            Kind::PolarBear => "entity.polar_bear.step",
            Kind::Goat => "entity.goat.step",
            Kind::Llama | Kind::TraderLlama => "entity.llama.step",
            Kind::Camel | Kind::CamelHusk => "entity.camel.step",
            Kind::Sniffer => "entity.sniffer.step",
            Kind::Strider => "entity.strider.step",
            Kind::Panda => "entity.panda.step",
            Kind::Fox => return None,
            Kind::Turtle => "entity.turtle.shamble",
            Kind::Frog => "entity.frog.step",
            Kind::Armadillo => "entity.armadillo.step",
            Kind::Warden => "entity.warden.step",
            Kind::Creaking => "entity.creaking.step",
            Kind::CopperGolem => "entity.copper_golem.step",
            Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => "entity.horse.step",
            Kind::Blaze | Kind::Breeze | Kind::Ghast | Kind::HappyGhast | Kind::Phantom | Kind::Vex | Kind::Allay | Kind::Bee | Kind::Parrot | Kind::Wither | Kind::EnderDragon | Kind::Shulker => return None,
            Kind::MagmaCube | Kind::SulfurCube => return None,
            _ if self.motion == Motion::Swim => return None,
            _ => return None,
        })
    }

    /// The category its sounds play in (`getSoundSource`).
    pub fn sound_category(&self) -> &'static str {
        match self.category {
            Category::Monster => "hostile_volume",
            Category::Ambient => "ambient_volume",
            _ if matches!(self.kind, Kind::CopperGolem | Kind::SnowGolem | Kind::Allay) => "friendly_volume",
            _ => "neutral_volume",
        }
    }

    /// `getSoundVolume`.
    pub fn sound_volume(&self) -> f32 {
        match self.kind {
            Kind::Ghast | Kind::HappyGhast => 5.0,
            Kind::EnderDragon => 5.0,
            Kind::Wither | Kind::Warden => 4.0,
            Kind::Squid | Kind::GlowSquid => 0.4,
            Kind::Bee | Kind::Parrot | Kind::Cod | Kind::Salmon | Kind::Pufferfish | Kind::TropicalFish => 0.4,
            Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => 0.8,
            Kind::Llama | Kind::TraderLlama => 0.8,
            _ => 1.0,
        }
    }

    /// The default loot table (`EntityType.getDefaultLootTable`).
    pub fn loot_table(&self) -> Option<&'static str> {
        match self.kind {
            Kind::Giant | Kind::EnderDragon | Kind::Allay | Kind::Vex | Kind::Breeze | Kind::Warden | Kind::Creaking | Kind::Tadpole => (!matches!(self.kind, Kind::Allay | Kind::Vex | Kind::Creaking | Kind::Tadpole | Kind::EnderDragon)).then_some(self.id),
            _ => Some(self.id),
        }
    }

    /// The items that tempt it (`TemptGoal`'s, usually its `isFood` tag).
    pub fn tempt_tag(&self) -> Option<&'static str> {
        Some(match self.kind {
            Kind::Armadillo => "minecraft:armadillo_food",
            Kind::Axolotl => "minecraft:axolotl_food",
            Kind::Bee => "minecraft:bee_food",
            Kind::Camel | Kind::CamelHusk => "minecraft:camel_food",
            Kind::Cat => "minecraft:cat_food",
            Kind::Fox => "minecraft:fox_food",
            Kind::Frog => "minecraft:frog_food",
            Kind::Goat => "minecraft:goat_food",
            Kind::Hoglin => "minecraft:hoglin_food",
            Kind::Llama | Kind::TraderLlama => "minecraft:llama_tempt_items",
            Kind::Ocelot => "minecraft:ocelot_food",
            Kind::Panda => "minecraft:panda_food",
            Kind::Parrot => "minecraft:parrot_food",
            Kind::Rabbit => "minecraft:rabbit_food",
            Kind::Sniffer => "minecraft:sniffer_food",
            Kind::Strider => "minecraft:strider_tempt_items",
            Kind::Turtle => "minecraft:turtle_food",
            Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => "minecraft:horse_tempt_items",
            Kind::HappyGhast => "minecraft:happy_ghast_tempt_items",
            Kind::Nautilus => "minecraft:nautilus_taming_items",
            _ => return None,
        })
    }

    /// `Animal.isFood`: what puts it in love and grows its babies; none for
    /// those that never breed (mules, the undead horses, polar bears,
    /// parrots, the sea creatures but axolotls and turtles).
    pub fn breeding_food(&self) -> Option<&'static str> {
        Some(match self.kind {
            Kind::Armadillo => "minecraft:armadillo_food",
            Kind::Axolotl => "minecraft:axolotl_food",
            Kind::Bee => "minecraft:bee_food",
            Kind::Camel => "minecraft:camel_food",
            Kind::Cat => "minecraft:cat_food",
            Kind::Fox => "minecraft:fox_food",
            Kind::Frog => "minecraft:frog_food",
            Kind::Goat => "minecraft:goat_food",
            Kind::Hoglin => "minecraft:hoglin_food",
            Kind::Llama | Kind::TraderLlama => "minecraft:llama_food",
            Kind::Ocelot => "minecraft:ocelot_food",
            Kind::Panda => "minecraft:panda_food",
            Kind::Rabbit => "minecraft:rabbit_food",
            Kind::Sniffer => "minecraft:sniffer_food",
            Kind::Strider => "minecraft:strider_food",
            Kind::Turtle => "minecraft:turtle_food",
            _ => return None,
        })
    }

    /// Damage after its armor (`CombatRules.getDamageAfterAbsorb`, no
    /// toughness).
    pub fn after_armor(&self, damage: f32) -> f32 {
        if self.armor <= 0.0 {
            return damage;
        }
        crate::health::damage_after_armor(damage, self.armor, 0.0)
    }
}

pub const ALL: [Kind; 66] = [Kind::Allay, Kind::Armadillo, Kind::Axolotl, Kind::Bee, Kind::Blaze, Kind::Breeze, Kind::Camel, Kind::CamelHusk, Kind::Cat, Kind::CaveSpider, Kind::Cod, Kind::CopperGolem, Kind::Creaking, Kind::Dolphin, Kind::ElderGuardian, Kind::Endermite, Kind::EnderDragon, Kind::Evoker, Kind::Fox, Kind::Frog, Kind::Ghast, Kind::HappyGhast, Kind::Giant, Kind::GlowSquid, Kind::Goat, Kind::Guardian, Kind::Hoglin, Kind::Illusioner, Kind::Llama, Kind::MagmaCube, Kind::Mule, Kind::Nautilus, Kind::Ocelot, Kind::Panda, Kind::Parrot, Kind::Phantom, Kind::Piglin, Kind::PiglinBrute, Kind::Pillager, Kind::PolarBear, Kind::Pufferfish, Kind::Rabbit, Kind::Ravager, Kind::Salmon, Kind::Shulker, Kind::Silverfish, Kind::SkeletonHorse, Kind::Sniffer, Kind::SnowGolem, Kind::Squid, Kind::Strider, Kind::SulfurCube, Kind::Tadpole, Kind::TraderLlama, Kind::TropicalFish, Kind::Turtle, Kind::Vex, Kind::Vindicator, Kind::WanderingTrader, Kind::Warden, Kind::Wither, Kind::WitherSkeleton, Kind::Zoglin, Kind::ZombieHorse, Kind::ZombieNautilus, Kind::ZombifiedPiglin];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Allay,
    Armadillo,
    Axolotl,
    Bee,
    Blaze,
    Breeze,
    Camel,
    CamelHusk,
    Cat,
    CaveSpider,
    Cod,
    CopperGolem,
    Creaking,
    Dolphin,
    ElderGuardian,
    Endermite,
    EnderDragon,
    Evoker,
    Fox,
    Frog,
    Ghast,
    HappyGhast,
    Giant,
    GlowSquid,
    Goat,
    Guardian,
    Hoglin,
    Illusioner,
    Llama,
    MagmaCube,
    Mule,
    Nautilus,
    Ocelot,
    Panda,
    Parrot,
    Phantom,
    Piglin,
    PiglinBrute,
    Pillager,
    PolarBear,
    Pufferfish,
    Rabbit,
    Ravager,
    Salmon,
    Shulker,
    Silverfish,
    SkeletonHorse,
    Sniffer,
    SnowGolem,
    Squid,
    Strider,
    SulfurCube,
    Tadpole,
    TraderLlama,
    TropicalFish,
    Turtle,
    Vex,
    Vindicator,
    WanderingTrader,
    Warden,
    Wither,
    WitherSkeleton,
    Zoglin,
    ZombieHorse,
    ZombieNautilus,
    ZombifiedPiglin,
}

pub static SPECIES: [Species; 66] = [
    Species { kind: Kind::Allay, id: "minecraft:allay", name: "allay", category: Category::Creature, size: [0.35_f32, 0.6, 0.36], fixed: false, health: 20.0, speed: 0.10000000149011612, flying_speed: 0.10000000149011612, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Fly, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [8, 2] },
    Species { kind: Kind::Armadillo, id: "minecraft:armadillo", name: "armadillo", category: Category::Creature, size: [0.7_f32, 0.65, 0.26], fixed: false, health: 12.0, speed: 0.14, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Axolotl, id: "minecraft:axolotl", name: "axolotl", category: Category::Axolotls, size: [0.75_f32, 0.42, 0.2751], fixed: false, health: 14.0, speed: 1.0, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Amphibious, temper: Temper::Neutral, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Bee, id: "minecraft:bee", name: "bee", category: Category::Creature, size: [0.55_f32, 0.5, 0.3], fixed: false, health: 10.0, speed: 0.30000001192092896, flying_speed: 0.6000000238418579, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Fly, temper: Temper::Neutral, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: true, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [8, 3] },
    Species { kind: Kind::Blaze, id: "minecraft:blaze", name: "blaze", category: Category::Monster, size: [0.6_f32, 1.8, 1.53], fixed: false, health: 20.0, speed: 0.23000000417232513, flying_speed: 0.0, attack: 6.0, attack_knockback: 0.0, armor: 0.0, follow_range: 48.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::SmallFireball, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 10, tracking: [8, 3] },
    Species { kind: Kind::Breeze, id: "minecraft:breeze", name: "breeze", category: Category::Monster, size: [0.6_f32, 1.77, 1.3452], fixed: false, health: 30.0, speed: 0.6299999952316284, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 24.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::WindCharge, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 10, tracking: [10, 3] },
    Species { kind: Kind::Camel, id: "minecraft:camel", name: "camel", category: Category::Creature, size: [1.7_f32, 2.375, 2.275], fixed: false, health: 32.0, speed: 0.09000000357627869, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.5, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::CamelHusk, id: "minecraft:camel_husk", name: "camel_husk", category: Category::Monster, size: [1.7_f32, 2.375, 2.275], fixed: false, health: 32.0, speed: 0.09000000357627869, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.5, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [10, 3] },
    Species { kind: Kind::Cat, id: "minecraft:cat", name: "cat", category: Category::Creature, size: [0.6_f32, 0.7, 0.35], fixed: false, health: 10.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [8, 3] },
    Species { kind: Kind::CaveSpider, id: "minecraft:cave_spider", name: "cave_spider", category: Category::Monster, size: [0.7_f32, 0.5, 0.45], fixed: false, health: 12.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: true, illager: false, aquatic: false, fish: false, climber: true, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::Cod, id: "minecraft:cod", name: "cod", category: Category::WaterAmbient, size: [0.5_f32, 0.3, 0.195], fixed: false, health: 3.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: true, climber: false, boss: false, xp: -2, tracking: [4, 3] },
    Species { kind: Kind::CopperGolem, id: "minecraft:copper_golem", name: "copper_golem", category: Category::Misc, size: [0.49_f32, 0.98, 0.8125], fixed: false, health: 12.0, speed: 0.20000000298023224, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [10, 3] },
    Species { kind: Kind::Creaking, id: "minecraft:creaking", name: "creaking", category: Category::Monster, size: [0.9_f32, 2.7, 2.3], fixed: false, health: 1.0, speed: 0.4000000059604645, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 32.0, knockback_resistance: 0.0, step_height: 1.0625, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [8, 3] },
    Species { kind: Kind::Dolphin, id: "minecraft:dolphin", name: "dolphin", category: Category::WaterCreature, size: [0.9_f32, 0.6, 0.3], fixed: false, health: 10.0, speed: 1.2000000476837158, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Neutral, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: -2, tracking: [5, 3] },
    Species { kind: Kind::ElderGuardian, id: "minecraft:elder_guardian", name: "elder_guardian", category: Category::Monster, size: [1.9975_f32, 1.9975, 0.99875], fixed: false, health: 80.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 8.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Hostile, attack_kind: AttackKind::Beam, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: 10, tracking: [10, 3] },
    Species { kind: Kind::Endermite, id: "minecraft:endermite", name: "endermite", category: Category::Monster, size: [0.4_f32, 0.3, 0.13], fixed: false, health: 8.0, speed: 0.25, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: true, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 3, tracking: [8, 3] },
    Species { kind: Kind::EnderDragon, id: "minecraft:ender_dragon", name: "ender_dragon", category: Category::Monster, size: [16.0_f32, 8.0, 6.8], fixed: false, health: 200.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Hover, temper: Temper::Hostile, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: true, xp: 12000, tracking: [10, 3] },
    Species { kind: Kind::Evoker, id: "minecraft:evoker", name: "evoker", category: Category::Monster, size: [0.6_f32, 1.95, 1.6575], fixed: false, health: 24.0, speed: 0.5, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 12.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Fangs, ageable: false, undead: false, arthropod: false, illager: true, aquatic: false, fish: false, climber: false, boss: false, xp: 10, tracking: [8, 3] },
    Species { kind: Kind::Fox, id: "minecraft:fox", name: "fox", category: Category::Creature, size: [0.6_f32, 0.7, 0.4], fixed: false, health: 10.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 32.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [8, 3] },
    Species { kind: Kind::Frog, id: "minecraft:frog", name: "frog", category: Category::Creature, size: [0.5_f32, 0.5, 0.425], fixed: false, health: 10.0, speed: 1.0, flying_speed: 0.0, attack: 10.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Amphibious, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Ghast, id: "minecraft:ghast", name: "ghast", category: Category::Monster, size: [4.0_f32, 4.0, 2.6], fixed: false, health: 10.0, speed: 0.7, flying_speed: 0.06, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 100.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Hover, temper: Temper::Hostile, attack_kind: AttackKind::LargeFireball, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [10, 3] },
    Species { kind: Kind::HappyGhast, id: "minecraft:happy_ghast", name: "happy_ghast", category: Category::Creature, size: [4.0_f32, 4.0, 2.6], fixed: false, health: 20.0, speed: 0.05, flying_speed: 0.05, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Hover, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [10, 3] },
    Species { kind: Kind::Giant, id: "minecraft:giant", name: "giant", category: Category::Monster, size: [3.6_f32, 12.0, 10.44], fixed: false, health: 100.0, speed: 0.5, flying_speed: 0.0, attack: 50.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Inert, attack_kind: AttackKind::None, ageable: false, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [10, 3] },
    Species { kind: Kind::GlowSquid, id: "minecraft:glow_squid", name: "glow_squid", category: Category::UndergroundWaterCreature, size: [0.8_f32, 0.8, 0.4], fixed: false, health: 10.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: -2, tracking: [10, 3] },
    Species { kind: Kind::Goat, id: "minecraft:goat", name: "goat", category: Category::Creature, size: [0.9_f32, 1.3, 1.105], fixed: false, health: 10.0, speed: 0.20000000298023224, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Guardian, id: "minecraft:guardian", name: "guardian", category: Category::Monster, size: [0.85_f32, 0.85, 0.425], fixed: false, health: 30.0, speed: 0.5, flying_speed: 0.0, attack: 6.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Hostile, attack_kind: AttackKind::Beam, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: 10, tracking: [8, 3] },
    Species { kind: Kind::Hoglin, id: "minecraft:hoglin", name: "hoglin", category: Category::Monster, size: [1.3964844_f32, 1.4, 1.19], fixed: false, health: 40.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 6.0, attack_knockback: 1.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.6000000238418579, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::Illusioner, id: "minecraft:illusioner", name: "illusioner", category: Category::Monster, size: [0.6_f32, 1.95, 1.6575], fixed: false, health: 32.0, speed: 0.5, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 18.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Arrow, ageable: false, undead: false, arthropod: false, illager: true, aquatic: false, fish: false, climber: false, boss: false, xp: 10, tracking: [8, 3] },
    Species { kind: Kind::Llama, id: "minecraft:llama", name: "llama", category: Category::Creature, size: [0.9_f32, 1.87, 1.7765], fixed: false, health: 53.0, speed: 0.17499999701976776, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::Spit, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::MagmaCube, id: "minecraft:magma_cube", name: "magma_cube", category: Category::Monster, size: [0.52_f32, 0.52, 0.325], fixed: false, health: 20.0, speed: 0.20000000298023224, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [8, 3] },
    Species { kind: Kind::Mule, id: "minecraft:mule", name: "mule", category: Category::Creature, size: [1.3964844_f32, 1.6, 1.52], fixed: false, health: 53.0, speed: 0.17499999701976776, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [8, 3] },
    Species { kind: Kind::Nautilus, id: "minecraft:nautilus", name: "nautilus", category: Category::WaterCreature, size: [0.875_f32, 0.95, 0.2751], fixed: false, health: 15.0, speed: 1.0, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.30000001192092896, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Neutral, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: -2, tracking: [10, 3] },
    Species { kind: Kind::Ocelot, id: "minecraft:ocelot", name: "ocelot", category: Category::Creature, size: [0.6_f32, 0.7, 0.595], fixed: false, health: 10.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Panda, id: "minecraft:panda", name: "panda", category: Category::Creature, size: [1.3_f32, 1.25, 1.0625], fixed: false, health: 20.0, speed: 0.15000000596046448, flying_speed: 0.0, attack: 6.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Parrot, id: "minecraft:parrot", name: "parrot", category: Category::Creature, size: [0.5_f32, 0.9, 0.54], fixed: false, health: 6.0, speed: 0.20000000298023224, flying_speed: 0.4000000059604645, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Fly, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [8, 3] },
    Species { kind: Kind::Phantom, id: "minecraft:phantom", name: "phantom", category: Category::Monster, size: [0.9_f32, 0.5, 0.175], fixed: false, health: 20.0, speed: 0.7, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Hover, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::Piglin, id: "minecraft:piglin", name: "piglin", category: Category::Monster, size: [0.6_f32, 1.95, 1.79], fixed: false, health: 16.0, speed: 0.3499999940395355, flying_speed: 0.0, attack: 5.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::PiglinBrute, id: "minecraft:piglin_brute", name: "piglin_brute", category: Category::Monster, size: [0.6_f32, 1.95, 1.79], fixed: false, health: 50.0, speed: 0.3499999940395355, flying_speed: 0.0, attack: 7.0, attack_knockback: 0.0, armor: 0.0, follow_range: 12.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 20, tracking: [8, 3] },
    Species { kind: Kind::Pillager, id: "minecraft:pillager", name: "pillager", category: Category::Monster, size: [0.6_f32, 1.95, 1.6575], fixed: false, health: 24.0, speed: 0.3499999940395355, flying_speed: 0.0, attack: 5.0, attack_knockback: 0.0, armor: 0.0, follow_range: 32.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Arrow, ageable: false, undead: false, arthropod: false, illager: true, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::PolarBear, id: "minecraft:polar_bear", name: "polar_bear", category: Category::Creature, size: [1.4_f32, 1.4, 1.19], fixed: false, health: 30.0, speed: 0.25, flying_speed: 0.0, attack: 6.0, attack_knockback: 0.0, armor: 0.0, follow_range: 20.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Neutral, attack_kind: AttackKind::Melee, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Pufferfish, id: "minecraft:pufferfish", name: "pufferfish", category: Category::WaterAmbient, size: [0.7_f32, 0.7, 0.455], fixed: false, health: 3.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: true, climber: false, boss: false, xp: -2, tracking: [4, 3] },
    Species { kind: Kind::Rabbit, id: "minecraft:rabbit", name: "rabbit", category: Category::Creature, size: [0.49_f32, 0.6, 0.59], fixed: false, health: 3.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [8, 3] },
    Species { kind: Kind::Ravager, id: "minecraft:ravager", name: "ravager", category: Category::Monster, size: [1.95_f32, 2.2, 1.8700001], fixed: false, health: 100.0, speed: 0.3, flying_speed: 0.0, attack: 12.0, attack_knockback: 1.5, armor: 0.0, follow_range: 32.0, knockback_resistance: 0.75, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 20, tracking: [10, 3] },
    Species { kind: Kind::Salmon, id: "minecraft:salmon", name: "salmon", category: Category::WaterAmbient, size: [0.7_f32, 0.4, 0.26], fixed: false, health: 3.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: true, climber: false, boss: false, xp: -2, tracking: [4, 3] },
    Species { kind: Kind::Shulker, id: "minecraft:shulker", name: "shulker", category: Category::Monster, size: [1.0_f32, 1.0, 0.5], fixed: false, health: 30.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Stationary, temper: Temper::Hostile, attack_kind: AttackKind::ShulkerBullet, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [10, 3] },
    Species { kind: Kind::Silverfish, id: "minecraft:silverfish", name: "silverfish", category: Category::Monster, size: [0.4_f32, 0.3, 0.13], fixed: false, health: 8.0, speed: 0.25, flying_speed: 0.0, attack: 1.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: true, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::SkeletonHorse, id: "minecraft:skeleton_horse", name: "skeleton_horse", category: Category::Creature, size: [1.3964844_f32, 1.6, 1.52], fixed: false, health: 15.0, speed: 0.20000000298023224, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Sniffer, id: "minecraft:sniffer", name: "sniffer", category: Category::Creature, size: [1.9_f32, 1.75, 1.05], fixed: false, health: 14.0, speed: 0.10000000149011612, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::SnowGolem, id: "minecraft:snow_golem", name: "snow_golem", category: Category::Misc, size: [0.7_f32, 1.9, 1.7], fixed: false, health: 4.0, speed: 0.20000000298023224, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Inert, attack_kind: AttackKind::Snowball, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [8, 3] },
    Species { kind: Kind::Squid, id: "minecraft:squid", name: "squid", category: Category::WaterCreature, size: [0.8_f32, 0.8, 0.4], fixed: false, health: 10.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: -2, tracking: [8, 3] },
    Species { kind: Kind::Strider, id: "minecraft:strider", name: "strider", category: Category::Creature, size: [0.9_f32, 1.7, 1.445], fixed: false, health: 20.0, speed: 0.17499999701976776, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::SulfurCube, id: "minecraft:sulfur_cube", name: "sulfur_cube", category: Category::Monster, size: [0.49_f32, 0.49, 0.175], fixed: false, health: 20.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [10, 3] },
    Species { kind: Kind::Tadpole, id: "minecraft:tadpole", name: "tadpole", category: Category::Creature, size: [0.4_f32, 0.3, 0.19500001], fixed: false, health: 6.0, speed: 1.0, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: true, climber: false, boss: false, xp: -2, tracking: [10, 3] },
    Species { kind: Kind::TraderLlama, id: "minecraft:trader_llama", name: "trader_llama", category: Category::Creature, size: [0.9_f32, 1.87, 1.7765], fixed: false, health: 53.0, speed: 0.17499999701976776, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::Spit, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::TropicalFish, id: "minecraft:tropical_fish", name: "tropical_fish", category: Category::WaterAmbient, size: [0.5_f32, 0.4, 0.26], fixed: false, health: 3.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: true, fish: true, climber: false, boss: false, xp: -2, tracking: [4, 3] },
    Species { kind: Kind::Turtle, id: "minecraft:turtle", name: "turtle", category: Category::Creature, size: [1.2_f32, 0.4, 0.34], fixed: false, health: 30.0, speed: 0.25, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Amphibious, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::Vex, id: "minecraft:vex", name: "vex", category: Category::Monster, size: [0.4_f32, 0.8, 0.51875], fixed: false, health: 14.0, speed: 0.7, flying_speed: 0.0, attack: 4.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Fly, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 3, tracking: [8, 3] },
    Species { kind: Kind::Vindicator, id: "minecraft:vindicator", name: "vindicator", category: Category::Monster, size: [0.6_f32, 1.95, 1.6575], fixed: false, health: 24.0, speed: 0.3499999940395355, flying_speed: 0.0, attack: 5.0, attack_knockback: 0.0, armor: 0.0, follow_range: 12.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: true, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::WanderingTrader, id: "minecraft:wandering_trader", name: "wandering_trader", category: Category::Creature, size: [0.6_f32, 1.95, 1.62], fixed: false, health: 20.0, speed: 0.7, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 0, tracking: [10, 3] },
    Species { kind: Kind::Warden, id: "minecraft:warden", name: "warden", category: Category::Monster, size: [0.9_f32, 2.9, 2.4650002], fixed: false, health: 500.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 30.0, attack_knockback: 1.5, armor: 0.0, follow_range: 24.0, knockback_resistance: 1.0, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: false, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [16, 3] },
    Species { kind: Kind::Wither, id: "minecraft:wither", name: "wither", category: Category::Monster, size: [0.9_f32, 3.5, 2.9750001], fixed: false, health: 300.0, speed: 0.6000000238418579, flying_speed: 0.6000000238418579, attack: 2.0, attack_knockback: 0.0, armor: 4.0, follow_range: 40.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Hover, temper: Temper::Hostile, attack_kind: AttackKind::WitherSkull, ageable: false, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: true, xp: 50, tracking: [10, 3] },
    Species { kind: Kind::WitherSkeleton, id: "minecraft:wither_skeleton", name: "wither_skeleton", category: Category::Monster, size: [0.7_f32, 2.4, 2.1], fixed: false, health: 20.0, speed: 0.25, flying_speed: 0.0, attack: 2.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::Zoglin, id: "minecraft:zoglin", name: "zoglin", category: Category::Monster, size: [1.3964844_f32, 1.4, 1.19], fixed: false, health: 40.0, speed: 0.30000001192092896, flying_speed: 0.0, attack: 6.0, attack_knockback: 1.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.6000000238418579, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: true, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
    Species { kind: Kind::ZombieHorse, id: "minecraft:zombie_horse", name: "zombie_horse", category: Category::Monster, size: [1.3964844_f32, 1.6, 1.52], fixed: false, health: 25.0, speed: 0.22499999403953552, flying_speed: 0.0, attack: 0.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.0, step_height: 1.0, fire_immune: false, motion: Motion::Ground, temper: Temper::Passive, attack_kind: AttackKind::None, ageable: true, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: -1, tracking: [10, 3] },
    Species { kind: Kind::ZombieNautilus, id: "minecraft:zombie_nautilus", name: "zombie_nautilus", category: Category::Monster, size: [0.875_f32, 0.95, 0.2751], fixed: false, health: 15.0, speed: 1.100000023841858, flying_speed: 0.0, attack: 3.0, attack_knockback: 0.0, armor: 0.0, follow_range: 16.0, knockback_resistance: 0.30000001192092896, step_height: 0.6, fire_immune: false, motion: Motion::Swim, temper: Temper::Hostile, attack_kind: AttackKind::Melee, ageable: false, undead: true, arthropod: false, illager: false, aquatic: true, fish: false, climber: false, boss: false, xp: 5, tracking: [10, 3] },
    Species { kind: Kind::ZombifiedPiglin, id: "minecraft:zombified_piglin", name: "zombified_piglin", category: Category::Monster, size: [0.6_f32, 1.95, 1.79], fixed: false, health: 20.0, speed: 0.23000000417232513, flying_speed: 0.0, attack: 5.0, attack_knockback: 0.0, armor: 2.0, follow_range: 35.0, knockback_resistance: 0.0, step_height: 0.6, fire_immune: true, motion: Motion::Ground, temper: Temper::Neutral, attack_kind: AttackKind::Melee, ageable: true, undead: true, arthropod: false, illager: false, aquatic: false, fish: false, climber: false, boss: false, xp: 5, tracking: [8, 3] },
];
