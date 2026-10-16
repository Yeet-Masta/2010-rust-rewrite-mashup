//! The generic mobs' renderers: each kind's 26.3 model layer
//! (`entity_model`, a baby's own layer where it has one, else the adult's
//! at half size), its skin by its variant (`*Renderer.getTextureLocation`),
//! its `setupAnim` (ported per model below: the quadrupeds' leg swing,
//! the humanoids' and illagers' arms, fish tails, wings, tentacles, the
//! blaze's rods, the keyframe animations of the newer mobs), its renderer's
//! turn (`setupRotations`: a fish's wobble and its side ashore, a
//! phantom's pitch), full brightness for the glowing (`getBlockLightLevel`)
//! and their eyes layers.
use crate::{
    client_mobs::{ClientMobs, MobPose},
    entity_model::{self, Placement, Posed},
    lighting::SkyLight,
    mesh::{Atlas, ChunkMesh},
    pack::ResourceId,
};
use glam::{Quat, Vec3};
use minecraftoss_entities::species::Kind;
use minecraftoss_entities::world::MobEntity;
use std::f32::consts::PI;

/// `Mth.cos`, `Mth.sin` (float precision is plenty for poses).
fn cos(x: f32) -> f32 {
    x.cos()
}
fn sin(x: f32) -> f32 {
    x.sin()
}

/// `DyeColor.getTextureDiffuseColor`, by id.
const DYES: [u32; 16] = [0xF9FFFE, 0xF9801D, 0xC74EBD, 0x3AB3DA, 0xFED83D, 0x80C71F, 0xF38BAA, 0x474F52, 0x9D9D97, 0x169C9C, 0x8932B8, 0x3C44AA, 0x835432, 0x5E7C16, 0xB02E26, 0x1D1D21];
const DYE_NAMES: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];

fn dye(id: i64) -> [f32; 4] {
    let c = DYES[(id.clamp(0, 15)) as usize];
    [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0]
}

/// What one layer of a mob draws: its model layer, skin, tint, and
/// whether it glows at full brightness.
struct Layer {
    model: String,
    texture: String,
    tint: [f32; 4],
    glow: bool,
}

fn layer(model: &str, texture: &str) -> Layer {
    Layer { model: format!("minecraft:{model}#main"), texture: format!("minecraft:entity/{texture}"), tint: [1.0; 4], glow: false }
}

/// A variant index saved as a number.
fn index(variant: &str) -> i64 {
    variant.parse().unwrap_or(0)
}

/// `Panda.Gene.getVariantFromGenes`: a recessive main gene shows only when
/// both match (only the main gene is kept here).
fn panda_gene(variant: &str) -> &str {
    match variant {
        "lazy" | "worried" | "playful" | "aggressive" | "brown" | "weak" => variant,
        _ => "normal",
    }
}

/// The layers a mob draws, adult or baby, with whether the model is the
/// baby's own (else the adult's is drawn at half size).
fn layers(e: &MobEntity) -> (Vec<Layer>, bool) {
    let baby = e.baby();
    let v = e.variant.trim_start_matches("minecraft:");
    let b = if baby { "_baby" } else { "" };
    // The model layer, its baby layer when it has one.
    let pick = |adult: &str| -> String {
        if baby {
            let baby_layer = format!("{adult}_baby");
            if entity_model::model(&format!("minecraft:{baby_layer}#main")).is_some() {
                return baby_layer;
            }
        }
        adult.to_owned()
    };
    let own_baby = |name: &str| baby && entity_model::model(&format!("minecraft:{name}_baby#main")).is_some();
    let mut out = Vec::new();
    let mut baby_model = false;
    let add = |model: &str, texture: String, out: &mut Vec<Layer>| {
        let m = pick(model);
        out.push(Layer { model: format!("minecraft:{m}#main"), texture: format!("minecraft:entity/{texture}"), tint: [1.0; 4], glow: false });
    };
    match e.kind {
        Kind::Allay => out.push(Layer { glow: true, ..layer("allay", "allay/allay") }),
        Kind::Armadillo => {
            baby_model = own_baby("armadillo");
            add("armadillo", format!("armadillo/armadillo{b}"), &mut out);
        }
        Kind::Axolotl => {
            const COLOURS: [&str; 5] = ["lucy", "wild", "gold", "cyan", "blue"];
            baby_model = own_baby("axolotl");
            add("axolotl", format!("axolotl/axolotl_{}{b}", COLOURS[index(v).clamp(0, 4) as usize]), &mut out);
        }
        Kind::Bee => {
            baby_model = own_baby("bee");
            let angry = if e.aggressive() { "_angry" } else { "" };
            add("bee", format!("bee/bee{angry}{b}"), &mut out);
        }
        Kind::Blaze => out.push(Layer { glow: true, ..layer("blaze", "blaze/blaze") }),
        Kind::Breeze => {
            out.push(layer("breeze", "breeze/breeze"));
            out.push(Layer { model: "minecraft:breeze#eyes".into(), glow: true, ..layer("breeze", "breeze/breeze_eyes") });
        }
        Kind::Camel => {
            baby_model = own_baby("camel");
            add("camel", format!("camel/camel{b}"), &mut out);
        }
        Kind::CamelHusk => out.push(layer("camel", "camel/camel_husk")),
        Kind::Cat => {
            baby_model = own_baby("cat");
            let coat = if v.is_empty() { "tabby" } else { v };
            add("cat", format!("cat/cat_{coat}{b}"), &mut out);
        }
        Kind::CaveSpider => {
            out.push(layer("cave_spider", "spider/cave_spider"));
            out.push(Layer { glow: true, ..layer("cave_spider", "spider/spider_eyes") });
        }
        Kind::Cod => out.push(layer("cod", "fish/cod")),
        Kind::CopperGolem => {
            let age = match v {
                "exposed" | "weathered" | "oxidized" => format!("_{v}"),
                _ => String::new(),
            };
            out.push(layer("copper_golem", &format!("copper_golem/copper_golem{age}")));
            out.push(Layer { model: "minecraft:copper_golem#eyes".into(), glow: true, ..layer("copper_golem", &format!("copper_golem/copper_golem_eyes{age}")) });
        }
        Kind::Creaking => {
            out.push(layer("creaking", "creaking/creaking"));
            if e.aggressive() {
                out.push(Layer { model: "minecraft:creaking#eyes".into(), glow: true, ..layer("creaking", "creaking/creaking_eyes") });
            }
        }
        Kind::Dolphin => {
            baby_model = own_baby("dolphin");
            add("dolphin", format!("dolphin/dolphin{b}"), &mut out);
        }
        Kind::ElderGuardian => out.push(layer("elder_guardian", "guardian/guardian_elder")),
        Kind::Endermite => out.push(layer("endermite", "endermite/endermite")),
        Kind::EnderDragon => {
            out.push(layer("ender_dragon", "enderdragon/dragon"));
            out.push(Layer { glow: true, ..layer("ender_dragon", "enderdragon/dragon_eyes") });
        }
        Kind::Evoker => out.push(layer("evoker", "illager/evoker")),
        Kind::Fox => {
            baby_model = own_baby("fox");
            let snow = if v == "snow" { "_snow" } else { "" };
            add("fox", format!("fox/fox{snow}{b}"), &mut out);
        }
        Kind::Frog => {
            let kind = if v.is_empty() { "temperate" } else { v };
            out.push(layer("frog", &format!("frog/frog_{kind}")));
        }
        Kind::Ghast => out.push(layer("ghast", if e.charging() { "ghast/ghast_shooting" } else { "ghast/ghast" })),
        Kind::HappyGhast => {
            baby_model = own_baby("happy_ghast");
            add("happy_ghast", format!("ghast/happy_ghast{b}"), &mut out);
        }
        Kind::Giant => out.push(layer("giant", "zombie/zombie")),
        Kind::GlowSquid => {
            baby_model = own_baby("glow_squid");
            let mut l = Layer { glow: true, ..layer("glow_squid", "squid/glow_squid") };
            l.model = format!("minecraft:{}#main", pick("glow_squid"));
            l.texture = format!("minecraft:entity/squid/glow_squid{b}");
            out.push(l);
        }
        Kind::Goat => {
            baby_model = own_baby("goat");
            add("goat", format!("goat/goat{b}"), &mut out);
        }
        Kind::Guardian => out.push(layer("guardian", "guardian/guardian")),
        Kind::Hoglin => {
            baby_model = own_baby("hoglin");
            add("hoglin", format!("hoglin/hoglin{b}"), &mut out);
        }
        Kind::Illusioner => out.push(layer("illusioner", "illager/illusioner")),
        Kind::Llama | Kind::TraderLlama => {
            const COATS: [&str; 4] = ["creamy", "white", "brown", "gray"];
            let model = if e.kind == Kind::TraderLlama { "trader_llama" } else { "llama" };
            baby_model = own_baby(model);
            add(model, format!("llama/llama_{}{b}", COATS[index(v).clamp(0, 3) as usize]), &mut out);
        }
        Kind::MagmaCube => out.push(Layer { glow: true, ..layer("magma_cube", "slime/magmacube") }),
        Kind::Mule => {
            baby_model = own_baby("mule");
            add("mule", format!("horse/mule{b}"), &mut out);
        }
        Kind::Nautilus => {
            baby_model = own_baby("nautilus");
            add("nautilus", format!("nautilus/nautilus{b}"), &mut out);
        }
        Kind::ZombieNautilus => out.push(layer("zombie_nautilus", "nautilus/zombie_nautilus")),
        Kind::Ocelot => {
            baby_model = own_baby("ocelot");
            add("ocelot", format!("cat/ocelot{b}"), &mut out);
        }
        Kind::Panda => {
            baby_model = own_baby("panda");
            let gene = panda_gene(v);
            let texture = match (gene, baby) {
                ("normal", false) => "panda/panda".to_owned(),
                ("normal", true) => "panda/panda_baby".to_owned(),
                (g, false) => format!("panda/panda_{g}"),
                (g, true) => format!("panda/{g}_panda_baby"),
            };
            add("panda", texture, &mut out);
        }
        Kind::Parrot => {
            const COLOURS: [&str; 5] = ["red_blue", "blue", "green", "yellow_blue", "grey"];
            out.push(layer("parrot", &format!("parrot/parrot_{}", COLOURS[index(v).clamp(0, 4) as usize])));
        }
        Kind::Phantom => {
            out.push(layer("phantom", "phantom/phantom"));
            out.push(Layer { glow: true, ..layer("phantom", "phantom/phantom_eyes") });
        }
        Kind::Piglin => {
            baby_model = own_baby("piglin");
            add("piglin", format!("piglin/piglin{b}"), &mut out);
        }
        Kind::PiglinBrute => out.push(layer("piglin_brute", "piglin/piglin_brute")),
        Kind::ZombifiedPiglin => {
            baby_model = own_baby("zombified_piglin");
            add("zombified_piglin", format!("piglin/zombified_piglin{b}"), &mut out);
        }
        Kind::Pillager => out.push(layer("pillager", "illager/pillager")),
        Kind::PolarBear => {
            baby_model = own_baby("polar_bear");
            add("polar_bear", format!("bear/polarbear{b}"), &mut out);
        }
        // `PufferfishRenderer`: a model for each puff state.
        Kind::Pufferfish => out.push(layer(["pufferfish_small", "pufferfish_medium", "pufferfish_big"][e.puff.clamp(0, 2) as usize], "fish/pufferfish")),
        Kind::Rabbit => {
            let coat = match index(v) {
                1 => "white",
                2 => "black",
                3 => "white_splotched",
                4 => "gold",
                5 => "salt",
                99 => "caerbannog",
                _ => "brown",
            };
            baby_model = own_baby("rabbit");
            add("rabbit", format!("rabbit/rabbit_{coat}{b}"), &mut out);
        }
        Kind::Ravager => out.push(layer("ravager", "illager/ravager")),
        Kind::Salmon => {
            let model = match v {
                "small" => "salmon_small",
                "large" => "salmon_large",
                _ => "salmon",
            };
            out.push(layer(model, "fish/salmon"));
        }
        Kind::Shulker => {
            let colour = index(v);
            let texture = if (0..16).contains(&colour) && !v.is_empty() { format!("shulker/shulker_{}", DYE_NAMES[colour as usize]) } else { "shulker/shulker".to_owned() };
            out.push(layer("shulker", &texture));
        }
        Kind::Silverfish => out.push(layer("silverfish", "silverfish/silverfish")),
        Kind::SkeletonHorse => {
            baby_model = own_baby("skeleton_horse");
            add("skeleton_horse", format!("horse/horse_skeleton{b}"), &mut out);
        }
        Kind::ZombieHorse => {
            baby_model = own_baby("zombie_horse");
            add("zombie_horse", format!("horse/horse_zombie{b}"), &mut out);
        }
        Kind::Sniffer => {
            baby_model = own_baby("sniffer");
            add("sniffer", if baby { "sniffer/snifflet".to_owned() } else { "sniffer/sniffer".to_owned() }, &mut out);
        }
        Kind::SnowGolem => out.push(layer("snow_golem", "snow_golem/snow_golem")),
        Kind::Squid => {
            baby_model = own_baby("squid");
            add("squid", format!("squid/squid{b}"), &mut out);
        }
        Kind::Strider => {
            baby_model = own_baby("strider");
            // `StriderRenderer`: purple with cold out of lava.
            let cold = if e.cold { "_cold" } else { "" };
            add("strider", format!("strider/strider{cold}{b}"), &mut out);
        }
        Kind::SulfurCube => out.push(layer("sulfur_cube", "sulfur_cube/sulfur_cube_outer")),
        Kind::Tadpole => out.push(layer("tadpole", "tadpole/tadpole")),
        Kind::TropicalFish => {
            // `TropicalFish.Pattern`: the shape, pattern and two colours
            // packed into its variant.
            let packed = index(v);
            let large = packed & 0xff == 1;
            let pattern = ((packed >> 8) & 0xff).clamp(0, 5) + 1;
            let (model, shape) = if large { ("tropical_fish_large", "b") } else { ("tropical_fish_small", "a") };
            out.push(Layer { tint: dye((packed >> 16) & 0xff), ..layer(model, &format!("fish/tropical_{shape}")) });
            out.push(Layer { model: format!("minecraft:{model}#pattern"), tint: dye((packed >> 24) & 0xff), ..layer(model, &format!("fish/tropical_{shape}_pattern_{pattern}")) });
        }
        Kind::Turtle => {
            baby_model = own_baby("turtle");
            add("turtle", format!("turtle/turtle{b}"), &mut out);
        }
        // `VexRenderer`: the charging face while it charges.
        Kind::Vex => out.push(Layer { glow: true, ..layer("vex", if e.charging() { "illager/vex_charging" } else { "illager/vex" }) }),
        Kind::Vindicator => out.push(layer("vindicator", "illager/vindicator")),
        Kind::WanderingTrader => out.push(layer("wandering_trader", "wandering_trader/wandering_trader")),
        Kind::Warden => out.push(layer("warden", "warden/warden")),
        // `WitherBossRenderer.getTextureLocation`: the pale skin while it
        // gathers itself, blinking over its last four seconds.
        Kind::Wither => out.push(layer("wither", if e.wither_spawn_look(0.0).1 { "wither/wither_invulnerable" } else { "wither/wither" })),
        Kind::WitherSkeleton => out.push(layer("wither_skeleton", "skeleton/wither_skeleton")),
        Kind::Zoglin => {
            baby_model = own_baby("zoglin");
            add("zoglin", format!("hoglin/zoglin{b}"), &mut out);
        }
    }
    (out, baby_model)
}

/// Every skin the generic mobs may draw, for the atlas.
pub fn textures() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |t: &str| out.push(format!("minecraft:entity/{t}"));
    for t in [
        "allay/allay", "armadillo/armadillo", "armadillo/armadillo_baby", "blaze/blaze", "breeze/breeze", "breeze/breeze_eyes", "camel/camel", "camel/camel_baby", "camel/camel_husk",
        "spider/cave_spider", "fish/cod", "creaking/creaking", "creaking/creaking_eyes", "dolphin/dolphin", "dolphin/dolphin_baby", "guardian/guardian_elder", "guardian/guardian", "endermite/endermite",
        "enderdragon/dragon", "enderdragon/dragon_eyes", "illager/evoker", "illager/illusioner", "illager/pillager", "illager/vindicator", "illager/ravager", "illager/vex", "illager/vex_charging",
        "fox/fox", "fox/fox_snow", "fox/fox_baby", "fox/fox_snow_baby", "frog/frog_temperate", "frog/frog_warm", "frog/frog_cold", "ghast/ghast", "ghast/ghast_shooting", "ghast/happy_ghast", "ghast/happy_ghast_baby",
        "zombie/zombie", "squid/glow_squid", "squid/glow_squid_baby", "squid/squid", "squid/squid_baby", "goat/goat", "goat/goat_baby", "hoglin/hoglin", "hoglin/hoglin_baby", "hoglin/zoglin", "hoglin/zoglin_baby",
        "slime/magmacube", "horse/mule", "horse/mule_baby", "horse/horse_skeleton", "horse/horse_skeleton_baby", "horse/horse_zombie", "horse/horse_zombie_baby", "nautilus/nautilus", "nautilus/nautilus_baby", "nautilus/zombie_nautilus",
        "cat/ocelot", "cat/ocelot_baby", "phantom/phantom", "phantom/phantom_eyes", "piglin/piglin", "piglin/piglin_baby", "piglin/piglin_brute", "piglin/zombified_piglin", "piglin/zombified_piglin_baby",
        "bear/polarbear", "bear/polarbear_baby", "fish/pufferfish", "fish/salmon", "silverfish/silverfish", "sniffer/sniffer", "sniffer/snifflet", "snow_golem/snow_golem", "strider/strider", "strider/strider_baby", "strider/strider_cold", "strider/strider_cold_baby",
        "sulfur_cube/sulfur_cube_outer", "tadpole/tadpole", "fish/tropical_a", "fish/tropical_b", "turtle/turtle", "turtle/turtle_baby", "wandering_trader/wandering_trader", "warden/warden", "wither/wither", "wither/wither_invulnerable",
        "skeleton/wither_skeleton", "shulker/shulker",
    ] {
        add(t);
    }
    for c in ["lucy", "wild", "gold", "cyan", "blue"] {
        add(&format!("axolotl/axolotl_{c}"));
        add(&format!("axolotl/axolotl_{c}_baby"));
    }
    for angry in ["", "_angry"] {
        for baby in ["", "_baby"] {
            add(&format!("bee/bee{angry}{baby}"));
        }
    }
    for coat in ["tabby", "black", "red", "siamese", "british_shorthair", "calico", "persian", "ragdoll", "white", "jellie", "all_black"] {
        add(&format!("cat/cat_{coat}"));
        add(&format!("cat/cat_{coat}_baby"));
    }
    for age in ["", "_exposed", "_weathered", "_oxidized"] {
        add(&format!("copper_golem/copper_golem{age}"));
        add(&format!("copper_golem/copper_golem_eyes{age}"));
    }
    for coat in ["creamy", "white", "brown", "gray"] {
        add(&format!("llama/llama_{coat}"));
        add(&format!("llama/llama_{coat}_baby"));
    }
    add("panda/panda");
    add("panda/panda_baby");
    for gene in ["lazy", "worried", "playful", "aggressive", "brown", "weak"] {
        add(&format!("panda/panda_{gene}"));
        add(&format!("panda/{gene}_panda_baby"));
    }
    for c in ["red_blue", "blue", "green", "yellow_blue", "grey"] {
        add(&format!("parrot/parrot_{c}"));
    }
    for coat in ["brown", "white", "black", "white_splotched", "gold", "salt", "caerbannog"] {
        add(&format!("rabbit/rabbit_{coat}"));
        add(&format!("rabbit/rabbit_{coat}_baby"));
    }
    for d in DYE_NAMES {
        add(&format!("shulker/shulker_{d}"));
    }
    for shape in ["a", "b"] {
        for p in 1..=6 {
            add(&format!("fish/tropical_{shape}_pattern_{p}"));
        }
    }
    out
}

/// `EntityRenderer.shadowRadius` for a generic kind.
pub fn shadow_radius(kind: Kind) -> f32 {
    match kind {
        Kind::Ghast | Kind::HappyGhast => 1.5,
        Kind::Wither => 1.0,
        Kind::Ravager => 1.1,
        Kind::Warden => 0.9,
        Kind::Phantom => 0.75,
        Kind::Giant => 3.0,
        Kind::CaveSpider => 0.56,
        Kind::Shulker | Kind::EnderDragon => 0.0,
        Kind::Endermite | Kind::Silverfish | Kind::Cod | Kind::Frog | Kind::Parrot | Kind::Vex => 0.3,
        Kind::Tadpole => 0.14,
        Kind::TropicalFish => 0.15,
        Kind::Pufferfish => 0.2,
        Kind::Allay | Kind::Bee | Kind::Fox | Kind::Cat | Kind::Salmon => 0.4,
        _ => (kind.species().size[0] * 0.5).clamp(0.3, 1.0),
    }
}

/// The quadrupeds' legs (`QuadrupedModel.setupAnim`): opposite corners
/// together, `amplitude` 1.4 by default.
fn quadruped_legs(posed: &mut Posed, pos: f32, speed: f32, amplitude: f32) {
    let a = cos(pos * 0.6662) * amplitude * speed;
    let b = cos(pos * 0.6662 + PI) * amplitude * speed;
    posed.with("right_hind_leg", |p| p.rotation.x = a);
    posed.with("left_hind_leg", |p| p.rotation.x = b);
    posed.with("right_front_leg", |p| p.rotation.x = b);
    posed.with("left_front_leg", |p| p.rotation.x = a);
}

/// `AnimationUtils.bobModelPart`.
fn bob(posed: &mut Posed, part: &str, age: f32, sign: f32) {
    posed.with(part, |p| {
        p.rotation.z += sign * (cos(age * 0.09) * 0.05 + 0.05);
        p.rotation.x += sign * sin(age * 0.067) * 0.05;
    });
}

/// `HumanoidModel.setupAnim`'s walk, with the attack swing and arm bob;
/// `zombie_arms` raises them (`animateZombieArms`) when aggressive.
fn humanoid(posed: &mut Posed, m: &MobPose, aggressive: bool, zombie_arms: bool) {
    let (pos, speed) = (m.walk_position, m.walk_speed);
    posed.look("head", m.head_yaw, m.head_pitch);
    posed.with("right_arm", |p| p.rotation.x = cos(pos * 0.6662 + PI) * 2.0 * speed * 0.5);
    posed.with("left_arm", |p| p.rotation.x = cos(pos * 0.6662) * 2.0 * speed * 0.5);
    posed.with("right_leg", |p| p.rotation = Vec3::new(cos(pos * 0.6662) * 1.4 * speed, 0.005, 0.005));
    posed.with("left_leg", |p| p.rotation = Vec3::new(cos(pos * 0.6662 + PI) * 1.4 * speed, -0.005, -0.005));
    let swing = m.swing.unwrap_or(0.0);
    if zombie_arms {
        let drop = -PI / if aggressive { 1.5 } else { 2.25 };
        let attack = sin(swing * PI);
        let lift = sin((1.0 - (1.0 - swing) * (1.0 - swing)) * PI);
        let x = drop + attack * 1.2 - lift * 0.4;
        posed.with("right_arm", |p| p.rotation = Vec3::new(x, -(0.1 - attack * 0.6), 0.0));
        posed.with("left_arm", |p| p.rotation = Vec3::new(x, 0.1 - attack * 0.6, 0.0));
    } else if swing > 0.0 {
        // `setupAttackAnimation`: the right arm swings down.
        let body = sin(swing.sqrt() * PI * 2.0) * 0.2;
        let attack = sin((1.0 - (1.0 - swing).powi(4)) * PI);
        let head_x = posed.get("head").map_or(0.0, |h| h.rotation.x);
        posed.with("body", |p| p.rotation.y = body);
        posed.with("right_arm", |p| {
            p.rotation.x -= attack * 1.2 + sin(swing * PI) * -(head_x - 0.7) * 0.75;
            p.rotation.y += body * 2.0;
            p.rotation.z += sin(swing * PI) * -0.4;
        });
        posed.with("left_arm", |p| p.rotation.y += body);
    }
    bob(posed, "right_arm", m.age_in_ticks, 1.0);
    bob(posed, "left_arm", m.age_in_ticks, -1.0);
}

/// `IllagerModel.setupAnim`: the walk, crossed arms at rest, the arms
/// raised to attack, cast or aim.
fn illager(posed: &mut Posed, m: &MobPose, kind: Kind, aggressive: bool, charging: bool) {
    let (pos, speed) = (m.walk_position, m.walk_speed);
    posed.look("head", m.head_yaw, m.head_pitch);
    posed.with("right_arm", |p| p.rotation = Vec3::new(cos(pos * 0.6662 + PI) * 2.0 * speed * 0.5, 0.0, 0.0));
    posed.with("left_arm", |p| p.rotation = Vec3::new(cos(pos * 0.6662) * 2.0 * speed * 0.5, 0.0, 0.0));
    posed.with("right_leg", |p| p.rotation = Vec3::new(cos(pos * 0.6662) * 1.4 * speed * 0.5, 0.0, 0.0));
    posed.with("left_leg", |p| p.rotation = Vec3::new(cos(pos * 0.6662 + PI) * 1.4 * speed * 0.5, 0.0, 0.0));
    let age = m.age_in_ticks;
    let crossed = !aggressive && !charging;
    if charging && matches!(kind, Kind::Evoker | Kind::Illusioner) {
        // `SPELLCASTING`.
        posed.with("right_arm", |p| {
            p.offset.x = -5.0;
            p.offset.z = 0.0;
            p.rotation = Vec3::new(cos(age * 0.6662) * 0.25, 0.0, PI * 3.0 / 4.0);
        });
        posed.with("left_arm", |p| {
            p.offset.x = 5.0;
            p.offset.z = 0.0;
            p.rotation = Vec3::new(cos(age * 0.6662) * 0.25, 0.0, -PI * 3.0 / 4.0);
        });
    } else if aggressive && kind == Kind::Pillager {
        // `CROSSBOW_HOLD` (`AnimationUtils.animateCrossbowHold`).
        let head = posed.get("head").map_or(Vec3::ZERO, |h| h.rotation);
        posed.with("right_arm", |p| p.rotation = Vec3::new(-PI / 2.0 + head.x + 0.1, -0.3 + head.y, 0.0));
        posed.with("left_arm", |p| p.rotation = Vec3::new(-1.5 + head.x, 0.6 + head.y, 0.0));
    } else if aggressive && kind == Kind::Illusioner {
        // `BOW_AND_ARROW`.
        let head = posed.get("head").map_or(Vec3::ZERO, |h| h.rotation);
        posed.with("right_arm", |p| p.rotation = Vec3::new(-PI / 2.0 + head.x, -0.1 + head.y, 0.0));
        posed.with("left_arm", |p| p.rotation = Vec3::new(-0.942_477_9 + head.x, head.y - 0.4, PI / 2.0));
    } else if aggressive {
        // `ATTACKING` with a weapon: `AnimationUtils.swingWeaponDown`.
        let swing = m.swing.unwrap_or(0.0);
        let a2 = sin(swing * PI);
        let a = sin((1.0 - (1.0 - swing) * (1.0 - swing)) * PI);
        posed.with("right_arm", |p| p.rotation = Vec3::new(-1.884_955_8 + cos(age * 0.09) * 0.15 + a2 * 2.2 - a * 0.4, PI / 20.0, 0.0));
        posed.with("left_arm", |p| p.rotation = Vec3::new(cos(age * 0.19) * 0.5 + a2 * 1.2 - a * 0.4, -PI / 20.0, 0.0));
    }
    // A wandering trader always crosses its arms (`VillagerModel`).
    if kind == Kind::WanderingTrader {
        return;
    }
    if crossed {
        posed.hide("left_arm");
        posed.hide("right_arm");
    } else {
        posed.hide("arms");
    }
}

/// A fish's tail beat (`CodModel`, `TropicalFish*Model`, `TadpoleModel`).
fn fish_tail(posed: &mut Posed, part: &str, age: f32, in_water: bool, amplitude: f32, frequency: f32) {
    let multiplier = if in_water { 1.0 } else { 1.5 };
    posed.with(part, |p| p.rotation.y = -multiplier * amplitude * sin(frequency * age));
}

/// The vanilla walk keyframes, from the walk.
fn walk_keyframes(posed: &mut Posed, name: &str, m: &MobPose, speed_factor: f32, scale_factor: f32) {
    if let Some(a) = entity_model::animation(name) {
        a.apply_walk(posed, m.walk_position, m.walk_speed, speed_factor, scale_factor);
    }
}

/// A looping keyframe animation, running since the mob's first tick.
fn idle_keyframes(posed: &mut Posed, name: &str, age: f32) {
    if let Some(a) = entity_model::animation(name) {
        a.apply(posed, age * 50.0, 1.0);
    }
}

/// `setupAnim` for a generic mob.
fn animate(posed: &mut Posed, e: &MobEntity, m: &MobPose) {
    let (pos, speed, age) = (m.walk_position, m.walk_speed, m.age_in_ticks);
    let in_water = e.body.touching_water;
    let aggressive = e.aggressive();
    match e.kind {
        Kind::Armadillo => {
            posed.look("head", m.head_yaw.clamp(-32.5, 32.5), m.head_pitch.clamp(-22.5, 25.0));
            walk_keyframes(posed, if e.baby() { "BabyArmadilloAnimation.ARMADILLO_BABY_WALK" } else { "ArmadilloAnimation.ARMADILLO_WALK" }, m, 16.5, 2.5);
        }
        Kind::Camel | Kind::CamelHusk => {
            posed.look("head", m.head_yaw.clamp(-30.0, 30.0), m.head_pitch.clamp(-25.0, 45.0));
            let baby = e.baby() && e.kind == Kind::Camel;
            walk_keyframes(posed, if baby { "CamelBabyAnimation.CAMEL_BABY_WALK" } else { "CamelAnimation.CAMEL_WALK" }, m, 2.0, 2.5);
            if speed < 0.01 {
                idle_keyframes(posed, if baby { "CamelBabyAnimation.CAMEL_BABY_IDLE" } else { "CamelAnimation.CAMEL_IDLE" }, age);
            }
        }
        Kind::Sniffer => {
            posed.look("head", m.head_yaw, m.head_pitch);
            walk_keyframes(posed, "SnifferAnimation.SNIFFER_WALK", m, 9.0, 100.0);
        }
        Kind::Frog => {
            // `FrogModel.setupAnim`: the tongue and croak play from when
            // they began; the throat sac shows only while it croaks.
            let since = |start: i32| ((e.tick_count - start) as f32 + m.age_in_ticks.fract()).max(0.0) * 50.0;
            if let (Some(start), Some(a)) = (e.frog.croak_start, entity_model::animation("FrogAnimation.FROG_CROAK")) {
                a.apply(posed, since(start), 1.0);
            } else {
                posed.hide("croaking_body");
            }
            if let (Some(start), Some(a)) = (e.frog.tongue_start, entity_model::animation("FrogAnimation.FROG_TONGUE")) {
                a.apply(posed, since(start), 1.0);
            }
            if in_water {
                walk_keyframes(posed, "FrogAnimation.FROG_SWIM", m, 1.0, 2.5);
                idle_keyframes(posed, "FrogAnimation.FROG_IDLE_WATER", age);
            } else {
                walk_keyframes(posed, "FrogAnimation.FROG_WALK", m, 1.5, 2.5);
            }
        }
        Kind::Creaking => {
            posed.look("head", m.head_yaw, m.head_pitch);
            walk_keyframes(posed, "CreakingAnimation.CREAKING_WALK", m, 1.0, 1.0);
        }
        Kind::CopperGolem => {
            posed.look("head", m.head_yaw, m.head_pitch);
            walk_keyframes(posed, "CopperGolemAnimation.COPPER_GOLEM_WALK", m, 2.0, 2.5);
            if speed < 0.01 {
                idle_keyframes(posed, "CopperGolemAnimation.COPPER_GOLEM_IDLE", age);
            }
        }
        Kind::Breeze => idle_keyframes(posed, "BreezeAnimation.IDLE", age),
        Kind::Nautilus | Kind::ZombieNautilus => {
            if let Some(a) = entity_model::animation("NautilusAnimation.SWIMMING") {
                a.apply_walk(posed, pos + age / 5.0, speed + 0.2, 2.0, 3.0);
            }
            posed.with("body", |p| {
                p.rotation.y = m.head_yaw.clamp(-10.0, 10.0).to_radians();
                p.rotation.x = m.head_pitch.clamp(-10.0, 10.0).to_radians();
            });
        }
        Kind::Rabbit => {
            posed.look("head", m.head_yaw, m.head_pitch);
            // `Rabbit.hopAnimationState` runs while it moves.
            if speed > 0.05 {
                idle_keyframes(posed, if e.baby() { "BabyRabbitAnimation.HOP" } else { "RabbitAnimation.HOP" }, age);
            }
        }
        Kind::Warden => {
            posed.look("head", m.head_yaw, m.head_pitch);
            // `WardenModel.animateWalk`.
            let s = speed.min(0.5) * 0.5;
            let (c, sn) = (cos(pos * 0.8) * s, sin(pos * 0.8) * s);
            posed.with("head", |p| {
                p.rotation.z = 0.3 * sin(pos * 0.4) * s;
                p.rotation.x += 1.2 * cos(pos * 0.4 + PI / 2.0) * s;
            });
            posed.with("body", |p| {
                p.rotation.z = 0.1 * sin(pos * 0.4) * s;
                p.rotation.x = 0.5 * c;
            });
            posed.with("left_leg", |p| p.rotation.x = 1.0 * c);
            posed.with("right_leg", |p| p.rotation.x = 1.0 * cos(pos * 0.8 + PI) * s);
            posed.with("left_arm", |p| p.rotation.x = -(0.8 * c));
            posed.with("right_arm", |p| p.rotation.x = -(0.8 * sn));
            let tendril = cos(age * 0.25);
            posed.with("left_tendril", |p| p.rotation.x = tendril * PI * 0.1);
            posed.with("right_tendril", |p| p.rotation.x = -tendril * PI * 0.1);
        }
        // `QuadrupedModel`s.
        Kind::PolarBear | Kind::Panda | Kind::Goat => {
            posed.look("head", m.head_yaw, m.head_pitch);
            quadruped_legs(posed, pos, speed, 1.4);
        }
        Kind::Turtle => {
            posed.look("head", m.head_yaw, m.head_pitch);
            if in_water {
                let swing = cos(pos * 0.6662 * 0.6) * 0.5 * speed;
                posed.with("right_hind_leg", |p| p.rotation.x = swing);
                posed.with("left_hind_leg", |p| p.rotation.x = -swing);
                posed.with("right_front_leg", |p| p.rotation.z = -swing);
                posed.with("left_front_leg", |p| p.rotation.z = swing);
            } else {
                let (front, hind) = (cos(pos * 5.0), cos(pos * 5.0));
                posed.with("right_front_leg", |p| p.rotation.y = -front * 8.0 * speed);
                posed.with("left_front_leg", |p| p.rotation.y = front * 8.0 * speed);
                posed.with("right_hind_leg", |p| p.rotation.y = -hind * 3.0 * speed);
                posed.with("left_hind_leg", |p| p.rotation.y = hind * 3.0 * speed);
            }
        }
        Kind::Llama | Kind::TraderLlama => {
            posed.look("head", m.head_yaw, m.head_pitch);
            quadruped_legs(posed, pos, speed, 1.4);
            posed.hide("right_chest");
            posed.hide("left_chest");
        }
        Kind::Hoglin | Kind::Zoglin => {
            posed.with("right_ear", |p| p.rotation.z = -PI * 2.0 / 9.0 - speed * sin(pos));
            posed.with("left_ear", |p| p.rotation.z = PI * 2.0 / 9.0 + speed * sin(pos));
            posed.with("head", |p| p.rotation.y = m.head_yaw.to_radians());
            let a = cos(pos) * 1.2 * speed;
            let b = cos(pos + PI) * 1.2 * speed;
            posed.with("right_front_leg", |p| p.rotation.x = a);
            posed.with("left_front_leg", |p| p.rotation.x = b);
            posed.with("right_hind_leg", |p| p.rotation.x = b);
            posed.with("left_hind_leg", |p| p.rotation.x = a);
        }
        Kind::Ravager => {
            // `RavagerModel.setupAnim`: the neck lunges and the mouth gapes
            // with the bite, the stunned head droops and sways, the roar
            // opens the mouth wide.
            let r = e.ravager;
            let partial = m.age_in_ticks.fract();
            let attack = if r.attack > 0 { r.attack as f32 - partial } else { 0.0 };
            let stunned = if r.stun > 0 { r.stun as f32 - partial } else { 0.0 };
            let roar = if r.roar > 0 { (20.0 - r.roar as f32 + partial) / 20.0 } else { 0.0 };
            if attack > 0.0 {
                // `Mth.triangleWave(attack, 10)`.
                let wave = (((attack % 10.0) - 5.0).abs() - 2.5) / 2.5;
                let scaled = (1.0 + wave) * 0.5;
                let lunge = scaled * scaled * scaled * 12.0;
                posed.with("neck", |p| {
                    let y_offset = lunge * p.rotation.x.sin();
                    p.offset.z = -6.5 + lunge;
                    p.offset.y = -7.0 - y_offset;
                });
                let mouth = if attack > 5.0 { ((-4.0 + attack) / 4.0).sin() * PI * 0.4 } else { PI / 20.0 * (PI * attack / 10.0).sin() };
                posed.with("mouth", |p| p.rotation.x = mouth);
            } else {
                let stun = stunned > 0.0;
                posed.with("neck", |p| {
                    p.rotation.x = if stun { 0.219_911_49 } else { 0.0 };
                    p.offset = Vec3::new(if stun { ((f64::from(stunned) / 40.0 * 10.0).sin() * 3.0) as f32 } else { 0.0 }, -7.0, 5.5);
                });
                let mouth = if stun { PI * 0.05 } else if roar > 0.0 { PI / 2.0 * (roar * PI * 0.25).sin() } else { PI * 0.01 };
                posed.with("mouth", |p| p.rotation.x = mouth);
            }
            posed.look("head", m.head_yaw, m.head_pitch);
            let leg = 0.4 * speed;
            posed.with("right_hind_leg", |p| p.rotation.x = cos(pos * 0.6662) * leg);
            posed.with("left_hind_leg", |p| p.rotation.x = cos(pos * 0.6662 + PI) * leg);
            posed.with("right_front_leg", |p| p.rotation.x = cos(pos * 0.6662 + PI) * leg);
            posed.with("left_front_leg", |p| p.rotation.x = cos(pos * 0.6662) * leg);
        }
        Kind::Mule | Kind::SkeletonHorse | Kind::ZombieHorse => {
            // `AbstractEquineModel.setupAnim` at a walk.
            let head_x = m.head_pitch.to_radians() + if speed > 0.2 { cos(pos * 0.8) * 0.15 * speed } else { 0.0 };
            posed.with("head_parts", |p| {
                p.rotation.x = PI / 6.0 + head_x;
                p.rotation.y = m.head_yaw.clamp(-20.0, 20.0).to_radians();
            });
            let water = if in_water { 0.2 } else { 1.0 };
            let leg = cos(water * pos * 0.6662 + PI);
            let x = leg * 0.8 * speed;
            posed.with("left_hind_leg", |p| p.rotation.x = -leg * 0.5 * speed);
            posed.with("right_hind_leg", |p| p.rotation.x = leg * 0.5 * speed);
            posed.with("left_front_leg", |p| p.rotation.x = x);
            posed.with("right_front_leg", |p| p.rotation.x = -x);
            posed.with("tail", |p| p.rotation.x = PI / 6.0 + speed * 0.75);
            posed.hide("left_chest");
            posed.hide("right_chest");
        }
        Kind::Cat | Kind::Ocelot => {
            // `AdultFelineModel.setupAnim`, walking.
            posed.look("head", m.head_yaw, m.head_pitch);
            posed.with("body", |p| p.rotation.x = PI / 2.0);
            let a = cos(pos * 0.6662) * speed;
            let b = cos(pos * 0.6662 + PI) * speed;
            posed.with("left_hind_leg", |p| p.rotation.x = a);
            posed.with("right_hind_leg", |p| p.rotation.x = b);
            posed.with("left_front_leg", |p| p.rotation.x = b);
            posed.with("right_front_leg", |p| p.rotation.x = a);
            posed.with("tail2", |p| p.rotation.x = 1.727_876_1 + PI / 4.0 * cos(pos) * speed);
            // `isSitting`.
            if e.sitting() {
                posed.with("body", |p| {
                    p.rotation.x = PI / 4.0;
                    p.offset += Vec3::new(0.0, -4.0, 5.0);
                });
                posed.with("head", |p| p.offset += Vec3::new(0.0, -3.3, 1.0));
                posed.with("tail1", |p| {
                    p.offset += Vec3::new(0.0, 8.0, -2.0);
                    p.rotation.x = 1.727_876_1;
                });
                posed.with("tail2", |p| {
                    p.offset += Vec3::new(0.0, 2.0, -0.8);
                    p.rotation.x = 2.670_354;
                });
                for leg in ["left_front_leg", "right_front_leg"] {
                    posed.with(leg, |p| {
                        p.rotation.x = -PI / 20.0;
                        p.offset += Vec3::new(0.0, 2.0, -2.0);
                    });
                }
                for leg in ["left_hind_leg", "right_hind_leg"] {
                    posed.with(leg, |p| {
                        p.rotation.x = -PI / 2.0;
                        p.offset += Vec3::new(0.0, 3.0, -4.0);
                    });
                }
            }
        }
        Kind::Fox => {
            posed.look("head", m.head_yaw, m.head_pitch);
            quadruped_legs(posed, pos, speed, 1.4);
        }
        Kind::Axolotl => {
            posed.with("body", |p| p.rotation.y += m.head_yaw.to_radians());
            if in_water {
                // `setupSwimmingAnimation` / hovering.
                let s = if speed > 0.01 { age * 0.33 } else { age * 0.09 };
                posed.with("tail", |p| p.rotation.y = 0.3 * cos(s * 0.9));
                posed.with("left_gills", |p| p.rotation.y = 0.2 * cos(s) + 0.6);
                posed.with("right_gills", |p| p.rotation.y = -(0.2 * cos(s) + 0.6));
                posed.with("top_gills", |p| p.rotation.x = 0.2 * cos(s));
                for leg in ["left_front_leg", "right_front_leg", "left_hind_leg", "right_hind_leg"] {
                    posed.with(leg, |p| p.rotation.x = 1.8849558);
                }
            } else {
                // `setupGroundCrawlingAnimation`.
                let s = pos * 1.3;
                posed.with("tail", |p| p.rotation.y = 0.3 * cos(s * 0.9) * speed.min(1.0));
                posed.with("right_front_leg", |p| p.rotation.x = cos(s) * 1.2 * speed);
                posed.with("left_front_leg", |p| p.rotation.x = cos(s + PI) * 1.2 * speed);
                posed.with("right_hind_leg", |p| p.rotation.x = cos(s + PI) * 1.2 * speed);
                posed.with("left_hind_leg", |p| p.rotation.x = cos(s) * 1.2 * speed);
            }
            // `AdultAxolotlModel.setupPlayDeadAnimation`: rolled half over,
            // its left legs splayed.
            let f = e.axolotl.dead_factor(m.age_in_ticks.fract());
            if f > 1.0e-5 {
                posed.with("left_hind_leg", |p| p.rotation += Vec3::new(1.413_716_7, 1.099_557_4, PI / 4.0) * f);
                posed.with("left_front_leg", |p| p.rotation += Vec3::new(PI / 4.0, 2.042_035, 0.0) * f);
                posed.with("body", |p| p.rotation += Vec3::new(-0.15, 0.0, 0.35) * f);
            }
        }
        Kind::Strider => {
            posed.with("body", |p| p.rotation = Vec3::new(m.head_pitch.to_radians(), m.head_yaw.to_radians(), 0.1 * sin(pos * 1.5) * 4.0 * speed.min(0.25)));
            let s = speed.min(0.25);
            posed.with("left_leg", |p| p.rotation = Vec3::new(sin(pos * 0.75) * 2.0 * s, 0.0, PI / 18.0 * cos(pos * 0.75) * s));
            posed.with("right_leg", |p| p.rotation = Vec3::new(sin(pos * 0.75 + PI) * 2.0 * s, 0.0, PI / 18.0 * cos(pos * 0.75 + PI) * s));
            for (bristle, sign) in [("left_top_bristle", -1.0), ("left_middle_bristle", -1.0), ("left_bottom_bristle", -1.0), ("right_top_bristle", 1.0), ("right_middle_bristle", 1.0), ("right_bottom_bristle", 1.0)] {
                posed.with(bristle, |p| p.rotation.z += sign * 0.1 * sin(age * 0.4));
            }
        }
        Kind::CaveSpider => {
            posed.look("head", m.head_yaw, m.head_pitch);
            let motion = crate::spider_render::leg_motion(pos, speed);
            let legs = [
                ("right_hind_leg", 0),
                ("left_hind_leg", 0),
                ("right_middle_hind_leg", 1),
                ("left_middle_hind_leg", 1),
                ("right_middle_front_leg", 2),
                ("left_middle_front_leg", 2),
                ("right_front_leg", 3),
                ("left_front_leg", 3),
            ];
            for (leg, phase) in legs {
                let (swing, step) = motion[phase];
                let left = leg.starts_with("left");
                posed.with(leg, |p| {
                    if left {
                        p.rotation.y -= swing;
                        p.rotation.z -= step;
                    } else {
                        p.rotation.y += swing;
                        p.rotation.z += step;
                    }
                });
            }
        }
        Kind::Piglin | Kind::PiglinBrute | Kind::ZombifiedPiglin => {
            humanoid(posed, m, aggressive, e.kind == Kind::ZombifiedPiglin && aggressive);
            // `PiglinArmPose.ADMIRING_ITEM`: head bowed over the gold in
            // its raised left hand.
            if e.admire_ticks > 0 {
                posed.with("head", |p| {
                    p.rotation.x = 0.5;
                    p.rotation.y = 0.0;
                });
                posed.with("left_arm", |p| {
                    p.rotation.y = 0.5;
                    p.rotation.x = -0.9;
                });
            }
            // `AbstractPiglinModel`: the ears flap.
            let f = age * 0.1 + pos * 0.5;
            let amp = 0.08 + speed * 0.4;
            let angle = PI / 6.0;
            posed.with("left_ear", |p| p.rotation.z = -angle - cos(f * 1.2) * amp);
            posed.with("right_ear", |p| p.rotation.z = angle + cos(f) * amp);
        }
        Kind::WitherSkeleton | Kind::Giant => humanoid(posed, m, aggressive, e.kind == Kind::Giant),
        Kind::Pillager | Kind::Vindicator | Kind::Evoker | Kind::Illusioner | Kind::WanderingTrader => {
            // `Pillager.getArmPose`: a crossbow is always held ready.
            let ready = e.kind == Kind::Pillager && e.hand.as_deref() == Some("minecraft:crossbow");
            // `SpellcasterIllager.isCastingSpell` on the client: a spell shows.
            let casting = e.spells.spell != minecraftoss_entities::world::Spell::None;
            illager(posed, m, e.kind, aggressive || ready, e.charging() || casting);
        }
        Kind::Vex => {
            posed.look("head", m.head_yaw, m.head_pitch);
            let bob = cos(age * 5.5 * PI / 180.0) * 0.1;
            posed.with("right_arm", |p| p.rotation.z = PI / 5.0 + bob);
            posed.with("left_arm", |p| p.rotation.z = -(PI / 5.0 + bob));
            if e.charging() {
                posed.with("body", |p| p.rotation.x = 0.0);
                posed.with("right_arm", |p| {
                    p.rotation.x = -1.2;
                    p.rotation.y = PI / 12.0;
                    p.rotation.z = -0.4 - bob;
                });
            } else {
                posed.with("body", |p| p.rotation.x = PI / 20.0);
            }
            let wing = 1.099_557_4 + cos(age * 45.836_624 * PI / 180.0) * PI / 180.0 * 16.2;
            posed.with("left_wing", |p| p.rotation = Vec3::new(0.471_238_9, wing, -0.471_238_9));
            posed.with("right_wing", |p| p.rotation = Vec3::new(0.471_238_9, -wing, 0.471_238_9));
        }
        Kind::Allay => {
            posed.look("head", m.head_yaw, m.head_pitch);
            let flap = cos(age * 20.0 * PI / 180.0 + pos) * PI * 0.15 + speed;
            let flying = (speed / 0.3).min(1.0);
            let idle = age * 9.0 * PI / 180.0;
            posed.with("right_wing", |p| p.rotation = Vec3::new(0.436_332_32 * (1.0 - flying), -PI / 4.0 + flap, p.rotation.z));
            posed.with("left_wing", |p| p.rotation = Vec3::new(0.436_332_32 * (1.0 - flying), PI / 4.0 - flap, p.rotation.z));
            posed.with("body", |p| p.rotation.x = flying * PI / 4.0);
            posed.with("root", |p| p.offset.y += cos(idle) * 0.25 * (1.0 - flying));
            let arm = 0.436_332_32 - cos(idle + PI * 1.5) * PI * 0.075 * (1.0 - flying);
            posed.with("left_arm", |p| p.rotation.z = -arm);
            posed.with("right_arm", |p| p.rotation.z = arm);
        }
        Kind::Bee => {
            if !e.body.on_ground {
                let s = age * 120.321_13 * PI / 180.0;
                let z = cos(s) * PI * 0.15;
                posed.with("right_wing", |p| {
                    p.rotation.y = 0.0;
                    p.rotation.z = z;
                });
                let right_x = posed.get("right_wing").map_or(0.0, |p| p.rotation.x);
                posed.with("left_wing", |p| p.rotation = Vec3::new(right_x, 0.0, -z));
                for leg in ["front_legs", "middle_legs", "back_legs"] {
                    posed.with(leg, |p| p.rotation.x = PI / 4.0);
                }
                if !aggressive {
                    // `bobUpAndDown`.
                    let bob = cos(age * 0.18);
                    posed.with("bone", |p| {
                        p.rotation.x = 0.1 + bob * PI * 0.025;
                        p.offset.y = 19.0 - bob * 0.9;
                    });
                }
            }
            posed.with("left_antenna", |p| p.rotation.x = 0.0);
        }
        Kind::Parrot => {
            posed.look("head", m.head_yaw, m.head_pitch);
            let flying = !e.body.on_ground;
            let flap = if flying { (sin(age * 0.6) + 1.0) * 0.6 } else { 0.0 };
            if !flying {
                posed.with("left_leg", |p| p.rotation.x += cos(pos * 0.6662) * 1.4 * speed);
                posed.with("right_leg", |p| p.rotation.x += cos(pos * 0.6662 + PI) * 1.4 * speed);
            }
            let bob = flap * 0.3;
            for part in ["head", "tail", "body", "left_wing", "right_wing", "left_leg", "right_leg"] {
                posed.with(part, |p| p.offset.y += bob);
            }
            posed.with("left_wing", |p| p.rotation.z = -0.0873 - flap);
            posed.with("right_wing", |p| p.rotation.z = 0.0873 + flap);
        }
        Kind::Blaze => {
            let mut angle = age * PI * -0.1;
            for i in 0..4 {
                posed.with(&format!("part{i}"), |p| {
                    p.offset = Vec3::new(cos(angle) * 9.0, -2.0 + cos((i as f32 * 2.0 + age) * 0.25), sin(angle) * 9.0);
                });
                angle += PI / 2.0;
            }
            angle = PI / 4.0 + age * PI * 0.03;
            for i in 4..8 {
                posed.with(&format!("part{i}"), |p| {
                    p.offset = Vec3::new(cos(angle) * 7.0, 2.0 + cos((i as f32 * 2.0 + age) * 0.25), sin(angle) * 7.0);
                });
                angle += PI / 2.0;
            }
            angle = 0.471_238_94 + age * PI * -0.05;
            for i in 8..12 {
                posed.with(&format!("part{i}"), |p| {
                    p.offset = Vec3::new(cos(angle) * 5.0, 11.0 + cos((i as f32 * 1.5 + age) * 0.5), sin(angle) * 5.0);
                });
                angle += PI / 2.0;
            }
            posed.look("head", m.head_yaw, m.head_pitch);
        }
        Kind::Ghast | Kind::HappyGhast => {
            for i in 0..9 {
                posed.with(&format!("tentacle{i}"), |p| p.rotation.x = 0.2 * sin(age * 0.3 + i as f32) + 0.4);
            }
        }
        Kind::Guardian | Kind::ElderGuardian => {
            // `GuardianModel.setupAnim`: the head looks, the spikes stand
            // out as it rests and draw in as it swims, and the tail sways.
            posed.look("head", m.head_yaw, m.head_pitch);
            let partial = m.age_in_ticks.fract();
            let withdrawal = (1.0 - e.guardian.spikes(partial)) * 0.55;
            const SPIKE_X: [f32; 12] = [0.0, 0.0, 8.0, -8.0, -8.0, 8.0, 8.0, -8.0, 0.0, 0.0, 8.0, -8.0];
            const SPIKE_Y: [f32; 12] = [-8.0, -8.0, -8.0, -8.0, 0.0, 0.0, 0.0, 0.0, 8.0, 8.0, 8.0, 8.0];
            const SPIKE_Z: [f32; 12] = [8.0, -8.0, 0.0, 0.0, -8.0, -8.0, 8.0, 8.0, 8.0, -8.0, 0.0, 0.0];
            for i in 0..12 {
                let out = 1.0 + cos(age * 1.5 + i as f32) * 0.01 - withdrawal;
                posed.with(&format!("spike{i}"), |p| p.offset = Vec3::new(SPIKE_X[i] * out, 16.0 + SPIKE_Y[i] * out, SPIKE_Z[i] * out));
            }
            let swim = e.guardian.tail(partial);
            for (i, part) in ["tail0", "tail1", "tail2"].into_iter().enumerate() {
                posed.with(part, |p| p.rotation.y = sin(swim) * PI * 0.05 * (i + 1) as f32);
            }
        }
        Kind::Squid | Kind::GlowSquid => {
            // `SquidModel.setupAnim`: every tentacle at its stroke's angle.
            let angle = e.squid.tentacle_angle(m.age_in_ticks.fract());
            for i in 0..8 {
                posed.with(&format!("tentacle{i}"), |p| p.rotation.x = angle);
            }
        }
        Kind::Cod | Kind::Pufferfish => {
            fish_tail(posed, "tail_fin", age, in_water, 0.45, 0.6);
            posed.with("right_blue_fin", |p| p.rotation.z = -0.2 + 0.4 * sin(age * 0.2));
            posed.with("left_blue_fin", |p| p.rotation.z = 0.2 - 0.4 * sin(age * 0.2));
        }
        Kind::TropicalFish => fish_tail(posed, "tail", age, in_water, 0.45, 0.6),
        Kind::Tadpole => fish_tail(posed, "tail", age, in_water, 0.25, 0.3),
        Kind::Salmon => {
            let (amplitude, angle) = if in_water { (1.0, 1.0) } else { (1.3, 1.7) };
            posed.with("body_back", |p| p.rotation.y = -amplitude * 0.25 * sin(angle * 0.6 * age));
        }
        Kind::Dolphin => {
            posed.with("body", |p| p.rotation = Vec3::new(e.pitch.to_radians(), m.head_yaw.to_radians(), 0.0));
            if speed > 0.01 || e.body.velocity.length_squared() > 1.0e-4 {
                posed.with("body", |p| p.rotation.x += -0.05 - 0.05 * cos(age * 0.3));
                posed.with("tail", |p| p.rotation.x = -0.1 * cos(age * 0.3));
                posed.with("tail_fin", |p| p.rotation.x = -0.2 * cos(age * 0.3));
            }
        }
        Kind::Phantom => {
            let a = age * 7.448_451 * PI / 180.0 * 2.0;
            let flap = cos(a) * 16.0 * PI / 180.0;
            posed.with("left_wing_base", |p| p.rotation.z = flap);
            posed.with("left_wing_tip", |p| p.rotation.z = flap);
            posed.with("right_wing_base", |p| p.rotation.z = -flap);
            posed.with("right_wing_tip", |p| p.rotation.z = -flap);
            let tail = -(5.0 + cos(a * 2.0) * 5.0) * PI / 180.0;
            posed.with("tail_base", |p| p.rotation.x = tail);
            posed.with("tail_tip", |p| p.rotation.x = tail);
        }
        Kind::Endermite | Kind::Silverfish => {
            let (twist, sway, count) = if e.kind == Kind::Endermite { (0.01, 0.1, 4) } else { (0.05, 0.2, 7) };
            let mut parts = Vec::new();
            for i in 0..count {
                let phase = age * 0.9 + i as f32 * 0.15 * PI;
                let (yaw, x) = (cos(phase) * PI * twist * (1 + (i as i32 - 2).abs()) as f32, sin(phase) * PI * sway * (i as i32 - 2).abs() as f32);
                posed.with(&format!("segment{i}"), |p| {
                    p.rotation.y = yaw;
                    p.offset.x = x;
                });
                parts.push((yaw, x));
            }
            if e.kind == Kind::Silverfish {
                for (layer, from) in [("layer0", 2), ("layer1", 4), ("layer2", 1)] {
                    let (yaw, x) = parts[from];
                    posed.with(layer, |p| {
                        p.rotation.y = yaw;
                        if layer != "layer0" {
                            p.offset.x = x;
                        }
                    });
                }
            }
        }
        Kind::Shulker => {
            // `ShulkerModel`: the lid rises as it peeks.
            let peek = e.shulker.peek_amount(age.fract());
            let bs = (0.5 + peek) * PI;
            let q = -1.0 + sin(bs);
            let extra = if bs > PI { sin(age * 0.1) * 0.7 } else { 0.0 };
            posed.with("lid", |p| {
                p.offset = Vec3::new(0.0, 16.0 + sin(bs) * 8.0 + extra, 0.0);
                p.rotation.y = if peek > 0.3 { q * q * q * q * PI * 0.125 } else { 0.0 };
            });
            // `head.yRot = yHeadRot - 180 - yBodyRot`.
            posed.look("head", m.head_yaw - 180.0, m.head_pitch);
        }
        Kind::SnowGolem => {
            posed.look("head", m.head_yaw, m.head_pitch);
            let upper = m.head_yaw.to_radians() * 0.25;
            posed.with("upper_body", |p| p.rotation.y = upper);
            let (s, c) = (sin(upper), cos(upper));
            posed.with("left_arm", |p| {
                p.rotation.y = upper;
                p.offset.x = c * 5.0;
                p.offset.z = -s * 5.0;
            });
            posed.with("right_arm", |p| {
                p.rotation.y = upper + PI;
                p.offset.x = -c * 5.0;
                p.offset.z = s * 5.0;
            });
        }
        Kind::Wither => {
            let anim = cos(age * 0.1);
            let rib = (0.065 + 0.05 * anim) * PI;
            posed.with("ribcage", |p| p.rotation.x = rib);
            posed.with("tail", |p| {
                p.offset = Vec3::new(-2.0, 6.9 + cos(rib) * 10.0, -0.5 + sin(rib) * 10.0);
                p.rotation.x = (0.265 + 0.1 * anim) * PI;
            });
            posed.look("center_head", m.head_yaw, m.head_pitch);
        }
        Kind::MagmaCube | Kind::SulfurCube | Kind::EnderDragon => {}
    }
}

/// `LivingEntityRenderer.setupRotations` beyond the body's turn: a fish
/// wobbles and lies on its side ashore, a phantom pitches.
fn extra_rotation(e: &MobEntity, m: &MobPose) -> (Quat, Vec3) {
    let s = e.species();
    if s.fish {
        let wobble = Quat::from_rotation_y((4.3 * sin(0.6 * m.age_in_ticks)).to_radians());
        if !e.body.touching_water {
            return (wobble * Quat::from_rotation_z(90.0_f32.to_radians()), Vec3::new(0.1, 0.1, -0.1));
        }
        return (wobble, Vec3::ZERO);
    }
    if matches!(e.kind, Kind::Phantom) {
        return (Quat::from_rotation_x(e.pitch.to_radians()), Vec3::ZERO);
    }
    // `SquidRenderer.setupRotations`: tilted and spun about a point half a
    // block up (a baby's quarter), the model hung 1.2 below it (0.6).
    if matches!(e.kind, Kind::Squid | Kind::GlowSquid) {
        let (x_rot, z_rot) = e.squid.body_rot(m.age_in_ticks.fract());
        let q = Quat::from_rotation_x(x_rot.to_radians()) * Quat::from_rotation_y(z_rot.to_radians());
        let (up, down) = if e.baby() { (0.25, 0.6) } else { (0.5, 1.2) };
        return (q, q.inverse() * Vec3::new(0.0, up, 0.0) - Vec3::new(0.0, down, 0.0));
    }
    // `ShulkerRenderer.setupRotations`: its body turned half round more,
    // then the whole of it about its middle to the face it clings by
    // (`Direction.getRotation` of the opposite face).
    if e.kind == Kind::Shulker {
        use std::f32::consts::PI;
        let q = match e.shulker.face {
            // Clinging by the floor, it stands upright.
            0 => Quat::IDENTITY,
            1 => Quat::from_rotation_x(PI),
            2 => Quat::from_rotation_x(PI / 2.0),
            3 => Quat::from_euler(glam::EulerRot::XYZ, PI / 2.0, 0.0, PI),
            4 => Quat::from_euler(glam::EulerRot::XYZ, PI / 2.0, 0.0, -PI / 2.0),
            _ => Quat::from_euler(glam::EulerRot::XYZ, PI / 2.0, 0.0, PI / 2.0),
        };
        let pivot = Vec3::new(0.0, 0.5, 0.0);
        return (Quat::from_rotation_y(PI) * q, q.inverse() * pivot - pivot);
    }
    // `isShaking`: a cold strider's turn jitters (`setupRotations`).
    if e.kind == Kind::Strider && e.cold {
        let shake = (m.age_in_ticks.floor() * 3.25).cos() * PI * 0.4;
        return (Quat::from_rotation_y(-shake.to_radians()), Vec3::ZERO);
    }
    (Quat::IDENTITY, Vec3::ZERO)
}

/// `ItemInHandLayer` for a mob holding something: each arm's place
/// (`translateToHand`, with the vex's shrink and shift), turned to hold
/// its hand's item.
fn held_in_hands(e: &MobEntity, posed: &Posed, m: &MobPose, rotation: Quat, scale: Vec3) -> Vec<crate::mesh::HeldItem> {
    use glam::Mat4;
    let mut held = Vec::new();
    for (item, arm, side, display) in [(&e.hand, "right_arm", 1.0, crate::mesh::HeldDisplay::RightHand), (&e.off_hand, "left_arm", -1.0, crate::mesh::HeldDisplay::LeftHand)] {
        let Some(item) = item else { continue };
        if !posed.shown(arm) {
            continue;
        }
        let Some(mut matrix) = posed.matrix_of(arm) else { continue };
        if e.kind == Kind::Vex {
            matrix = matrix * Mat4::from_scale(Vec3::splat(0.55)) * Mat4::from_translation(Vec3::new(0.046875 * side, -0.15625, 0.078125));
        }
        let pose = Mat4::from_translation(m.feet.as_vec3())
            * Mat4::from_quat(rotation)
            * Mat4::from_scale(scale)
            * Mat4::from_translation(Vec3::new(0.0, 1.501, 0.0))
            * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
            * matrix
            * Mat4::from_rotation_x((-90.0f32).to_radians())
            * Mat4::from_rotation_y(180.0f32.to_radians())
            * Mat4::from_translation(Vec3::new(side, 2.0, -10.0) / 16.0);
        held.push(crate::mesh::HeldItem { pose, light: m.light_probe.as_vec3(), id: item.clone(), display, first_tint: None, state: Default::default() });
    }
    held
}

/// Draws the generic mobs into the model mesh; returns what they hold.
pub fn append_mobs<'a>(mesh: &mut ChunkMesh, mobs: impl IntoIterator<Item = &'a MobEntity>, poses: &ClientMobs, atlas: &Atlas, light: &SkyLight, partial: f32) -> Vec<crate::mesh::HeldItem> {
    let partial = partial.clamp(0.0, 1.0);
    let mut marks = Vec::new();
    let mut held = Vec::new();
    for e in mobs {
        let Some(m) = poses.pose(e.id, partial) else { continue };
        let (layers, own_baby) = layers(e);
        let sample = m.light_block();
        let (sky, block) = (light.get(sample) as f32, light.get_block(sample) as f32);
        let (extra, shift) = extra_rotation(e, &m);
        let rotation = m.body_rotation(90.0) * extra;
        let age_scale = if e.baby() && !own_baby && !e.species().fixed { 0.5 } else { 1.0 };
        // A magma cube's size (`MagmaCubeRenderer.scale`).
        let size = match e.kind {
            Kind::MagmaCube | Kind::SulfurCube => (e.variant.parse::<i32>().unwrap_or(0) + 1) as f32 * 0.999,
            // `WitherBossRenderer.scale`: smaller as it spawns in.
            Kind::Wither => e.wither_spawn_look(partial).0,
            _ => 1.0,
        };
        let scale = Vec3::splat(age_scale * size);
        // An unseen mob is not drawn, but an illusioner shows its four
        // images, each wavering (`IllusionerRenderer.submit`).
        let copies: Vec<Vec3> = if !e.invisible() {
            vec![Vec3::ZERO]
        } else if e.kind == Kind::Illusioner {
            let age = m.age_in_ticks;
            e.illusion.offsets(partial).iter().enumerate().map(|(i, o)| {
                let i = i as f32;
                Vec3::new(o[0] + (i + age * 0.5).cos() * 0.025, o[1] + (i + age * 0.75).cos() * 0.0125, o[2] + (i + age * 0.7).cos() * 0.025)
            }).collect()
        } else {
            continue;
        };
        for copy in copies {
        marks.push((mesh.vertices.len(), m.overlay(0.0)));
        for l in &layers {
            let Some(model) = entity_model::model(&l.model) else { continue };
            let id = ResourceId::parse(&l.texture).unwrap_or_else(|_| ResourceId::parse("minecraft:missingno").unwrap());
            if !atlas.contains(&id) {
                continue;
            }
            let mut posed = Posed::new(model);
            animate(&mut posed, e, &m);
            let mut m = m.clone();
            m.feet += copy.as_dvec3();
            if std::ptr::eq(l, &layers[0]) {
                held.extend(held_in_hands(e, &posed, &m, rotation, scale));
            }
            let feet = m.feet + rotation.mul_vec3(shift).as_dvec3();
            let glow = if e.kind == Kind::GlowSquid { e.squid.glow_light(block) } else { 15.0 };
            let placement = Placement { feet, rotation, scale, region: atlas.entity_region(&id), sky, block: if l.glow { glow } else { block }, tint: l.tint };
            entity_model::draw(mesh, &posed, &placement);
        }
        }
    }
    crate::cow_render::apply_overlays(mesh, &marks);
    held
}
