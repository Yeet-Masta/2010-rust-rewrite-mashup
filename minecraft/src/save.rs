//! A saved world's own file beside its region files: the seed it was
//! generated from and the player, as `level.json`.
use std::path::Path;

use minecraftoss_player::inventory::ItemStack;
use serde_json::{Value, json};

pub struct Saved {
    pub seed: i64,
    pub creative: bool,
    pub position: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub health: f32,
    pub food: u8,
    pub saturation: f32,
    pub experience: (u32, f32, u32),
    pub day_ticks: f64,
    /// A creative player in the air stays there.
    pub flying: bool,
    pub selected: usize,
    pub slots: Vec<Option<ItemStack>>,
    /// The ender chest's 27 slots (`EnderItems`), which are the player's.
    pub ender: Vec<Option<ItemStack>>,
    /// `XpSeed`: the enchantment seed the enchanting table offers from.
    pub enchantment_seed: i32,
}

const FILE: &str = "level.json";

pub fn read(dir: &Path) -> Option<Saved> {
    let value: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).ok()?).ok()?;
    let player = &value["player"];
    let float = |v: &Value| v.as_f64().unwrap_or(0.0);
    let position = player["position"].as_array()?;
    let slots = stacks(player["inventory"].as_array()?);
    // A world saved before ender chests opened has none.
    let ender = player["ender_items"]
        .as_array()
        .map_or_else(Vec::new, |slots| stacks(slots));
    Some(Saved {
        seed: value["seed"].as_i64()?,
        creative: value["creative"].as_bool().unwrap_or(false),
        position: [
            float(position.first()?),
            float(position.get(1)?),
            float(position.get(2)?),
        ],
        yaw: float(&player["yaw"]),
        pitch: float(&player["pitch"]),
        health: player["health"].as_f64().unwrap_or(20.0) as f32,
        food: player["food"].as_u64().unwrap_or(20).min(20) as u8,
        saturation: player["saturation"].as_f64().unwrap_or(5.0) as f32,
        experience: (
            player["experience_level"].as_u64().unwrap_or(0) as u32,
            player["experience_progress"].as_f64().unwrap_or(0.0) as f32,
            player["experience_total"].as_u64().unwrap_or(0) as u32,
        ),
        day_ticks: value["day_ticks"].as_f64().unwrap_or(1000.0),
        flying: player["flying"].as_bool().unwrap_or(false),
        selected: player["selected"].as_u64().unwrap_or(0).min(8) as usize,
        slots,
        ender,
        enchantment_seed: player["enchantment_seed"]
            .as_i64()
            .and_then(|seed| i32::try_from(seed).ok())
            .unwrap_or(0),
    })
}

/// Saved slots: a stack each, or null for an empty slot.
fn stacks(slots: &[Value]) -> Vec<Option<ItemStack>> {
    slots
        .iter()
        .map(|slot| {
            let id = slot["id"].as_str()?;
            let mut stack = ItemStack::new(id, slot["count"].as_u64()?.clamp(1, 255) as u8);
            stack.components = slot.get("components").filter(|c| !c.is_null()).cloned();
            Some(stack)
        })
        .collect()
}

fn slots_json(slots: &[Option<ItemStack>]) -> Vec<Value> {
    slots
        .iter()
        .map(|slot| match slot {
            Some(stack) => {
                json!({ "id": stack.id, "count": stack.count, "components": stack.components })
            }
            None => Value::Null,
        })
        .collect()
}

pub fn write(dir: &Path, saved: &Saved) -> std::io::Result<()> {
    let inventory = slots_json(&saved.slots);
    let value = json!({
        "seed": saved.seed,
        "creative": saved.creative,
        "day_ticks": saved.day_ticks,
        "player": {
            "position": saved.position,
            "yaw": saved.yaw,
            "pitch": saved.pitch,
            "health": saved.health,
            "food": saved.food,
            "saturation": saved.saturation,
            "experience_level": saved.experience.0,
            "experience_progress": saved.experience.1,
            "experience_total": saved.experience.2,
            "selected": saved.selected,
            "flying": saved.flying,
            "inventory": inventory,
            "ender_items": slots_json(&saved.ender),
            "enchantment_seed": saved.enchantment_seed,
        },
    });
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?;
    let partial = dir.join(format!("{FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(partial, dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(ender: Vec<Option<ItemStack>>) -> Saved {
        Saved {
            seed: 7,
            creative: false,
            position: [0.5, 64.0, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            health: 20.0,
            food: 20,
            saturation: 5.0,
            experience: (0, 0.0, 0),
            day_ticks: 1000.0,
            flying: false,
            selected: 0,
            slots: vec![None; 43],
            ender,
            enchantment_seed: -1_234_567,
        }
    }

    #[test]
    fn the_ender_chest_saves_with_the_player() {
        let dir = std::env::temp_dir().join(format!("minecraft-save-{}", std::process::id()));
        let mut sword = ItemStack::new("minecraft:diamond_sword", 1);
        sword.components = Some(json!({"minecraft:custom_name": "Edge"}));
        let mut ender = vec![None; 27];
        ender[0] = Some(ItemStack::new("minecraft:dirt", 12));
        ender[26] = Some(sword);
        write(&dir, &saved(ender.clone())).unwrap();
        let loaded = read(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded.ender, ender);
        // A world saved before ender chests opened has an empty one.
        let value = json!({"seed": 1, "player": {"position": [0, 0, 0], "inventory": []}});
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), value.to_string()).unwrap();
        let old = read(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(old.ender.is_empty());
    }

    #[test]
    fn the_enchantment_seed_saves_with_the_player() {
        let dir = std::env::temp_dir().join(format!("minecraft-seed-{}", std::process::id()));
        write(&dir, &saved(Vec::new())).unwrap();
        let loaded = read(&dir).unwrap();
        assert_eq!(loaded.enchantment_seed, -1_234_567);
        // A world saved before the seed was kept reads 0, which the game
        // replaces with a random one (`readAdditionalSaveData`).
        let value = json!({"seed": 1, "player": {"position": [0, 0, 0], "inventory": []}});
        std::fs::write(dir.join(FILE), value.to_string()).unwrap();
        let old = read(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(old.enchantment_seed, 0);
    }
}
