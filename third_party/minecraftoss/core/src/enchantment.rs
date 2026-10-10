//! The enchantment registry as loot and enchanting choose from it (26.3
//! `Enchantment`'s definition, `EnchantmentHelper.selectEnchantment` and
//! `getAvailableEnchantmentResults`), read from the data pack: every
//! `enchantment/*.json`, in identifier order as the registry loads them,
//! and the enchantment and item tags their holder sets name.

use crate::datapack::DataPack;
use crate::ident::Identifier;
use crate::random::RandomSource;
use serde_json::Value as Json;
use std::collections::HashSet;

/// One enchantment's definition, as selection reads it.
#[derive(Clone, Debug)]
pub struct Enchantment {
    pub id: String,
    pub max_level: i32,
    pub weight: i32,
    /// `anvil_cost`: what the anvil charges a level of it.
    pub anvil_cost: i32,
    /// `Enchantment.Cost`: its base, and what each level above the first
    /// adds.
    min_cost: (i32, i32),
    max_cost: (i32, i32),
    /// `supported_items`, `primary_items` (none: the supported ones) and
    /// `exclusive_set`, resolved to ids.
    supported: HashSet<String>,
    primary: Option<HashSet<String>>,
    exclusive: HashSet<String>,
}

impl Enchantment {
    pub fn min_cost(&self, level: i32) -> i32 {
        self.min_cost.0 + self.min_cost.1 * (level - 1)
    }

    pub fn max_cost(&self, level: i32) -> i32 {
        self.max_cost.0 + self.max_cost.1 * (level - 1)
    }

    /// `canEnchant`: the item is one it supports.
    pub fn can_enchant(&self, item: &str) -> bool {
        self.supported.contains(item)
    }

    /// `isPrimaryItem`: a supported item, and one of its primary items if
    /// it names any.
    pub fn is_primary_item(&self, item: &str) -> bool {
        self.can_enchant(item) && self.primary.as_ref().is_none_or(|primary| primary.contains(item))
    }
}

/// The enchantment registry, in registry order, with its tags.
#[derive(Clone, Debug, Default)]
pub struct Enchantments {
    pub list: Vec<Enchantment>,
    pack: Option<DataPack>,
}

/// A data pack tag's members (`tags/<registry>/<id>.json`): its values in
/// file order, nested tags expanded where they stand, each id once, and
/// optional entries (`required: false`) kept as named. A missing tag has
/// none.
pub fn tag_members(pack: &DataPack, registry: &str, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    collect_tag(pack, registry, id, &mut out, 0);
    out
}

fn collect_tag(pack: &DataPack, registry: &str, id: &str, out: &mut Vec<String>, depth: usize) {
    let Ok(ident) = Identifier::parse(id) else { return };
    let Ok(tag) = pack.read_json(&format!("tags/{registry}"), &ident) else { return };
    for value in tag["values"].as_array().into_iter().flatten() {
        let Some(name) = value.as_str().or_else(|| value["id"].as_str()) else { continue };
        match name.strip_prefix('#') {
            Some(nested) if depth < 16 => collect_tag(pack, registry, nested, out, depth + 1),
            Some(_) => {}
            None => {
                let name = id_of(name);
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
}

fn id_of(text: &str) -> String {
    if text.contains(':') { text.to_owned() } else { format!("minecraft:{text}") }
}

/// A holder set's ids: a `#tag`'s members, one id, or a list of ids.
pub fn holder_set(pack: &DataPack, registry: &str, value: &Json) -> Vec<String> {
    match value {
        Json::String(text) => match text.strip_prefix('#') {
            Some(tag) => tag_members(pack, registry, tag),
            None => vec![id_of(text)],
        },
        Json::Array(list) => list.iter().filter_map(Json::as_str).map(id_of).collect(),
        _ => Vec::new(),
    }
}

fn cost(value: &Json) -> (i32, i32) {
    (value["base"].as_i64().unwrap_or(0) as i32, value["per_level_above_first"].as_i64().unwrap_or(0) as i32)
}

/// `Math.round(float)`: the nearest int, halves up, saturating. The sum is
/// exact in a double, so its floor is Java's.
fn java_round(value: f32) -> i32 {
    (f64::from(value) + 0.5).floor() as i32
}

impl Enchantments {
    /// The registry from a data pack.
    pub fn load(pack: &DataPack) -> Result<Self, String> {
        let mut list = Vec::new();
        for ident in pack.list("enchantment")? {
            let json = pack.read_json("enchantment", &ident)?;
            list.push(Enchantment {
                id: ident.to_string(),
                max_level: json["max_level"].as_i64().unwrap_or(1) as i32,
                weight: json["weight"].as_i64().unwrap_or(1) as i32,
                anvil_cost: json["anvil_cost"].as_i64().unwrap_or(0) as i32,
                min_cost: cost(&json["min_cost"]),
                max_cost: cost(&json["max_cost"]),
                supported: holder_set(pack, "item", &json["supported_items"]).into_iter().collect(),
                primary: json.get("primary_items").map(|items| holder_set(pack, "item", items).into_iter().collect()),
                exclusive: json.get("exclusive_set").map_or_else(HashSet::new, |set| holder_set(pack, "enchantment", set).into_iter().collect()),
            });
        }
        Ok(Self { list, pack: Some(pack.clone()) })
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        let id = id_of(id);
        self.list.iter().position(|e| e.id == id)
    }

    /// An enchantment holder set (`options`) as registry indices in its
    /// order; every enchantment, in registry order, for none.
    pub fn source(&self, options: Option<&Json>) -> Vec<usize> {
        match (options, &self.pack) {
            (None, _) => (0..self.list.len()).collect(),
            (Some(options), Some(pack)) => holder_set(pack, "enchantment", options).iter().filter_map(|id| self.index(id)).collect(),
            (Some(_), None) => Vec::new(),
        }
    }

    /// `Enchantment.areCompatible`.
    pub fn compatible(&self, a: usize, b: usize) -> bool {
        a != b && !self.list[a].exclusive.contains(&self.list[b].id) && !self.list[b].exclusive.contains(&self.list[a].id)
    }

    /// `getAvailableEnchantmentResults`: each enchantment of `source` for
    /// which the item is a primary item (a book takes any) at the highest
    /// level whose cost range holds `cost`.
    fn available(&self, cost: i32, item: &str, source: &[usize]) -> Vec<(usize, i32)> {
        let book = item == "minecraft:book";
        let mut out = Vec::new();
        for &index in source {
            let enchantment = &self.list[index];
            if !book && !enchantment.is_primary_item(item) {
                continue;
            }
            if let Some(level) = (1..=enchantment.max_level).rev().find(|&level| cost >= enchantment.min_cost(level) && cost <= enchantment.max_cost(level)) {
                out.push((index, level));
            }
        }
        out
    }

    /// `WeightedRandom.getRandomItem` by the enchantments' weights: none
    /// when they weigh nothing.
    fn weighted(&self, random: &mut dyn RandomSource, list: &[(usize, i32)]) -> Option<(usize, i32)> {
        let total: i32 = list.iter().map(|&(index, _)| self.list[index].weight).sum();
        if total <= 0 {
            return None;
        }
        let mut selection = random.next_i32_bound(total);
        for &entry in list {
            selection -= self.list[entry.0].weight;
            if selection < 0 {
                return Some(entry);
            }
        }
        None
    }

    /// `EnchantmentHelper.selectEnchantment` for an item whose
    /// `enchantable` value is `enchantable` (none: it takes nothing): the
    /// cost raised by two draws on a quarter of it and spread by up to 15%,
    /// one enchantment by weight, then, while a draw below 50 stays within
    /// the cost (halved each time), another compatible with the last one.
    /// The chosen (registry index, level) in order.
    pub fn select(&self, random: &mut dyn RandomSource, item: &str, enchantable: Option<i32>, mut cost: i32, source: &[usize]) -> Vec<(usize, i32)> {
        let mut chosen = Vec::new();
        let Some(enchantable) = enchantable else { return chosen };
        cost += 1 + random.next_i32_bound(enchantable / 4 + 1) + random.next_i32_bound(enchantable / 4 + 1);
        let spread = (random.next_f32() + random.next_f32() - 1.0) * 0.15;
        cost = java_round(cost as f32 + cost as f32 * spread).max(1);
        let mut available = self.available(cost, item, source);
        if available.is_empty() {
            return chosen;
        }
        chosen.extend(self.weighted(random, &available));
        while random.next_i32_bound(50) <= cost {
            if let Some(&(last, _)) = chosen.last() {
                available.retain(|&(index, _)| self.compatible(last, index));
            }
            if available.is_empty() {
                break;
            }
            chosen.extend(self.weighted(random, &available));
            cost /= 2;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registries::DataPaths;

    #[test]
    fn rounds_as_java_does() {
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(0.49999997), 0);
        assert_eq!(java_round(7.4), 7);
        assert_eq!(java_round(1.0e10), i32::MAX);
    }

    #[test]
    fn the_registry_reads_its_definitions_and_tags() {
        let Ok(paths) = DataPaths::discover() else { return };
        let Ok(pack) = DataPack::open(&paths.datapack) else { return };
        let enchantments = Enchantments::load(&pack).unwrap();
        // Identifier order: aqua_affinity first, wind_burst last.
        assert_eq!(enchantments.list.first().map(|e| e.id.as_str()), Some("minecraft:aqua_affinity"));
        assert_eq!(enchantments.list.last().map(|e| e.id.as_str()), Some("minecraft:wind_burst"));
        let sharpness = &enchantments.list[enchantments.index("minecraft:sharpness").unwrap()];
        assert_eq!((sharpness.max_level, sharpness.weight, sharpness.min_cost(2), sharpness.max_cost(2)), (5, 10, 12, 32));
        // Axes take sharpness (supported) but are not melee weapons.
        assert!(sharpness.can_enchant("minecraft:iron_axe") && !sharpness.is_primary_item("minecraft:iron_axe"));
        assert!(sharpness.is_primary_item("minecraft:iron_sword"));
        let smite = enchantments.index("minecraft:smite").unwrap();
        let sharp = enchantments.index("minecraft:sharpness").unwrap();
        assert!(!enchantments.compatible(sharp, smite));
        // The loot tag lists the non-treasure tag's members first, then
        // the curses, frost walker and mending.
        let loot = enchantments.source(Some(&Json::from("#minecraft:on_random_loot")));
        let ids: Vec<&str> = loot.iter().map(|&i| enchantments.list[i].id.as_str()).collect();
        assert_eq!(ids[ids.len() - 4..], ["minecraft:binding_curse", "minecraft:vanishing_curse", "minecraft:frost_walker", "minecraft:mending"]);
    }
}
