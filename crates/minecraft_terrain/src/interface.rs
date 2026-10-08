//! The item model helpers the block mesher needs, from the viewer's interface.
use crate::pack::{PackStack, ResourceId};
use anyhow::Result;

pub(crate) fn item_model_reference(value: &serde_json::Value) -> Option<&str> {
    match value.get("type")?.as_str()? {
        "minecraft:model" => value.get("model")?.as_str(),
        "minecraft:select" | "minecraft:range_dispatch" => value
            .get("fallback")
            .and_then(item_model_reference)
            .or_else(|| {
                value
                    .get("cases")
                    .or_else(|| value.get("entries"))?
                    .as_array()?
                    .iter()
                    .find_map(|entry| item_model_reference(&entry["model"]))
            }),
        "minecraft:condition" => value
            .get("on_false")
            .or_else(|| value.get("on_true"))
            .and_then(item_model_reference),
        "minecraft:composite" => value
            .get("models")?
            .as_array()?
            .iter()
            .find_map(item_model_reference),
        _ => None,
    }
}

/// The `minecraft:model` node `item_model_reference` picks.
pub(crate) fn item_model_node(value: &serde_json::Value) -> Option<&serde_json::Value> {
    match value.get("type")?.as_str()? {
        "minecraft:model" => Some(value),
        "minecraft:select" | "minecraft:range_dispatch" => value
            .get("fallback")
            .and_then(item_model_node)
            .or_else(|| {
                value
                    .get("cases")
                    .or_else(|| value.get("entries"))?
                    .as_array()?
                    .iter()
                    .find_map(|entry| item_model_node(&entry["model"]))
            }),
        "minecraft:condition" => value
            .get("on_false")
            .or_else(|| value.get("on_true"))
            .and_then(item_model_node),
        "minecraft:composite" => value
            .get("models")?
            .as_array()?
            .iter()
            .find_map(item_model_node),
        _ => None,
    }
}

/// The tint colours of the model node `item_model_reference` picks.
pub(crate) fn item_model_tints(
    packs: &PackStack,
    model: &serde_json::Value,
) -> Result<Vec<[u8; 3]>> {
    let model = item_model_node(model).unwrap_or(model);
    let Some(tints) = model.get("tints").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };
    tints
        .iter()
        .map(|tint| {
            let color = match tint.get("type").and_then(serde_json::Value::as_str) {
                Some("minecraft:constant") => tint
                    .get("value")
                    .and_then(serde_json::Value::as_i64)
                    .map(|color| color as u32),
                Some("minecraft:potion") => tint
                    .get("default")
                    .and_then(serde_json::Value::as_i64)
                    .map(|color| color as u32),
                Some(kind @ ("minecraft:grass" | "minecraft:foliage")) => {
                    let name = if kind == "minecraft:grass" {
                        "grass"
                    } else {
                        "foliage"
                    };
                    let id = ResourceId::parse(&format!("minecraft:colormap/{name}"))?;
                    let temp = tint
                        .get("temperature")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.5)
                        .clamp(0.0, 1.0);
                    let rain = tint
                        .get("downfall")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.5)
                        .clamp(0.0, 1.0)
                        * temp;
                    if let Some(bytes) = packs.texture(&id)? {
                        let map = image::load_from_memory(&bytes)?.to_rgba8();
                        if map.width() >= 256 && map.height() >= 256 {
                            let pixel = map.get_pixel(
                                ((1.0 - temp) * 255.0) as u32,
                                ((1.0 - rain) * 255.0) as u32,
                            );
                            Some(u32::from_be_bytes([0, pixel[0], pixel[1], pixel[2]]))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                // Every other source (a dye, a firework star's colours, a
                // map's) shows its default without the stack's data.
                _ => tint
                    .get("default")
                    .and_then(serde_json::Value::as_i64)
                    .map(|color| color as u32),
            }
            .unwrap_or(0xffffff);
            Ok([
                ((color >> 16) & 255) as u8,
                ((color >> 8) & 255) as u8,
                (color & 255) as u8,
            ])
        })
        .collect()
}
