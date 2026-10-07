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
}

const FILE: &str = "level.json";

pub fn read(dir: &Path) -> Option<Saved> {
    let value: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).ok()?).ok()?;
    let player = &value["player"];
    let float = |v: &Value| v.as_f64().unwrap_or(0.0);
    let position = player["position"].as_array()?;
    let slots = player["inventory"]
        .as_array()?
        .iter()
        .map(|slot| {
            let id = slot["id"].as_str()?;
            let mut stack = ItemStack::new(id, slot["count"].as_u64()?.clamp(1, 255) as u8);
            stack.components = slot.get("components").filter(|c| !c.is_null()).cloned();
            Some(stack)
        })
        .collect();
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
    })
}

pub fn write(dir: &Path, saved: &Saved) -> std::io::Result<()> {
    let inventory: Vec<Value> = saved
        .slots
        .iter()
        .map(|slot| match slot {
            Some(stack) => {
                json!({ "id": stack.id, "count": stack.count, "components": stack.components })
            }
            None => Value::Null,
        })
        .collect();
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
        },
    });
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?;
    let partial = dir.join(format!("{FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(partial, dir.join(FILE))
}
