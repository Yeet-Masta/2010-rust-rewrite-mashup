//! The particle types: each one's provider (`ParticleResources`'
//! registrations) and the behaviour of its vanilla class, as 26.3 has them.
use glam::DVec3;
use minecraft_terrain::scene::Block;

use super::{FULL_BRIGHT, Layer, Light, Particle, Particles, Random};
use crate::world::World;

/// The particle types this game spawns, by `ParticleTypes` name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    AngryVillager,
    Ash,
    Block,
    BlockCrumble,
    Bubble,
    BubblePop,
    CampfireCosySmoke,
    CampfireSignalSmoke,
    CherryLeaves,
    Cloud,
    Composter,
    CopperFireFlame,
    Crit,
    CrimsonSpore,
    DamageIndicator,
    Dolphin,
    DrippingDripstoneLava,
    DrippingDripstoneWater,
    DrippingHoney,
    DrippingLava,
    DrippingObsidianTear,
    DrippingWater,
    Dust,
    DustPillar,
    Effect,
    EggCrack,
    ElectricSpark,
    Enchant,
    EnchantedHit,
    EndRod,
    EntityEffect,
    Explosion,
    ExplosionEmitter,
    FallingDripstoneLava,
    FallingDripstoneWater,
    FallingDust,
    FallingHoney,
    FallingLava,
    FallingNectar,
    FallingObsidianTear,
    FallingSporeBlossom,
    FallingWater,
    Firefly,
    Fishing,
    Flame,
    Glow,
    HappyVillager,
    Heart,
    InstantEffect,
    Infested,
    Item,
    ItemCobweb,
    ItemSlime,
    ItemSnowball,
    LandingHoney,
    LandingLava,
    LandingObsidianTear,
    LargeSmoke,
    Lava,
    Mycelium,
    Note,
    PaleOakLeaves,
    Poof,
    Portal,
    Rain,
    Scrape,
    SculkSoul,
    SmallFlame,
    Smoke,
    Sneeze,
    Snowflake,
    Soul,
    SoulFireFlame,
    Splash,
    SporeBlossomAir,
    SweepAttack,
    TintedLeaves,
    Totem,
    Underwater,
    VaultConnection,
    WarpedSpore,
    WaxOff,
    WaxOn,
    WhiteAsh,
    WhiteSmoke,
    Witch,
}

impl Type {
    /// The particle's name, which is also its sprite list's file.
    pub fn name(self) -> &'static str {
        use Type::*;
        match self {
            AngryVillager => "angry_villager",
            Ash => "ash",
            Block => "block",
            BlockCrumble => "block_crumble",
            Bubble => "bubble",
            BubblePop => "bubble_pop",
            CampfireCosySmoke => "campfire_cosy_smoke",
            CampfireSignalSmoke => "campfire_signal_smoke",
            CherryLeaves => "cherry_leaves",
            Cloud => "cloud",
            Composter => "composter",
            CopperFireFlame => "copper_fire_flame",
            Crit => "crit",
            CrimsonSpore => "crimson_spore",
            DamageIndicator => "damage_indicator",
            Dolphin => "dolphin",
            DrippingDripstoneLava => "dripping_dripstone_lava",
            DrippingDripstoneWater => "dripping_dripstone_water",
            DrippingHoney => "dripping_honey",
            DrippingLava => "dripping_lava",
            DrippingObsidianTear => "dripping_obsidian_tear",
            DrippingWater => "dripping_water",
            Dust => "dust",
            DustPillar => "dust_pillar",
            Effect => "effect",
            EggCrack => "egg_crack",
            ElectricSpark => "electric_spark",
            Enchant => "enchant",
            EnchantedHit => "enchanted_hit",
            EndRod => "end_rod",
            EntityEffect => "entity_effect",
            Explosion => "explosion",
            ExplosionEmitter => "explosion_emitter",
            FallingDripstoneLava => "falling_dripstone_lava",
            FallingDripstoneWater => "falling_dripstone_water",
            FallingDust => "falling_dust",
            FallingHoney => "falling_honey",
            FallingLava => "falling_lava",
            FallingNectar => "falling_nectar",
            FallingObsidianTear => "falling_obsidian_tear",
            FallingSporeBlossom => "falling_spore_blossom",
            FallingWater => "falling_water",
            Firefly => "firefly",
            Fishing => "fishing",
            Flame => "flame",
            Glow => "glow",
            HappyVillager => "happy_villager",
            Heart => "heart",
            InstantEffect => "instant_effect",
            Infested => "infested",
            Item => "item",
            ItemCobweb => "item_cobweb",
            ItemSlime => "item_slime",
            ItemSnowball => "item_snowball",
            LandingHoney => "landing_honey",
            LandingLava => "landing_lava",
            LandingObsidianTear => "landing_obsidian_tear",
            LargeSmoke => "large_smoke",
            Lava => "lava",
            Mycelium => "mycelium",
            Note => "note",
            PaleOakLeaves => "pale_oak_leaves",
            Poof => "poof",
            Portal => "portal",
            Rain => "rain",
            Scrape => "scrape",
            SculkSoul => "sculk_soul",
            SmallFlame => "small_flame",
            Smoke => "smoke",
            Sneeze => "sneeze",
            Snowflake => "snowflake",
            Soul => "soul",
            SoulFireFlame => "soul_fire_flame",
            Splash => "splash",
            SporeBlossomAir => "spore_blossom_air",
            SweepAttack => "sweep_attack",
            TintedLeaves => "tinted_leaves",
            Totem => "totem_of_undying",
            Underwater => "underwater",
            VaultConnection => "vault_connection",
            WarpedSpore => "warped_spore",
            WaxOff => "wax_off",
            WaxOn => "wax_on",
            WhiteAsh => "white_ash",
            WhiteSmoke => "white_smoke",
            Witch => "witch",
        }
    }
}

/// `ParticleOptions`: a type, with the data some types carry.
#[derive(Clone, Debug)]
pub enum Options {
    Simple(Type),
    /// `BlockParticleOption`: block, falling dust, crumble, dust pillar.
    Block(Type, Block),
    /// `ItemParticleOption`, by item id.
    Item(String),
    /// `DustParticleOptions`: colour and scale.
    Dust([f32; 3], f32),
    /// `ColorParticleOption` (entity effect, tinted leaves): ARGB.
    Color(Type, [f32; 4]),
    /// `SpellParticleOption` (effect, instant effect): colour and power.
    Spell(Type, [f32; 3], f32),
}

impl From<Type> for Options {
    fn from(kind: Type) -> Self {
        Options::Simple(kind)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    None,
    Water,
    Lava,
}

/// `Particle.LifetimeAlpha`.
#[derive(Clone, Copy, Debug)]
pub struct LifetimeAlpha {
    start: f32,
    end: f32,
    start_at: f32,
    end_at: f32,
}

impl LifetimeAlpha {
    const OPAQUE: Self = Self {
        start: 1.0,
        end: 1.0,
        start_at: 0.0,
        end_at: 1.0,
    };

    fn at(&self, age: i32, lifetime: i32, partial: f32) -> f32 {
        if (self.start - self.end).abs() < 1.0e-5 {
            return self.start;
        }
        let t = (age as f32 + partial) / lifetime.max(1) as f32;
        let t = (t - self.start_at) / (self.end_at - self.start_at);
        self.start + (self.end - self.start) * t.clamp(0.0, 1.0)
    }
}

/// Each kind's own behaviour and state, after its vanilla class.
#[derive(Clone, Debug)]
pub enum Kind {
    /// `Particle.tick` as it is.
    Plain,
    /// `FlameParticle`: rises through blocks, shrinks, glows as it ages.
    Flame,
    /// `EmissiveRisingParticle` (soul, sculk soul).
    EmissiveRising { set: Type, glowing: bool },
    /// `BaseAshSmokeParticle` (smoke, large smoke, white smoke, ash).
    AshSmoke { set: Type },
    /// `LavaParticle`: a spark that sheds smoke.
    Lava,
    /// `DripParticle` and its hang, fall and land stages.
    Drip {
        fluid: Fluid,
        stage: DripStage,
        glowing: bool,
    },
    /// `WaterDropParticle` and `SplashParticle`.
    WaterDrop,
    /// `BubbleParticle`.
    Bubble,
    /// `BubblePopParticle`.
    BubblePop,
    /// `SuspendedTownParticle` (happy villager, mycelium, composter).
    SuspendedTown,
    /// `HeartParticle`, `NoteParticle`, `CritParticle`, `DustParticle`:
    /// grow in over the first thirty-second of their life.
    GrowIn { crit: bool, set: Option<Type> },
    /// `AttackSweepParticle` and `HugeExplosionParticle`: still, full
    /// bright, animated through their sprites.
    Flash { set: Type },
    /// `HugeExplosionSeedParticle`: unseen, sheds explosions.
    ExplosionSeed,
    /// `SpellParticle`.
    Spell { set: Type, original_alpha: f32 },
    /// `ExplodeParticle` (poof) and `SnowflakeParticle`: sprites by age.
    Animated { set: Type, snow: bool },
    /// `PlayerCloudParticle`: settles to the player's feet.
    Cloud { set: Type },
    /// `PortalParticle`: flies out and eases back.
    Portal { start: DVec3 },
    /// `SimpleAnimatedParticle` (end rod, totem): fades out, full bright.
    Fading {
        set: Type,
        fade: Option<[f32; 3]>,
        free: bool,
    },
    /// `FallingParticle` (leaves).
    Leaves(Leaves),
    /// `FallingDustParticle`.
    FallingDust { set: Type, spin: f32 },
    /// `GlowParticle`.
    Glow { set: Type },
    /// `FlyTowardsPositionParticle` (enchant).
    FlyTowards {
        start: DVec3,
        glowing: bool,
        alpha: LifetimeAlpha,
    },
    /// `TerrainParticle` and `BreakingItemParticle`: a quarter of a texture.
    Breaking,
    /// `WakeParticle`.
    Wake,
    /// `FireflyParticle`.
    Firefly,
    /// `CampfireSmokeParticle`.
    CampfireSmoke,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DripStage {
    /// Hangs, then becomes `falls`.
    Hang { falls: Type, cooling: bool },
    /// Falls, and on landing becomes `lands` (and sounds, for dripstone
    /// and honey).
    Fall {
        lands: Option<Type>,
        sound: Option<&'static str>,
    },
    /// Lies where it landed.
    Land,
}

#[derive(Clone, Debug)]
pub struct Leaves {
    rot_speed: f32,
    spin_acceleration: f32,
    wind: f32,
    swirl: bool,
    flow_away: bool,
    flow: (f64, f64),
    swirl_period: f64,
}

/// Makes the particle the options name (`ParticleProvider.createParticle`).
pub fn create(
    engine: &mut Particles,
    world: &World,
    options: &Options,
    pos: DVec3,
    motion: DVec3,
) -> Option<Particle> {
    use Type::*;
    let kind = match options {
        Options::Simple(kind)
        | Options::Block(kind, _)
        | Options::Color(kind, _)
        | Options::Spell(kind, _, _) => *kind,
        Options::Item(_) => Item,
        Options::Dust(..) => Dust,
    };
    let sprites = &engine.sprites;
    let set = sprites.get(kind.name()).clone();
    let random = &mut engine.random;
    let (x, y, z) = (motion.x, motion.y, motion.z);
    let p = match kind {
        Flame | SoulFireFlame | CopperFireFlame | SmallFlame => {
            let mut p = rising(Kind::Flame, pos, motion, set.pick(random), random);
            if kind == SmallFlame {
                p.scale(0.5);
            }
            p
        }
        Soul | SculkSoul => {
            let mut p = rising(
                Kind::EmissiveRising {
                    set: kind,
                    glowing: kind == SculkSoul,
                },
                pos,
                motion,
                set.first(),
                random,
            );
            p.scale(1.5);
            p.set_sprite_from_age(&set);
            p.layer = Layer::Translucent;
            p
        }
        Smoke | LargeSmoke | WhiteSmoke => {
            let scale = if kind == LargeSmoke { 2.5 } else { 1.0 };
            let mut p = ash_smoke(
                kind,
                pos,
                [0.1, 0.1, 0.1],
                motion,
                scale,
                &set,
                0.3,
                8,
                -0.1,
                true,
                random,
            );
            if kind == WhiteSmoke {
                p.rgb = [0.729_411_8, 0.694_117_67, 0.760_784_3];
            }
            p
        }
        Ash => ash_smoke(
            kind,
            pos,
            [0.1, -0.1, 0.1],
            DVec3::ZERO,
            1.0,
            &set,
            0.5,
            20,
            0.1,
            false,
            random,
        ),
        WhiteAsh => {
            let xa = f64::from(random.next_float()) * -1.9 * f64::from(random.next_float()) * 0.1;
            let ya =
                f64::from(random.next_float()) * -0.5 * f64::from(random.next_float()) * 0.1 * 5.0;
            let za = f64::from(random.next_float()) * -1.9 * f64::from(random.next_float()) * 0.1;
            let mut p = ash_smoke(
                kind,
                pos,
                [0.1, -0.1, 0.1],
                DVec3::new(xa, ya, za),
                1.0,
                &set,
                0.0,
                20,
                0.0125,
                false,
                random,
            );
            p.rgb = rgb(0xbab1c2);
            p
        }
        Lava => {
            let mut p = Particle::moving(Kind::Lava, pos, DVec3::ZERO, set.pick(random), random);
            p.gravity = 0.75;
            p.friction = 0.999;
            p.velocity *= f64::from(0.8f32);
            p.velocity.y = f64::from(random.next_float() * 0.4 + 0.05);
            p.quad_size *= random.next_float() * 2.0 + 0.2;
            p.lifetime = (16.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p
        }
        DrippingWater | DrippingDripstoneWater => {
            let falls = if kind == DrippingWater {
                FallingWater
            } else {
                FallingDripstoneWater
            };
            let mut p = drip(
                Fluid::Water,
                DripStage::Hang {
                    falls,
                    cooling: false,
                },
                pos,
                set.pick(random),
                random,
            );
            p.rgb = [0.2, 0.3, 1.0];
            p
        }
        DrippingLava | DrippingDripstoneLava => {
            let falls = if kind == DrippingLava {
                FallingLava
            } else {
                FallingDripstoneLava
            };
            drip(
                Fluid::Lava,
                DripStage::Hang {
                    falls,
                    cooling: true,
                },
                pos,
                set.pick(random),
                random,
            )
        }
        DrippingHoney | DrippingObsidianTear => {
            let falls = if kind == DrippingHoney {
                FallingHoney
            } else {
                FallingObsidianTear
            };
            let mut p = drip(
                Fluid::None,
                DripStage::Hang {
                    falls,
                    cooling: false,
                },
                pos,
                set.pick(random),
                random,
            );
            p.gravity *= 0.01;
            p.lifetime = 100;
            if kind == DrippingHoney {
                p.rgb = [0.622, 0.508, 0.082];
            } else {
                p.rgb = [0.511_718_75, 0.031_25, 0.890_625];
                p.kind = Kind::Drip {
                    fluid: Fluid::None,
                    stage: DripStage::Hang {
                        falls,
                        cooling: false,
                    },
                    glowing: true,
                };
            }
            p
        }
        FallingWater | FallingDripstoneWater => {
            let sound = (kind == FallingDripstoneWater)
                .then_some("minecraft:block.pointed_dripstone.drip_water");
            let mut p = drip(
                Fluid::Water,
                DripStage::Fall {
                    lands: Some(Splash),
                    sound,
                },
                pos,
                set.pick(random),
                random,
            );
            p.lifetime = (64.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.rgb = [0.2, 0.3, 1.0];
            p
        }
        FallingLava | FallingDripstoneLava => {
            let sound = (kind == FallingDripstoneLava)
                .then_some("minecraft:block.pointed_dripstone.drip_lava");
            let mut p = drip(
                Fluid::Lava,
                DripStage::Fall {
                    lands: Some(LandingLava),
                    sound,
                },
                pos,
                set.pick(random),
                random,
            );
            p.lifetime = (64.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.rgb = [1.0, 0.285_714_3, 0.083_333_336];
            p
        }
        FallingHoney => {
            let mut p = drip(
                Fluid::None,
                DripStage::Fall {
                    lands: Some(LandingHoney),
                    sound: Some("minecraft:block.beehive.drip"),
                },
                pos,
                set.pick(random),
                random,
            );
            p.lifetime = (64.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.gravity = 0.01;
            p.rgb = [0.582, 0.448, 0.082];
            p
        }
        FallingObsidianTear => {
            let mut p = drip(
                Fluid::None,
                DripStage::Fall {
                    lands: Some(LandingObsidianTear),
                    sound: None,
                },
                pos,
                set.pick(random),
                random,
            );
            p.lifetime = (64.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.gravity = 0.01;
            p.rgb = [0.511_718_75, 0.031_25, 0.890_625];
            p.kind = Kind::Drip {
                fluid: Fluid::None,
                stage: DripStage::Fall {
                    lands: Some(LandingObsidianTear),
                    sound: None,
                },
                glowing: true,
            };
            p
        }
        FallingNectar | FallingSporeBlossom => {
            let mut p = drip(
                Fluid::None,
                DripStage::Fall {
                    lands: None,
                    sound: None,
                },
                pos,
                set.pick(random),
                random,
            );
            if kind == FallingNectar {
                p.lifetime = (16.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
                p.gravity = 0.007;
                p.rgb = [0.92, 0.782, 0.72];
            } else {
                p.lifetime = (64.0 / random.range(0.1, 0.9)) as i32;
                p.gravity = 0.005;
                p.rgb = [0.32, 0.5, 0.22];
            }
            p
        }
        LandingLava | LandingHoney | LandingObsidianTear => {
            let fluid = if kind == LandingLava {
                Fluid::Lava
            } else {
                Fluid::None
            };
            let mut p = drip(fluid, DripStage::Land, pos, set.pick(random), random);
            p.lifetime = (16.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            match kind {
                LandingLava => p.rgb = [1.0, 0.285_714_3, 0.083_333_336],
                LandingHoney => {
                    p.lifetime = (128.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
                    p.rgb = [0.522, 0.408, 0.082];
                }
                _ => {
                    p.lifetime = (28.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
                    p.rgb = [0.511_718_75, 0.031_25, 0.890_625];
                    p.kind = Kind::Drip {
                        fluid,
                        stage: DripStage::Land,
                        glowing: true,
                    };
                }
            }
            p
        }
        Rain | Splash => {
            let mut p = water_drop(pos, set.pick(random), random);
            if kind == Splash {
                p.gravity = 0.04;
                if y == 0.0 && (x != 0.0 || z != 0.0) {
                    p.velocity = DVec3::new(x, 0.1, z);
                }
            }
            p
        }
        Bubble => {
            let mut p = Particle::at(Kind::Bubble, pos, set.pick(random), random);
            p.set_size(0.02, 0.02);
            p.quad_size *= random.next_float() * 0.6 + 0.2;
            let mut jitter = || f64::from((random.next_float() * 2.0 - 1.0) * 0.02);
            p.velocity = DVec3::new(
                x * f64::from(0.2f32) + jitter(),
                y * f64::from(0.2f32) + jitter(),
                z * f64::from(0.2f32) + jitter(),
            );
            p.lifetime = (8.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p
        }
        BubblePop => {
            let mut p = Particle::at(Kind::BubblePop, pos, set.first(), random);
            p.lifetime = 4;
            p.gravity = 0.008;
            p.velocity = motion;
            p.set_sprite_from_age(&set);
            p
        }
        Underwater | SporeBlossomAir | CrimsonSpore | WarpedSpore => {
            let below = pos - DVec3::new(0.0, 0.125, 0.0);
            let mut p = if kind == Underwater {
                let mut p = Particle::at(Kind::Plain, below, set.pick(random), random);
                p.quad_size *= random.next_float() * 0.6 + 0.2;
                p
            } else {
                let motion = match kind {
                    SporeBlossomAir => DVec3::new(0.0, f64::from(-0.8f32), 0.0),
                    CrimsonSpore => DVec3::new(
                        random.next_gaussian() * f64::from(1.0e-6f32),
                        random.next_gaussian() * f64::from(1.0e-4f32),
                        random.next_gaussian() * f64::from(1.0e-6f32),
                    ),
                    _ => DVec3::new(
                        0.0,
                        f64::from(random.next_float())
                            * -1.9
                            * f64::from(random.next_float())
                            * 0.1,
                        0.0,
                    ),
                };
                let mut p = Particle::moving(Kind::Plain, below, motion, set.pick(random), random);
                p.quad_size *= random.next_float() * 0.6 + 0.6;
                p
            };
            p.set_size(0.01, 0.01);
            p.lifetime = (16.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.has_physics = false;
            p.friction = 1.0;
            p.gravity = 0.0;
            match kind {
                Underwater => p.rgb = [0.4, 0.4, 0.7],
                SporeBlossomAir => {
                    p.lifetime = 500 + random.next_int(501);
                    p.gravity = 0.01;
                    p.rgb = [0.32, 0.5, 0.22];
                }
                CrimsonSpore => p.rgb = [0.9, 0.4, 0.5],
                _ => {
                    p.rgb = [0.1, 0.1, 0.3];
                    p.set_size(0.001, 0.001);
                }
            }
            p
        }
        HappyVillager | Mycelium | Composter | Dolphin | EggCrack => {
            let mut p =
                Particle::moving(Kind::SuspendedTown, pos, motion, set.pick(random), random);
            let br = random.next_float() * 0.1 + 0.2;
            p.rgb = [br; 3];
            p.set_size(0.02, 0.02);
            p.quad_size *= random.next_float() * 0.6 + 0.5;
            p.velocity *= f64::from(0.02f32);
            p.lifetime = (20.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            match kind {
                HappyVillager | EggCrack => p.rgb = [1.0; 3],
                Composter => {
                    p.rgb = [1.0; 3];
                    p.lifetime = 3 + random.next_int(5);
                }
                Dolphin => {
                    p.rgb = [0.3, 0.5, 1.0];
                    p.alpha = 1.0 - random.next_float() * 0.7;
                    p.lifetime /= 2;
                }
                _ => {}
            }
            p
        }
        Heart | AngryVillager => {
            let at = if kind == AngryVillager {
                pos + DVec3::Y * 0.5
            } else {
                pos
            };
            let mut p = Particle::moving(
                Kind::GrowIn {
                    crit: false,
                    set: None,
                },
                at,
                DVec3::ZERO,
                set.pick(random),
                random,
            );
            p.speed_up_when_y_blocked = true;
            p.friction = 0.86;
            p.velocity *= f64::from(0.01f32);
            p.velocity.y += 0.1;
            p.quad_size *= 1.5;
            p.lifetime = 16;
            p.has_physics = false;
            p
        }
        Note => {
            let mut p = Particle::moving(
                Kind::GrowIn {
                    crit: false,
                    set: None,
                },
                pos,
                DVec3::ZERO,
                set.pick(random),
                random,
            );
            p.friction = 0.66;
            p.speed_up_when_y_blocked = true;
            p.velocity *= f64::from(0.01f32);
            p.velocity.y += 0.2;
            let colour = x as f32;
            let channel = |offset: f32| {
                ((colour + offset) * std::f32::consts::TAU)
                    .sin()
                    .mul_add(0.65, 0.35)
                    .max(0.0)
            };
            p.rgb = [channel(0.0), channel(0.333_333_34), channel(0.666_666_7)];
            p.quad_size *= 1.5;
            p.lifetime = 6;
            p
        }
        Crit | EnchantedHit | DamageIndicator => {
            let motion = if kind == DamageIndicator {
                motion + DVec3::Y
            } else {
                motion
            };
            let mut p = Particle::moving(
                Kind::GrowIn {
                    crit: true,
                    set: None,
                },
                pos,
                DVec3::ZERO,
                set.pick(random),
                random,
            );
            p.friction = 0.7;
            p.gravity = 0.5;
            p.velocity *= f64::from(0.1f32);
            p.velocity += motion * 0.4;
            let col = random.next_float() * 0.3 + 0.6;
            p.rgb = [col; 3];
            p.quad_size *= 0.75;
            p.lifetime = ((6.0 / (f64::from(random.next_float()) * 0.8 + 0.6)) as i32).max(1);
            p.has_physics = false;
            // The constructor ticks it once.
            p.base_tick(world);
            p.rgb[1] *= 0.96;
            p.rgb[2] *= 0.9;
            match kind {
                DamageIndicator => p.lifetime = 20,
                EnchantedHit => {
                    p.rgb[0] *= 0.3;
                    p.rgb[1] *= 0.8;
                }
                _ => {}
            }
            p
        }
        SweepAttack | Explosion => {
            let mut p = Particle::moving(
                Kind::Flash { set: kind },
                pos,
                DVec3::ZERO,
                set.first(),
                random,
            );
            if kind == SweepAttack {
                p.lifetime = 4;
                let col = random.next_float() * 0.6 + 0.4;
                p.rgb = [col; 3];
                p.quad_size = 1.0 - x as f32 * 0.5;
            } else {
                p.lifetime = 6 + random.next_int(4);
                let col = random.next_float() * 0.6 + 0.4;
                p.rgb = [col; 3];
                p.quad_size = 2.0 * (1.0 - x as f32 * 0.5);
            }
            p.set_sprite_from_age(&set);
            p
        }
        ExplosionEmitter => {
            let mut p = Particle::moving(Kind::ExplosionSeed, pos, DVec3::ZERO, [0.0; 4], random);
            p.lifetime = 8;
            p.alpha = 0.0;
            p
        }
        Effect | InstantEffect | EntityEffect | Witch | Infested => {
            let scatter = DVec3::new(0.5 - random.next_double(), y, 0.5 - random.next_double());
            let mut p = Particle::moving(
                Kind::Spell {
                    set: kind,
                    original_alpha: 1.0,
                },
                pos,
                scatter,
                set.first(),
                random,
            );
            p.friction = 0.96;
            p.gravity = -0.1;
            p.speed_up_when_y_blocked = true;
            p.velocity.y *= f64::from(0.2f32);
            if x == 0.0 && z == 0.0 {
                p.velocity.x *= f64::from(0.1f32);
                p.velocity.z *= f64::from(0.1f32);
            }
            p.quad_size *= 0.75;
            p.lifetime = (8.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.has_physics = false;
            p.set_sprite_from_age(&set);
            p.layer = Layer::Translucent;
            match options {
                Options::Spell(_, colour, power) => {
                    p.rgb = *colour;
                    p.set_power(*power);
                }
                Options::Color(_, argb) => {
                    p.rgb = [argb[1], argb[2], argb[3]];
                    p.alpha = argb[0];
                    p.kind = Kind::Spell {
                        set: kind,
                        original_alpha: argb[0],
                    };
                }
                _ if kind == Witch => {
                    let bright = random.next_float() * 0.5 + 0.35;
                    p.rgb = [bright, 0.0, bright];
                }
                _ => {}
            }
            p
        }
        Poof | Snowflake => {
            let mut p = Particle::at(
                Kind::Animated {
                    set: kind,
                    snow: kind == Snowflake,
                },
                pos,
                set.first(),
                random,
            );
            let mut jitter = || f64::from((random.next_float() * 2.0 - 1.0) * 0.05);
            p.velocity = DVec3::new(x + jitter(), y + jitter(), z + jitter());
            if kind == Poof {
                p.gravity = -0.1;
                p.friction = 0.9;
                let col = random.next_float() * 0.3 + 0.7;
                p.rgb = [col; 3];
                p.quad_size = 0.1 * (random.next_float() * random.next_float() * 6.0 + 1.0);
            } else {
                p.gravity = 0.225;
                p.friction = 1.0;
                p.quad_size = 0.1 * (random.next_float() * random.next_float() + 1.0);
                p.rgb = [0.923, 0.964, 0.999];
            }
            p.lifetime = (16.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32 + 2;
            p.set_sprite_from_age(&set);
            p
        }
        Cloud | Sneeze => {
            let mut p = Particle::moving(
                Kind::Cloud { set: kind },
                pos,
                DVec3::ZERO,
                set.first(),
                random,
            );
            p.friction = 0.96;
            p.velocity *= f64::from(0.1f32);
            p.velocity += motion;
            let col = 1.0 - random.next_float() * 0.3;
            p.rgb = [col; 3];
            p.quad_size *= 1.875;
            let base = (8.0 / (f64::from(random.next_float()) * 0.8 + 0.3)) as i32;
            p.lifetime = (base as f32 * 2.5).max(1.0) as i32;
            p.has_physics = false;
            p.set_sprite_from_age(&set);
            p.layer = Layer::Translucent;
            if kind == Sneeze {
                p.rgb = [0.22, 1.0, 0.53];
                p.alpha = 0.4;
            }
            p
        }
        Portal => {
            let mut p = Particle::at(Kind::Portal { start: pos }, pos, set.pick(random), random);
            p.velocity = motion;
            p.quad_size = 0.1 * (random.next_float() * 0.2 + 0.5);
            let br = random.next_float() * 0.6 + 0.4;
            p.rgb = [br * 0.9, br * 0.3, br];
            p.lifetime = (random.next_float() * 10.0) as i32 + 40;
            p
        }
        EndRod | Totem => {
            let mut p = Particle::at(
                Kind::Fading {
                    set: kind,
                    fade: (kind == EndRod).then(|| rgb(0xf2_dd_c9)),
                    free: kind == EndRod,
                },
                pos,
                set.first(),
                random,
            );
            p.layer = Layer::Translucent;
            p.velocity = motion;
            p.quad_size *= 0.75;
            p.lifetime = 60 + random.next_int(12);
            if kind == EndRod {
                p.friction = 0.91;
                p.gravity = 0.0125;
            } else {
                p.friction = 0.6;
                p.gravity = 1.25;
                p.rgb = if random.next_int(4) == 0 {
                    [
                        0.6 + random.next_float() * 0.2,
                        0.6 + random.next_float() * 0.3,
                        random.next_float() * 0.2,
                    ]
                } else {
                    [
                        0.1 + random.next_float() * 0.2,
                        0.4 + random.next_float() * 0.3,
                        random.next_float() * 0.2,
                    ]
                };
            }
            p.set_sprite_from_age(&set);
            p
        }
        CherryLeaves | PaleOakLeaves | TintedLeaves => {
            let (fall, side, swirl, flow_away, scale, start) = if kind == CherryLeaves {
                (0.25, 2.0, false, true, 1.0, 0.0)
            } else {
                (0.07, 10.0, true, false, 2.0, 0.021)
            };
            let mut p = leaves(
                pos,
                set.pick(random),
                random,
                fall,
                side,
                swirl,
                flow_away,
                scale,
                start,
            );
            if let Options::Color(_, argb) = options {
                p.rgb = [argb[1], argb[2], argb[3]];
            }
            p
        }
        Dust => {
            let (colour, scale) = match options {
                Options::Dust(colour, scale) => (*colour, *scale),
                _ => ([1.0, 0.0, 0.0], 1.0),
            };
            let mut p = Particle::moving(
                Kind::GrowIn {
                    crit: false,
                    set: Some(Dust),
                },
                pos,
                motion,
                set.first(),
                random,
            );
            p.friction = 0.96;
            p.speed_up_when_y_blocked = true;
            p.velocity *= f64::from(0.1f32);
            p.quad_size *= 0.75 * scale;
            let base = (8.0 / (random.next_double() * 0.8 + 0.2)) as i32;
            p.lifetime = (base as f32 * scale).max(1.0) as i32;
            p.set_sprite_from_age(&set);
            let base_factor = random.next_float() * 0.4 + 0.6;
            for (channel, value) in p.rgb.iter_mut().zip(colour) {
                *channel = (random.next_float() * 0.2 + 0.8) * value * base_factor;
            }
            p
        }
        FallingDust => {
            let Options::Block(_, block) = options else {
                return None;
            };
            let mut p = Particle::at(
                Kind::FallingDust {
                    set: kind,
                    spin: 0.0,
                },
                pos,
                set.first(),
                random,
            );
            p.rgb = dust_colour(block);
            p.quad_size *= 0.674_999_95;
            let base = (32.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.lifetime = (base as f32 * 0.9).max(1.0) as i32;
            p.set_sprite_from_age(&set);
            let spin = (random.next_float() - 0.5) * 0.1;
            p.roll = random.next_float() * std::f32::consts::TAU;
            p.previous_roll = p.roll;
            p.kind = Kind::FallingDust { set: kind, spin };
            p
        }
        Glow | WaxOn | WaxOff | ElectricSpark | Scrape => {
            let start = if kind == Glow {
                DVec3::new(0.5 - random.next_double(), y, 0.5 - random.next_double())
            } else {
                DVec3::ZERO
            };
            let mut p = Particle::moving(Kind::Glow { set: kind }, pos, start, set.first(), random);
            p.friction = 0.96;
            p.speed_up_when_y_blocked = true;
            p.quad_size *= 0.75;
            p.has_physics = false;
            p.set_sprite_from_age(&set);
            match kind {
                Glow => {
                    p.rgb = if random.next_bool() {
                        [0.6, 1.0, 0.8]
                    } else {
                        [0.08, 0.4, 0.4]
                    };
                    p.velocity.y *= f64::from(0.2f32);
                    if x == 0.0 && z == 0.0 {
                        p.velocity.x *= f64::from(0.1f32);
                        p.velocity.z *= f64::from(0.1f32);
                    }
                    p.lifetime = (8.0 / (random.next_double() * 0.8 + 0.2)) as i32;
                }
                ElectricSpark => {
                    p.rgb = [1.0, 0.9, 1.0];
                    p.velocity = motion * 0.25;
                    p.lifetime = random.next_int(2) + 2;
                }
                Scrape => {
                    p.rgb = if random.next_bool() {
                        [0.29, 0.58, 0.51]
                    } else {
                        [0.43, 0.77, 0.62]
                    };
                    p.velocity = motion * 0.01;
                    p.lifetime = random.next_int(30) + 10;
                }
                _ => {
                    p.rgb = if kind == WaxOn {
                        [0.91, 0.55, 0.08]
                    } else {
                        [1.0, 0.9, 1.0]
                    };
                    p.velocity = DVec3::new(x * 0.01 / 2.0, y * 0.01, z * 0.01 / 2.0);
                    p.lifetime = random.next_int(30) + 10;
                }
            }
            p
        }
        Enchant | VaultConnection => {
            let (glowing, alpha) = if kind == VaultConnection {
                (
                    true,
                    LifetimeAlpha {
                        start: 0.0,
                        end: 0.6,
                        start_at: 0.25,
                        end_at: 1.0,
                    },
                )
            } else {
                (false, LifetimeAlpha::OPAQUE)
            };
            let mut p = Particle::at(
                Kind::FlyTowards {
                    start: pos,
                    glowing,
                    alpha,
                },
                pos + motion,
                set.pick(random),
                random,
            );
            p.alpha = alpha.start;
            p.velocity = motion;
            p.previous = pos + motion;
            p.quad_size = 0.1 * (random.next_float() * 0.5 + 0.2);
            let br = random.next_float() * 0.6 + 0.4;
            p.rgb = [0.9 * br, 0.9 * br, br];
            p.has_physics = false;
            p.lifetime = (random.next_float() * 10.0) as i32 + 30;
            if alpha.start < 1.0 || alpha.end < 1.0 {
                p.layer = Layer::Translucent;
            }
            if kind == VaultConnection {
                p.scale(1.5);
            }
            p
        }
        Item | ItemSlime | ItemSnowball | ItemCobweb => {
            let id = match options {
                Options::Item(id) => id.clone(),
                _ => match kind {
                    ItemSlime => "minecraft:slime_ball".to_owned(),
                    ItemSnowball => "minecraft:snowball".to_owned(),
                    _ => "minecraft:cobweb".to_owned(),
                },
            };
            let region = item_region(world, &id)?;
            let mut p = Particle::moving(Kind::Breaking, pos, DVec3::ZERO, region, random);
            if kind == Item {
                p.velocity *= f64::from(0.1f32);
                p.velocity += motion;
            }
            breaking(&mut p, region, random);
            p
        }
        Block | BlockCrumble | DustPillar => {
            let Options::Block(_, block) = options else {
                return None;
            };
            if matches!(
                block.id.path.as_str(),
                "air" | "cave_air" | "void_air" | "moving_piston" | "water" | "lava"
            ) {
                return None;
            }
            let texture =
                minecraft_terrain::model::block_particle_texture(&world.packs, block).ok()??;
            if !world.atlas.contains(&texture) {
                return None;
            }
            let region = world.atlas.region(&texture);
            let mut p = Particle::moving(Kind::Breaking, pos, motion, region, random);
            let block_pos = (
                pos.x.floor() as i32,
                pos.y.floor() as i32,
                pos.z.floor() as i32,
            );
            let tint = minecraft_terrain::block_particles::terrain_tint(
                &engine.tint,
                &world.scene,
                block_pos,
                block,
            );
            p.rgb = [0.6 * tint[0], 0.6 * tint[1], 0.6 * tint[2]];
            breaking(&mut p, region, &mut engine.random);
            let random = &mut engine.random;
            match kind {
                BlockCrumble => {
                    p.velocity = DVec3::ZERO;
                    p.lifetime = random.next_int(10) + 1;
                }
                DustPillar => {
                    p.velocity = DVec3::new(
                        random.next_gaussian() / 30.0,
                        y + random.next_gaussian() / 2.0,
                        random.next_gaussian() / 30.0,
                    );
                    p.lifetime = random.next_int(20) + 20;
                }
                _ => {}
            }
            return Some(p);
        }
        Fishing => {
            let mut p = Particle::moving(Kind::Wake, pos, DVec3::ZERO, set.first(), random);
            p.set_size(0.01, 0.01);
            p.lifetime = (8.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
            p.gravity = 0.0;
            p.velocity = motion;
            p.sprite = set.by_age(0, 4);
            p
        }
        Firefly => {
            let start = DVec3::new(
                0.5 - random.next_double(),
                if random.next_bool() { y } else { -y },
                0.5 - random.next_double(),
            );
            let mut p = Particle::moving(Kind::Firefly, pos, start, set.pick(random), random);
            p.speed_up_when_y_blocked = true;
            p.friction = 0.96;
            p.quad_size *= 0.75;
            p.velocity *= f64::from(0.8f32);
            p.lifetime = 200 + random.next_int(101);
            p.scale(1.5);
            p.alpha = 0.0;
            p.layer = Layer::Translucent;
            p
        }
        CampfireCosySmoke | CampfireSignalSmoke => {
            let mut p = Particle::at(Kind::CampfireSmoke, pos, set.pick(random), random);
            p.scale(3.0);
            p.set_size(0.25, 0.25);
            p.lifetime = random.next_int(50) + if kind == CampfireSignalSmoke { 280 } else { 80 };
            p.gravity = 3.0e-6;
            p.velocity = DVec3::new(x, y + f64::from(random.next_float() / 500.0), z);
            p.alpha = if kind == CampfireSignalSmoke {
                0.95
            } else {
                0.9
            };
            p.layer = Layer::Translucent;
            p
        }
    };
    Some(p)
}

fn rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

/// `RisingParticle`.
fn rising(
    kind: Kind,
    pos: DVec3,
    motion: DVec3,
    sprite: [f32; 4],
    random: &mut Random,
) -> Particle {
    let mut p = Particle::moving(kind, pos, motion, sprite, random);
    p.friction = 0.96;
    p.velocity = p.velocity * f64::from(0.01f32) + motion;
    // The jitter moves the particle but not its box, as vanilla's does.
    let mut jitter = || f64::from((random.next_float() - random.next_float()) * 0.05);
    p.pos += DVec3::new(jitter(), jitter(), jitter());
    p.lifetime = (8.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32 + 4;
    p
}

/// `BaseAshSmokeParticle`.
#[allow(clippy::too_many_arguments)]
fn ash_smoke(
    kind: Type,
    pos: DVec3,
    dir: [f64; 3],
    motion: DVec3,
    scale: f32,
    set: &super::SpriteSet,
    colour_random: f32,
    max_lifetime: i32,
    gravity: f32,
    has_physics: bool,
    random: &mut Random,
) -> Particle {
    let mut p = Particle::moving(
        Kind::AshSmoke { set: kind },
        pos,
        DVec3::ZERO,
        set.first(),
        random,
    );
    p.friction = 0.96;
    p.gravity = gravity;
    p.speed_up_when_y_blocked = true;
    p.velocity = DVec3::new(
        p.velocity.x * dir[0] + motion.x,
        p.velocity.y * dir[1] + motion.y,
        p.velocity.z * dir[2] + motion.z,
    );
    let col = random.next_float() * colour_random;
    p.rgb = [col; 3];
    p.quad_size *= 0.75 * scale;
    p.lifetime = ((f64::from(max_lifetime) / (f64::from(random.next_float()) * 0.8 + 0.2))
        * f64::from(scale)) as i32;
    p.lifetime = p.lifetime.max(1);
    p.set_sprite_from_age(set);
    p.has_physics = has_physics;
    p
}

/// `DripParticle`.
fn drip(
    fluid: Fluid,
    stage: DripStage,
    pos: DVec3,
    sprite: [f32; 4],
    random: &mut Random,
) -> Particle {
    let mut p = Particle::at(
        Kind::Drip {
            fluid,
            stage,
            glowing: false,
        },
        pos,
        sprite,
        random,
    );
    p.set_size(0.01, 0.01);
    p.gravity = 0.06;
    if let DripStage::Hang { .. } = stage {
        p.gravity *= 0.02;
        p.lifetime = 40;
    }
    p
}

/// `WaterDropParticle`.
fn water_drop(pos: DVec3, sprite: [f32; 4], random: &mut Random) -> Particle {
    let mut p = Particle::moving(Kind::WaterDrop, pos, DVec3::ZERO, sprite, random);
    p.velocity.x *= f64::from(0.3f32);
    p.velocity.y = f64::from(random.next_float() * 0.2 + 0.1);
    p.velocity.z *= f64::from(0.3f32);
    p.set_size(0.01, 0.01);
    p.gravity = 0.06;
    p.lifetime = (8.0 / (f64::from(random.next_float()) * 0.8 + 0.2)) as i32;
    p
}

/// `FallingParticle`.
#[allow(clippy::too_many_arguments)]
fn leaves(
    pos: DVec3,
    sprite: [f32; 4],
    random: &mut Random,
    fall: f32,
    side: f32,
    swirl: bool,
    flow_away: bool,
    scale: f32,
    start: f32,
) -> Particle {
    let rot_speed = if random.next_bool() { -30f32 } else { 30.0 }.to_radians();
    let spin_acceleration = if random.next_bool() { -5f32 } else { 5.0 }.to_radians();
    let mut p = Particle::at(Kind::Plain, pos, sprite, random);
    p.lifetime = 300;
    p.gravity = fall * 1.2 * 0.0025;
    let size = scale * if random.next_bool() { 0.05 } else { 0.075 };
    p.quad_size = size;
    p.set_size(size, size);
    p.friction = 1.0;
    p.velocity.y = -f64::from(start);
    let r = random.next_float();
    let angle = f64::from(r * 60.0).to_radians();
    p.kind = Kind::Leaves(Leaves {
        rot_speed,
        spin_acceleration,
        wind: side,
        swirl,
        flow_away,
        flow: (angle.cos() * f64::from(side), angle.sin() * f64::from(side)),
        swirl_period: f64::from(1000.0 + r * 3000.0).to_radians(),
    });
    p
}

/// `BreakingItemParticle` and `TerrainParticle`: gravity, half size, and a
/// random quarter of the texture.
fn breaking(p: &mut Particle, region: [f32; 4], random: &mut Random) {
    p.gravity = 1.0;
    p.quad_size /= 2.0;
    let uo = random.next_float() * 3.0;
    let vo = random.next_float() * 3.0;
    let [u0, v0, u1, v1] = region;
    let u = |f: f32| u0 + (u1 - u0) * f;
    let v = |f: f32| v0 + (v1 - v0) * f;
    p.sprite = [
        u((uo + 1.0) / 4.0),
        v(vo / 4.0),
        u(uo / 4.0),
        v((vo + 1.0) / 4.0),
    ];
}

/// The texture an item breaks into: its flat sprite, or its block's.
fn item_region(world: &World, id: &str) -> Option<[f32; 4]> {
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    let flat =
        minecraft_terrain::pack::ResourceId::parse(&format!("minecraft:item/{name}")).ok()?;
    if world.atlas.contains(&flat) {
        return Some(world.atlas.region(&flat));
    }
    let block = Block::new(id);
    let texture = minecraft_terrain::model::block_particle_texture(&world.packs, &block).ok()??;
    world
        .atlas
        .contains(&texture)
        .then(|| world.atlas.region(&texture))
}

/// `FallingDustParticle.Provider`'s colour: a falling block's dust colour,
/// else its map colour.
fn dust_colour(block: &Block) -> [f32; 3] {
    let hex = match block.id.path.as_str() {
        "sand" | "suspicious_sand" => 0xdbd3a0,
        "red_sand" => 0xa95821,
        "gravel" | "suspicious_gravel" => 0x807c7b,
        "anvil" | "chipped_anvil" | "damaged_anvil" => 0x404040,
        "dragon_egg" => 0x191919,
        path if path.ends_with("concrete_powder") => 0x9e9e9e,
        _ => 0x707070,
    };
    rgb(hex)
}

/// The world's light, plus block light rising from none to full over
/// `progress` (`LightCoordsUtil.addSmoothBlockEmission`).
fn emission(light: Light, progress: f32) -> Light {
    let emitted = (progress.clamp(0.0, 1.0) * 240.0) as u32;
    Light {
        sky: light.sky,
        block: (u32::from(light.block) + emitted).min(240) as u8,
    }
}

/// `LightCoordsUtil.withBlock(light, 15)`.
fn full_block(light: Light) -> Light {
    Light {
        sky: light.sky,
        block: 240,
    }
}

/// The light a particle draws with this frame.
pub fn light(p: &Particle, world: &World, partial: f32) -> Light {
    let progress = (p.age as f32 + partial) / p.lifetime.max(1) as f32;
    match &p.kind {
        Kind::Flash { .. } | Kind::Fading { .. } => FULL_BRIGHT,
        Kind::Flame | Kind::Glow { .. } => emission(p.world_light(world), progress),
        Kind::EmissiveRising { glowing: true, .. }
        | Kind::Drip { glowing: true, .. }
        | Kind::Lava
        | Kind::FlyTowards { glowing: true, .. } => full_block(p.world_light(world)),
        Kind::Portal { .. } | Kind::FlyTowards { .. } => {
            let b = p.age as f32 / p.lifetime.max(1) as f32;
            emission(p.world_light(world), b * b * b * b)
        }
        Kind::Firefly => Light {
            sky: 0,
            block: (255.0 * fade_amount(progress.clamp(0.0, 1.0), 0.1, 0.3)).min(240.0) as u8,
        },
        _ => p.world_light(world),
    }
}

/// `getQuadSize(partial)`.
pub fn quad_size(p: &Particle, partial: f32) -> f32 {
    let life = p.lifetime.max(1) as f32;
    let s = (p.age as f32 + partial) / life;
    match &p.kind {
        Kind::Flame => p.quad_size * (1.0 - s * s * 0.5),
        Kind::Lava => p.quad_size * (1.0 - s * s),
        Kind::Portal { .. } => {
            let s = 1.0 - s;
            p.quad_size * (1.0 - s * s)
        }
        Kind::AshSmoke { .. }
        | Kind::GrowIn { .. }
        | Kind::Cloud { .. }
        | Kind::FallingDust { .. } => p.quad_size * (s * 32.0).clamp(0.0, 1.0),
        _ => p.quad_size,
    }
}

/// The alpha this frame, for kinds that set it while drawing.
pub fn alpha(p: &Particle, partial: f32) -> f32 {
    match &p.kind {
        Kind::FlyTowards { alpha, .. } => alpha.at(p.age, p.lifetime, partial),
        _ => p.alpha,
    }
}

fn fade_amount(progress: f32, fade_in: f32, fade_out: f32) -> f32 {
    if progress >= 1.0 - fade_in {
        (1.0 - progress) / fade_in
    } else if progress <= fade_out {
        progress / fade_out
    } else {
        1.0
    }
}

/// The fluid at a block and its height there (`FluidState.getHeight`).
fn fluid_at(world: &World, pos: (i32, i32, i32)) -> Option<(Fluid, f64)> {
    let block = world.block(pos)?;
    let fluid = match block.id.path.as_str() {
        "water" | "bubble_column" | "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => {
            Fluid::Water
        }
        "lava" => Fluid::Lava,
        _ if block
            .properties
            .get("waterlogged")
            .is_some_and(|w| w == "true") =>
        {
            Fluid::Water
        }
        _ => return None,
    };
    let above = world.block((pos.0, pos.1 + 1, pos.2)).is_some_and(|b| {
        let same = match fluid {
            Fluid::Water => {
                b.id.path == "water" || b.properties.get("waterlogged").is_some_and(|w| w == "true")
            }
            _ => b.id.path == "lava",
        };
        same
    });
    if above {
        return Some((fluid, 1.0));
    }
    let level: u32 = block
        .properties
        .get("level")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let amount = if level == 0 || level >= 8 {
        8
    } else {
        8 - level
    };
    Some((fluid, f64::from(amount) / 9.0))
}

/// The top of a block's collision shape over a point (`max(Y, x, z)`).
fn collision_top(world: &World, pos: (i32, i32, i32), x: f64, z: f64) -> f64 {
    world
        .collision_boxes(pos)
        .iter()
        .filter(|b| x >= b[0] && x <= b[3] && z >= b[2] && z <= b[5])
        .map(|b| b[4])
        .fold(0.0, f64::max)
}

fn block_pos(pos: DVec3) -> (i32, i32, i32) {
    (
        pos.x.floor() as i32,
        pos.y.floor() as i32,
        pos.z.floor() as i32,
    )
}

/// One tick of a particle, as its class ticks it.
pub fn tick(p: &mut Particle, engine: &mut Particles, world: &World) {
    match p.kind.clone() {
        Kind::Plain
        | Kind::GrowIn {
            set: None,
            crit: false,
        } => p.base_tick(world),
        Kind::GrowIn { crit, set } => {
            p.base_tick(world);
            if crit {
                p.rgb[1] *= 0.96;
                p.rgb[2] *= 0.9;
            }
            if let Some(set) = set {
                p.set_sprite_from_age(engine.sprites.get(set.name()));
            }
        }
        Kind::Flame => free_tick(p),
        Kind::EmissiveRising { set, .. }
        | Kind::AshSmoke { set }
        | Kind::Animated { set, snow: false } => {
            p.base_tick(world);
            p.set_sprite_from_age(engine.sprites.get(set.name()));
        }
        Kind::Animated { set, snow: true } => {
            p.base_tick(world);
            p.set_sprite_from_age(engine.sprites.get(set.name()));
            p.velocity.x *= f64::from(0.95f32);
            p.velocity.y *= f64::from(0.9f32);
            p.velocity.z *= f64::from(0.95f32);
        }
        Kind::Lava => {
            p.base_tick(world);
            if !p.removed {
                let odds = p.age as f32 / p.lifetime.max(1) as f32;
                if engine.random.next_float() > odds {
                    engine.spawn(world, &Type::Smoke.into(), p.pos, p.velocity);
                }
            }
        }
        Kind::Drip { fluid, stage, .. } => drip_tick(p, engine, world, fluid, stage),
        Kind::WaterDrop => {
            p.previous = p.pos;
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
                return;
            }
            p.velocity.y -= f64::from(p.gravity);
            p.move_by(world, p.velocity);
            p.velocity *= f64::from(0.98f32);
            if p.on_ground {
                if engine.random.next_float() < 0.5 {
                    p.removed = true;
                }
                p.velocity.x *= f64::from(0.7f32);
                p.velocity.z *= f64::from(0.7f32);
            }
            let at = block_pos(p.pos);
            let top = collision_top(
                world,
                at,
                p.pos.x - f64::from(at.0),
                p.pos.z - f64::from(at.2),
            )
            .max(fluid_at(world, at).map_or(0.0, |(_, h)| h));
            if top > 0.0 && p.pos.y < f64::from(at.1) + top {
                p.removed = true;
            }
        }
        Kind::Bubble => {
            p.previous = p.pos;
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
                return;
            }
            p.velocity.y += 0.002;
            p.move_by(world, p.velocity);
            p.velocity *= f64::from(0.85f32);
            if !matches!(fluid_at(world, block_pos(p.pos)), Some((Fluid::Water, _))) {
                p.removed = true;
            }
        }
        Kind::BubblePop => {
            p.previous = p.pos;
            let age = p.age;
            p.age += 1;
            if age >= p.lifetime {
                p.removed = true;
                return;
            }
            p.velocity.y -= f64::from(p.gravity);
            p.move_by(world, p.velocity);
            p.set_sprite_from_age(engine.sprites.get(Type::BubblePop.name()));
        }
        Kind::SuspendedTown => {
            p.previous = p.pos;
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
                return;
            }
            p.move_freely(p.velocity);
            p.velocity *= 0.99;
        }
        Kind::Flash { set } => {
            p.previous = p.pos;
            let age = p.age;
            p.age += 1;
            if age >= p.lifetime {
                p.removed = true;
            } else {
                p.set_sprite_from_age(engine.sprites.get(set.name()));
            }
        }
        Kind::ExplosionSeed => {
            for _ in 0..6 {
                let mut spread =
                    || (engine.random.next_double() - engine.random.next_double()) * 4.0;
                let at = p.pos + DVec3::new(spread(), spread(), spread());
                let size = p.age as f64 / f64::from(p.lifetime.max(1));
                engine.spawn(
                    world,
                    &Type::Explosion.into(),
                    at,
                    DVec3::new(size, 0.0, 0.0),
                );
            }
            p.age += 1;
            if p.age == p.lifetime {
                p.removed = true;
            }
        }
        Kind::Spell {
            set,
            original_alpha,
        } => {
            p.base_tick(world);
            p.set_sprite_from_age(engine.sprites.get(set.name()));
            p.alpha += (original_alpha - p.alpha) * 0.05;
        }
        Kind::Cloud { set } => {
            p.base_tick(world);
            if !p.removed {
                p.set_sprite_from_age(engine.sprites.get(set.name()));
                if let Some((feet, vy)) = engine.player
                    && feet.distance(p.pos) <= 2.0
                    && p.pos.y > feet.y
                {
                    let y = p.pos.y + (feet.y - p.pos.y) * 0.2;
                    p.velocity.y += (vy - p.velocity.y) * 0.2;
                    p.set_pos(DVec3::new(p.pos.x, y, p.pos.z));
                }
            }
        }
        Kind::Portal { start } => {
            p.previous = p.pos;
            let age = p.age;
            p.age += 1;
            if age >= p.lifetime {
                p.removed = true;
                return;
            }
            let a = p.age as f32 / p.lifetime as f32;
            let pos = 1.0 - (-a + a * a * 2.0);
            p.pos = start + p.velocity * f64::from(pos) + DVec3::Y * f64::from(1.0 - a);
        }
        Kind::Fading { set, fade, free } => {
            if free {
                // `EndRodParticle.move`: through blocks.
                p.previous = p.pos;
                let age = p.age;
                p.age += 1;
                if age >= p.lifetime {
                    p.removed = true;
                } else {
                    p.velocity.y -= 0.04 * f64::from(p.gravity);
                    p.move_freely(p.velocity);
                    p.velocity *= f64::from(p.friction);
                }
            } else {
                p.base_tick(world);
            }
            p.set_sprite_from_age(engine.sprites.get(set.name()));
            if p.age > p.lifetime / 2 {
                p.alpha = 1.0 - (p.age as f32 - (p.lifetime / 2) as f32) / p.lifetime as f32;
                if let Some(fade) = fade {
                    for (channel, target) in p.rgb.iter_mut().zip(fade) {
                        *channel += (target - *channel) * 0.2;
                    }
                }
            }
        }
        Kind::Leaves(mut leaves) => {
            p.previous = p.pos;
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
                return;
            }
            let alive = (300 - p.lifetime) as f32;
            let relative = (alive / 300.0).min(1.0);
            let r = f64::from(relative);
            let (mut xa, mut za) = (0.0, 0.0);
            if leaves.flow_away {
                xa += leaves.flow.0 * r.powf(1.25);
                za += leaves.flow.1 * r.powf(1.25);
            }
            if leaves.swirl {
                xa += r * (r * leaves.swirl_period).cos() * f64::from(leaves.wind);
                za += r * (r * leaves.swirl_period).sin() * f64::from(leaves.wind);
            }
            p.velocity.x += xa * f64::from(0.0025f32);
            p.velocity.z += za * f64::from(0.0025f32);
            p.velocity.y -= f64::from(p.gravity);
            leaves.rot_speed += leaves.spin_acceleration / 20.0;
            p.previous_roll = p.roll;
            p.roll += leaves.rot_speed / 20.0;
            p.move_by(world, p.velocity);
            if p.on_ground || p.lifetime < 299 && (p.velocity.x == 0.0 || p.velocity.z == 0.0) {
                p.removed = true;
            }
            if !p.removed {
                p.velocity *= f64::from(p.friction);
            }
            p.kind = Kind::Leaves(leaves);
        }
        Kind::FallingDust { set, spin } => {
            p.previous = p.pos;
            let age = p.age;
            p.age += 1;
            if age >= p.lifetime {
                p.removed = true;
                return;
            }
            p.set_sprite_from_age(engine.sprites.get(set.name()));
            p.previous_roll = p.roll;
            p.roll += std::f32::consts::PI * spin * 2.0;
            if p.on_ground {
                p.roll = 0.0;
                p.previous_roll = 0.0;
            }
            p.move_by(world, p.velocity);
            p.velocity.y -= f64::from(0.003f32);
            p.velocity.y = p.velocity.y.max(f64::from(-0.14f32));
        }
        Kind::Glow { set } => {
            p.base_tick(world);
            p.set_sprite_from_age(engine.sprites.get(set.name()));
        }
        Kind::FlyTowards { start, .. } => {
            p.previous = p.pos;
            let age = p.age;
            p.age += 1;
            if age >= p.lifetime {
                p.removed = true;
                return;
            }
            let pos = 1.0 - p.age as f32 / p.lifetime as f32;
            let mut pp = 1.0 - pos;
            pp *= pp;
            pp *= pp;
            p.pos = start + p.velocity * f64::from(pos) - DVec3::Y * f64::from(pp * 1.2);
        }
        Kind::Breaking => p.base_tick(world),
        Kind::Wake => {
            p.previous = p.pos;
            let life_used = 60 - p.lifetime;
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
                return;
            }
            p.velocity.y -= f64::from(p.gravity);
            p.move_by(world, p.velocity);
            p.velocity *= f64::from(0.98f32);
            let size = life_used as f32 * 0.001;
            p.set_size(size, size);
            p.sprite = engine
                .sprites
                .get(Type::Fishing.name())
                .by_age(life_used % 4, 4);
        }
        Kind::Firefly => {
            p.base_tick(world);
            let inside = world
                .block(block_pos(p.pos))
                .is_some_and(|b| !matches!(b.id.path.as_str(), "air" | "cave_air" | "void_air"));
            if inside {
                p.removed = true;
                return;
            }
            let progress = (p.age as f32 / p.lifetime.max(1) as f32).clamp(0.0, 1.0);
            p.alpha = fade_amount(progress, 0.3, 0.5);
            if engine.random.next_float() > 0.95 || p.age == 1 {
                let mut drift = || f64::from(-0.05f32 + 0.1 * engine.random.next_float());
                p.velocity = DVec3::new(drift(), drift(), drift());
            }
        }
        Kind::CampfireSmoke => {
            p.previous = p.pos;
            let age = p.age;
            p.age += 1;
            if age < p.lifetime && p.alpha > 0.0 {
                let random = &mut engine.random;
                let mut sway = || {
                    let amount = f64::from(random.next_float() / 5000.0);
                    if random.next_bool() { amount } else { -amount }
                };
                p.velocity.x += sway();
                p.velocity.z += sway();
                p.velocity.y -= f64::from(p.gravity);
                p.move_by(world, p.velocity);
                if p.age >= p.lifetime - 60 && p.alpha > 0.01 {
                    p.alpha -= 0.015;
                }
            } else {
                p.removed = true;
            }
        }
    }
}

/// `Particle.tick` for kinds whose `move` ignores blocks.
fn free_tick(p: &mut Particle) {
    p.previous = p.pos;
    let age = p.age;
    p.age += 1;
    if age >= p.lifetime {
        p.removed = true;
        return;
    }
    p.velocity.y -= 0.04 * f64::from(p.gravity);
    p.move_freely(p.velocity);
    let friction = f64::from(p.friction);
    p.velocity *= friction;
}

fn drip_tick(
    p: &mut Particle,
    engine: &mut Particles,
    world: &World,
    fluid: Fluid,
    stage: DripStage,
) {
    p.previous = p.pos;
    // `preMoveUpdate`.
    match stage {
        DripStage::Hang { falls, cooling } => {
            if cooling {
                p.rgb = [
                    1.0,
                    16.0 / (40 - p.lifetime + 16) as f32,
                    4.0 / (40 - p.lifetime + 8) as f32,
                ];
            }
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
                engine.spawn(world, &falls.into(), p.pos, p.velocity);
            }
        }
        _ => {
            let life = p.lifetime;
            p.lifetime -= 1;
            if life <= 0 {
                p.removed = true;
            }
        }
    }
    if p.removed {
        return;
    }
    p.velocity.y -= f64::from(p.gravity);
    p.move_by(world, p.velocity);
    // `postMoveUpdate`.
    match stage {
        DripStage::Hang { .. } => p.velocity *= 0.02,
        DripStage::Fall { lands, sound } => {
            if p.on_ground {
                p.removed = true;
                if let Some(lands) = lands {
                    engine.spawn(world, &lands.into(), p.pos, DVec3::ZERO);
                }
                if let Some(sound) = sound {
                    let volume = engine.random.range(0.3, 1.0);
                    engine.sounds.push((sound, p.pos, volume, 1.0));
                }
            }
        }
        DripStage::Land => {}
    }
    if p.removed {
        return;
    }
    p.velocity *= f64::from(0.98f32);
    if fluid != Fluid::None {
        let at = block_pos(p.pos);
        if let Some((here, height)) = fluid_at(world, at)
            && here == fluid
            && p.pos.y < f64::from(at.1) + height
        {
            p.removed = true;
        }
    }
}
