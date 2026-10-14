//! Which of an item definition's models a held stack shows (26.3
//! `ItemModelResolver` with `ConditionalItemModel`, `SelectItemModel` and
//! `RangeSelectItemModel`): the properties a holder's state decides
//! (`minecraft:using_item`, `use_duration`, `crossbow/pull`), the stack's
//! own (`charge_type`) and the display context. A model other than the
//! one an idle stack shows is named by a key the item visuals resolve:
//! the item, `#`, and the model's reference.
use serde_json::Value;

/// What a held stack's model properties read of its holder.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeldState {
    /// `IsUsingItem`: the holder is using this very stack.
    pub using: bool,
    /// `UseDuration` (counting up): ticks it has been used, with the
    /// partial tick.
    pub use_ticks: f32,
    /// `CrossbowPull`: how far a crossbow is drawn, 0 to 1.
    pub pull: f32,
}

/// `CrossbowItem.ChargeType` of a stack: what its
/// `minecraft:charged_projectiles` hold.
pub fn charge_type(components: Option<&Value>) -> &'static str {
    let charged = components.and_then(|c| c["minecraft:charged_projectiles"].as_array());
    match charged {
        Some(list) if list.iter().any(|p| p["id"] == "minecraft:firework_rocket") => "rocket",
        Some(list) if !list.is_empty() => "arrow",
        _ => "none",
    }
}

/// The model node an item definition's `model` picks for `state` in a
/// display context (`gui`, `firstperson_righthand`...): a `minecraft:model`
/// or `minecraft:special` node.
pub fn select<'a>(value: &'a Value, context: &str, state: &HeldState, components: Option<&Value>) -> Option<&'a Value> {
    match value.get("type")?.as_str()? {
        "minecraft:model" | "minecraft:special" => Some(value),
        "minecraft:condition" => {
            let holds = match value["property"].as_str() {
                Some("minecraft:using_item") => state.using,
                _ => false,
            };
            select(&value[if holds { "on_true" } else { "on_false" }], context, state, components)
        }
        "minecraft:select" => {
            let wanted = match value["property"].as_str() {
                Some("minecraft:display_context") => Some(context),
                Some("minecraft:charge_type") => Some(charge_type(components)),
                _ => None,
            };
            let case = wanted.and_then(|wanted| {
                value["cases"].as_array()?.iter().find(|case| {
                    let when = &case["when"];
                    when.as_str() == Some(wanted) || when.as_array().is_some_and(|list| list.iter().any(|w| w.as_str() == Some(wanted)))
                })
            });
            match case {
                Some(case) => select(&case["model"], context, state, components),
                None => select(value.get("fallback")?, context, state, components),
            }
        }
        "minecraft:range_dispatch" => {
            let scale = value["scale"].as_f64().unwrap_or(1.0) as f32;
            let reading = match value["property"].as_str() {
                Some("minecraft:use_duration") => Some(state.use_ticks),
                Some("minecraft:crossbow/pull") => Some(state.pull),
                _ => None,
            };
            // The last entry whose threshold the scaled value reaches.
            let entry = reading.and_then(|reading| {
                let at = reading * scale;
                value["entries"].as_array()?.iter().filter(|e| e["threshold"].as_f64().is_some_and(|t| t as f32 <= at)).last()
            });
            match entry {
                Some(entry) => select(&entry["model"], context, state, components),
                None => select(value.get("fallback")?, context, state, components),
            }
        }
        "minecraft:composite" => value["models"].as_array()?.iter().find_map(|m| select(m, context, state, components)),
        _ => None,
    }
}

/// The item visuals' key for the model a stack shows: its item for the
/// model it shows idle, else the item with the picked model's reference.
pub fn model_key(definition: Option<&Value>, item: &str, context: &str, state: &HeldState, components: Option<&Value>) -> String {
    let Some(definition) = definition else { return item.to_owned() };
    let idle = crate::interface::item_model_reference(&definition["model"]);
    let picked = select(&definition["model"], context, state, components).and_then(|node| node["model"].as_str());
    match picked {
        Some(picked) if Some(picked) != idle => format!("{item}#{picked}"),
        _ => item.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_drawn_bow_shows_its_pull_stages() {
        let bow = json!({"model": {"type": "minecraft:condition", "property": "minecraft:using_item",
            "on_false": {"type": "minecraft:model", "model": "minecraft:item/bow"},
            "on_true": {"type": "minecraft:range_dispatch", "property": "minecraft:use_duration", "scale": 0.05,
                "entries": [{"model": {"type": "minecraft:model", "model": "minecraft:item/bow_pulling_1"}, "threshold": 0.65},
                            {"model": {"type": "minecraft:model", "model": "minecraft:item/bow_pulling_2"}, "threshold": 0.9}],
                "fallback": {"type": "minecraft:model", "model": "minecraft:item/bow_pulling_0"}}}});
        let key = |using, use_ticks| model_key(Some(&bow), "minecraft:bow", "gui", &HeldState { using, use_ticks, pull: 0.0 }, None);
        assert_eq!(key(false, 30.0), "minecraft:bow");
        assert_eq!(key(true, 5.0), "minecraft:bow#minecraft:item/bow_pulling_0");
        assert_eq!(key(true, 13.0), "minecraft:bow#minecraft:item/bow_pulling_1");
        assert_eq!(key(true, 18.0), "minecraft:bow#minecraft:item/bow_pulling_2");
        assert_eq!(charge_type(Some(&json!({"minecraft:charged_projectiles": [{"id": "minecraft:arrow"}]}))), "arrow");
    }
}
