//! Mobs in buckets (26.3 `Bucketable` and `MobBucketItem`): a water bucket
//! used on a fish, an axolotl or a tadpole scoops it up into its bucket,
//! which keeps what `saveToBucketTag` saves (its health, flags and age in
//! `bucket_entity_data`, its look as the variant's own component); emptying
//! the bucket pours the water and lets the mob out again from that data,
//! marked `FromBucket` so it never despawns.
use minecraftoss_core::nbt::Tag;
use minecraftoss_entities::species::Kind;
use minecraftoss_entities::world::MobEntity;
use serde_json::{json, Map, Value};

const AXOLOTLS: [&str; 5] = ["lucy", "wild", "gold", "cyan", "blue"];
const DYES: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
/// `TropicalFish.Pattern`, the small shapes then the large.
const PATTERNS: [&str; 12] = ["kob", "sunstreak", "snooper", "dasher", "brinely", "spotty", "flopper", "stripey", "glitter", "blockfish", "betty", "clayfish"];

/// The bucket a mob goes into and `getPickupSound`, for the bucketable.
pub fn bucket_for(kind: Kind) -> Option<(&'static str, &'static str)> {
    Some(match kind {
        Kind::Cod => ("minecraft:cod_bucket", "item.bucket.fill_fish"),
        Kind::Salmon => ("minecraft:salmon_bucket", "item.bucket.fill_fish"),
        Kind::Pufferfish => ("minecraft:pufferfish_bucket", "item.bucket.fill_fish"),
        Kind::TropicalFish => ("minecraft:tropical_fish_bucket", "item.bucket.fill_fish"),
        Kind::Axolotl => ("minecraft:axolotl_bucket", "item.bucket.fill_axolotl"),
        Kind::Tadpole => ("minecraft:tadpole_bucket", "item.bucket.fill_tadpole"),
        _ => return None,
    })
}

/// The mob a bucket holds and `MobBucketItem`'s empty sound.
pub fn mob_in(bucket: &str) -> Option<(&'static str, &'static str)> {
    Some(match bucket {
        "minecraft:cod_bucket" => ("minecraft:cod", "item.bucket.empty_fish"),
        "minecraft:salmon_bucket" => ("minecraft:salmon", "item.bucket.empty_fish"),
        "minecraft:pufferfish_bucket" => ("minecraft:pufferfish", "item.bucket.empty_fish"),
        "minecraft:tropical_fish_bucket" => ("minecraft:tropical_fish", "item.bucket.empty_fish"),
        "minecraft:axolotl_bucket" => ("minecraft:axolotl", "item.bucket.empty_axolotl"),
        "minecraft:tadpole_bucket" => ("minecraft:tadpole", "item.bucket.empty_tadpole"),
        _ => return None,
    })
}

/// `saveToBucketTag`: the bucket's components for this mob.
pub fn bucket_components(mob: &MobEntity) -> Value {
    // `saveDefaultDataToBucketTag`: the flags only when set, then health.
    let mut data = Map::new();
    if mob.no_ai {
        data.insert("NoAI".into(), json!(true));
    }
    if mob.persistence_required {
        data.insert("PersistenceRequired".into(), json!(true));
    }
    data.insert("Health".into(), json!(mob.health));
    if matches!(mob.kind, Kind::Axolotl | Kind::Tadpole) {
        data.insert("Age".into(), json!(if mob.kind == Kind::Tadpole { mob.frog.growth } else { mob.age.ticks }));
        data.insert("AgeLocked".into(), json!(mob.age.locked));
    }
    let mut components = Map::new();
    components.insert("minecraft:bucket_entity_data".into(), Value::Object(data));
    let packed: i64 = mob.variant.parse().unwrap_or(0);
    match mob.kind {
        Kind::Salmon => {
            let size = if mob.variant.is_empty() { "medium" } else { mob.variant.as_str() };
            components.insert("minecraft:salmon/size".into(), json!(size));
        }
        Kind::Axolotl => {
            components.insert("minecraft:axolotl/variant".into(), json!(AXOLOTLS[packed.clamp(0, 4) as usize]));
        }
        Kind::TropicalFish => {
            let pattern = (packed & 0xff).clamp(0, 1) * 6 + ((packed >> 8) & 0xff).clamp(0, 5);
            components.insert("minecraft:tropical_fish/pattern".into(), json!(PATTERNS[pattern as usize]));
            components.insert("minecraft:tropical_fish/base_color".into(), json!(DYES[((packed >> 16) & 0xf) as usize]));
            components.insert("minecraft:tropical_fish/pattern_color".into(), json!(DYES[((packed >> 24) & 0xf) as usize]));
        }
        _ => {}
    }
    Value::Object(components)
}

/// `MobBucketItem.spawn`: the mob a bucket lets out, as the tag it loads
/// from (`loadFromBucketTag`, the variant components, `setFromBucket`).
pub fn released_tag(bucket: &str, components: Option<&Value>) -> Option<(&'static str, Tag)> {
    let (mob, _) = mob_in(bucket)?;
    let empty = Map::new();
    let components = components.and_then(Value::as_object).unwrap_or(&empty);
    let mut tag = std::collections::BTreeMap::new();
    if let Some(data) = components.get("minecraft:bucket_entity_data").and_then(Value::as_object) {
        let flag = |key: &str| data.get(key).is_some_and(|v| v.as_bool().unwrap_or_else(|| v.as_i64().is_some_and(|n| n != 0)));
        for key in ["NoAI", "Silent", "NoGravity", "Glowing", "Invulnerable", "PersistenceRequired", "AgeLocked"] {
            if flag(key) {
                tag.insert(key.to_owned(), Tag::Byte(1));
            }
        }
        if let Some(health) = data.get("Health").and_then(Value::as_f64) {
            tag.insert("Health".to_owned(), Tag::Float(health as f32));
        }
        if let Some(age) = data.get("Age").and_then(Value::as_i64) {
            tag.insert("Age".to_owned(), Tag::Int(age as i32));
        }
    }
    let text = |key: &str| components.get(key).and_then(Value::as_str);
    match mob {
        "minecraft:salmon" => {
            tag.insert("type".to_owned(), Tag::String(text("minecraft:salmon/size").unwrap_or("medium").to_owned()));
        }
        "minecraft:axolotl" => {
            let index = text("minecraft:axolotl/variant").and_then(|v| AXOLOTLS.iter().position(|a| *a == v)).unwrap_or(0);
            tag.insert("Variant".to_owned(), Tag::Int(index as i32));
        }
        "minecraft:tropical_fish" => {
            let pattern = text("minecraft:tropical_fish/pattern").and_then(|v| PATTERNS.iter().position(|p| *p == v)).unwrap_or(0) as i32;
            let dye = |key: &str, default: usize| text(key).and_then(|v| DYES.iter().position(|d| *d == v)).unwrap_or(default) as i32;
            let packed = (pattern / 6) | (pattern % 6) << 8 | dye("minecraft:tropical_fish/base_color", 0) << 16 | dye("minecraft:tropical_fish/pattern_color", 0) << 24;
            tag.insert("Variant".to_owned(), Tag::Int(packed));
        }
        _ => {}
    }
    tag.insert("FromBucket".to_owned(), Tag::Byte(1));
    Some((mob, Tag::Compound(tag)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tropical_fish_keeps_its_look() {
        let components = json!({"minecraft:tropical_fish/pattern": "glitter", "minecraft:tropical_fish/base_color": "lime", "minecraft:tropical_fish/pattern_color": "red"});
        let (mob, tag) = released_tag("minecraft:tropical_fish_bucket", Some(&components)).unwrap();
        assert_eq!(mob, "minecraft:tropical_fish");
        let Tag::Compound(map) = tag else { panic!() };
        assert_eq!(map.get("Variant"), Some(&Tag::Int(1 | 2 << 8 | 5 << 16 | 14 << 24)));
        assert_eq!(map.get("FromBucket"), Some(&Tag::Byte(1)));
    }
}
