//! Item stacks crossing between the player and the level. The player's
//! inventory keeps a stack's component patch as the JSON the item codecs
//! read (`DataComponentPatch.CODEC` through `JsonOps`), the level keeps it
//! as saved NBT (the same codec through `NbtOps`), as containers' `Items`,
//! item entities and region files hold it. Every server path that moves a
//! stack from one side to the other converts here, so enchantments, names,
//! potions, colours and patterns survive the trip both ways.

use minecraftoss_core::nbt::Tag;
use serde_json::Value;

/// A stack as the level holds it.
pub type LevelStack = minecraftoss_core::item::ItemStack;
/// A stack as the player's inventory holds it.
pub type PlayerStack = minecraftoss_player::inventory::ItemStack;

/// NBT as JSON: numbers keep their value, bytes stand for booleans too,
/// arrays become lists.
pub fn tag_json(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(v) => Value::from(*v),
        Tag::Short(v) => Value::from(*v),
        Tag::Int(v) => Value::from(*v),
        Tag::Long(v) => Value::from(*v),
        Tag::Float(v) => Value::from(f64::from(*v)),
        Tag::Double(v) => Value::from(*v),
        Tag::ByteArray(a) => Value::Array(a.iter().map(|&v| Value::from(v)).collect()),
        Tag::String(s) => Value::from(s.clone()),
        Tag::List(list) => Value::Array(list.iter().map(tag_json).collect()),
        Tag::Compound(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), tag_json(v))).collect()),
        Tag::IntArray(a) => Value::Array(a.iter().map(|&v| Value::from(v)).collect()),
        Tag::LongArray(a) => Value::Array(a.iter().map(|&v| Value::from(v)).collect()),
    }
}

/// JSON as NBT, as `NbtOps` writes what a codec encodes: whole numbers as
/// ints (longs past an int's range), others as doubles, booleans as bytes.
pub fn json_tag(value: &Value) -> Tag {
    match value {
        Value::Object(map) => Tag::Compound(map.iter().map(|(k, v)| (k.clone(), json_tag(v))).collect()),
        Value::Array(list) => Tag::List(list.iter().map(json_tag).collect()),
        Value::String(s) => Tag::String(s.clone()),
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(v) => i32::try_from(v).map_or(Tag::Long(v), Tag::Int),
            None => Tag::Double(n.as_f64().unwrap_or(0.0)),
        },
        Value::Null => Tag::Compound(Default::default()),
    }
}

/// Where the item components' `Codec.BOOL` fields sit (each component's
/// codec in `DataComponents`): NBT keeps them as bytes, and JSON as
/// `true`/`false`, which is what the client's readers (`hasFoil`'s glint
/// override, say) and stack comparisons expect back. `*` is each element
/// of a list.
const BOOLEANS: &[&[&str]] = &[
    &["minecraft:enchantment_glint_override"],
    &["minecraft:custom_model_data", "flags", "*"],
    &["minecraft:tooltip_display", "hide_tooltip"],
    &["minecraft:food", "can_always_eat"],
    &["minecraft:consumable", "has_consume_particles"],
    &["minecraft:use_effects", "can_sprint"],
    &["minecraft:use_effects", "interact_vibrations"],
    &["minecraft:tool", "can_destroy_blocks_in_creative"],
    &["minecraft:tool", "rules", "*", "correct_for_drops"],
    &["minecraft:equippable", "dispensable"],
    &["minecraft:equippable", "swappable"],
    &["minecraft:equippable", "damage_on_hurt"],
    &["minecraft:equippable", "equip_on_interact"],
    &["minecraft:equippable", "can_be_sheared"],
    &["minecraft:piercing_weapon", "deals_knockback"],
    &["minecraft:piercing_weapon", "dismounts"],
    &["minecraft:potion_contents", "custom_effects", "*", "ambient"],
    &["minecraft:potion_contents", "custom_effects", "*", "show_particles"],
    &["minecraft:potion_contents", "custom_effects", "*", "show_icon"],
    &["minecraft:written_book_content", "resolved"],
    &["minecraft:lodestone_tracker", "tracked"],
    &["minecraft:firework_explosion", "has_trail"],
    &["minecraft:firework_explosion", "has_twinkle"],
    &["minecraft:fireworks", "explosions", "*", "has_trail"],
    &["minecraft:fireworks", "explosions", "*", "has_twinkle"],
    &["minecraft:sign_text_front", "has_glowing_text"],
    &["minecraft:sign_text_back", "has_glowing_text"],
];

/// Text components among the item components (`ComponentSerialization`
/// in names, lore, book pages, plain or `Filterable`, and sign lines).
const TEXTS: &[&[&str]] = &[
    &["minecraft:custom_name"],
    &["minecraft:item_name"],
    &["minecraft:lore", "*"],
    &["minecraft:written_book_content", "pages", "*"],
    &["minecraft:written_book_content", "pages", "*", "raw"],
    &["minecraft:written_book_content", "pages", "*", "filtered"],
    &["minecraft:sign_text_front", "messages", "*"],
    &["minecraft:sign_text_front", "filtered_messages", "*"],
    &["minecraft:sign_text_back", "messages", "*"],
    &["minecraft:sign_text_back", "filtered_messages", "*"],
];

/// A text component's `Codec.BOOL` fields: `Style`'s formats and
/// `NbtContents`' flags.
const TEXT_BOOLEANS: [&str; 7] = ["bold", "italic", "underlined", "strikethrough", "obfuscated", "interpret", "plain"];

/// Turns a text component's saved bytes back into booleans, through its
/// siblings, arguments, separator and hover text.
fn restore_text(text: &mut Value) {
    match text {
        Value::Array(list) => list.iter_mut().for_each(restore_text),
        Value::Object(map) => {
            for key in TEXT_BOOLEANS {
                if let Some(field) = map.get_mut(key) {
                    if let Some(n) = field.as_i64() {
                        *field = Value::Bool(n != 0);
                    }
                }
            }
            for key in ["extra", "with", "separator"] {
                if let Some(part) = map.get_mut(key) {
                    restore_text(part);
                }
            }
            if let Some(hover) = map.get_mut("hover_event").and_then(|h| h.get_mut("value")) {
                restore_text(hover);
            }
        }
        _ => {}
    }
}

/// Stacks inside components (`ItemStackTemplate.CODEC` in
/// `ItemContainerContents.Slot`, `BundleContents`, `ChargedProjectiles`
/// and `UseRemainder`), whose own components are read the same way.
const NESTED_STACKS: &[&[&str]] = &[
    &["minecraft:container", "*", "item"],
    &["minecraft:bundle_contents", "*"],
    &["minecraft:charged_projectiles", "*"],
    &["minecraft:use_remainder"],
];

/// Runs `f` on every value at `path` under `value`.
fn visit(value: &mut Value, path: &[&str], f: &mut dyn FnMut(&mut Value)) {
    match path.split_first() {
        None => f(value),
        Some((&"*", rest)) => {
            if let Value::Array(list) = value {
                for element in list {
                    visit(element, rest, f);
                }
            }
        }
        Some((key, rest)) => {
            if let Some(field) = value.get_mut(*key) {
                visit(field, rest, f);
            }
        }
    }
}

/// Turns the saved bytes of boolean fields back into booleans, in nested
/// stacks too.
fn restore_booleans(components: &mut Value) {
    for path in BOOLEANS {
        visit(components, path, &mut |field| {
            if let Some(n) = field.as_i64() {
                *field = Value::Bool(n != 0);
            }
        });
    }
    for path in TEXTS {
        visit(components, path, &mut restore_text);
    }
    for path in NESTED_STACKS {
        visit(components, path, &mut |stack| {
            if let Some(components) = stack.get_mut("components") {
                restore_booleans(components);
            }
        });
    }
}

/// A component patch as the level saves it; none when it is empty, as
/// `ItemStack.MAP_CODEC` leaves an empty patch out.
pub fn components_tag(components: &Value) -> Option<Tag> {
    match json_tag(components) {
        Tag::Compound(map) if map.is_empty() => None,
        tag => Some(tag),
    }
}

/// A saved component patch as the player's inventory keeps it; none when
/// it is empty.
pub fn components_json(tag: &Tag) -> Option<Value> {
    let mut json = tag_json(tag);
    if json.as_object().is_some_and(serde_json::Map::is_empty) {
        return None;
    }
    restore_booleans(&mut json);
    Some(json)
}

/// A stack's component patch as JSON text, as the client hears of item
/// entities.
pub fn components_text(stack: &LevelStack) -> Option<String> {
    stack.components.as_ref().and_then(components_json).map(|json| json.to_string())
}

/// A level stack from an item, a count and its components as JSON text
/// (what the client hands over); unreadable components are left off.
pub fn level_stack(item: &str, count: i32, components: Option<&str>) -> LevelStack {
    let mut stack = LevelStack::new(item, count);
    stack.components = components.and_then(|text| serde_json::from_str::<Value>(text).ok()).as_ref().and_then(components_tag);
    stack
}

/// A player's stack as the level holds it.
pub fn to_level(stack: &PlayerStack) -> LevelStack {
    let mut level = LevelStack::new(&stack.id, i32::from(stack.count));
    level.components = stack.components.as_ref().and_then(components_tag);
    level
}

/// `ItemStack.getMaxStackSize` for the player's stack: a
/// `max_stack_size` component, else the item's default (`default_max`),
/// within 1 to 99.
pub fn max_stack(id: &str, components: Option<&Value>, default_max: impl Fn(&str) -> i32) -> u8 {
    components
        .and_then(|c| c.get("minecraft:max_stack_size"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| i64::from(default_max(id)))
        .clamp(1, 99) as u8
}

/// A level stack as the player's inventory holds it (none when empty),
/// its count within 1 to 99 and its size limit from its components or
/// `default_max`.
pub fn to_player(stack: &LevelStack, default_max: impl Fn(&str) -> i32) -> Option<PlayerStack> {
    if stack.is_empty() {
        return None;
    }
    let components = stack.components.as_ref().and_then(components_json);
    let max = max_stack(&stack.id, components.as_ref(), default_max);
    Some(PlayerStack { id: stack.id.clone(), count: stack.count.clamp(1, 99) as u8, max, components })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Components a survival player carries: an enchanted, renamed and
    /// repaired sword, a potion, dyed leather and a banner.
    fn carried() -> Vec<PlayerStack> {
        let stack = |id: &str, max: u8, components: Value| PlayerStack { id: id.to_owned(), count: 1, max, components: Some(components) };
        vec![
            stack(
                "minecraft:diamond_sword",
                1,
                json!({
                    "minecraft:enchantments": {"minecraft:sharpness": 5, "minecraft:unbreaking": 3},
                    "minecraft:custom_name": {"text": "Excalibur", "color": "gold", "italic": false, "extra": [{"text": "!", "bold": true}]},
                    "minecraft:lore": ["A plain line", {"text": "An old blade", "italic": true}],
                    "minecraft:repair_cost": 3,
                    "minecraft:damage": 120
                }),
            ),
            stack("minecraft:potion", 1, json!({"minecraft:potion_contents": {"potion": "minecraft:long_swiftness"}})),
            stack(
                "minecraft:splash_potion",
                1,
                json!({"minecraft:potion_contents": {"custom_color": 16711680, "custom_effects": [{"id": "minecraft:speed", "amplifier": 1, "duration": 600, "show_particles": false}]}}),
            ),
            stack("minecraft:leather_chestplate", 1, json!({"minecraft:dyed_color": 3847130})),
            stack(
                "minecraft:white_banner",
                16,
                json!({"minecraft:banner_patterns": [{"pattern": "minecraft:stripe_bottom", "color": "red"}, {"pattern": "minecraft:creeper", "color": "black"}]}),
            ),
            stack("minecraft:enchanted_book", 1, json!({"minecraft:stored_enchantments": {"minecraft:mending": 1}, "minecraft:enchantment_glint_override": false})),
            stack(
                "minecraft:firework_rocket",
                64,
                json!({"minecraft:fireworks": {"flight_duration": 2, "explosions": [{"shape": "star", "colors": [11743532], "has_trail": true, "has_twinkle": false}]}}),
            ),
            stack(
                "minecraft:shulker_box",
                1,
                json!({"minecraft:container": [{"slot": 0, "item": {"id": "minecraft:golden_apple", "count": 3, "components": {"minecraft:enchantment_glint_override": true}}}]}),
            ),
        ]
    }

    /// A player's stack into the level and back is the stack it was.
    #[test]
    fn player_stacks_round_trip_through_the_level() {
        let default_max = |id: &str| if id.ends_with("banner") { 16 } else if id.ends_with("rocket") { 64 } else { 1 };
        for stack in carried() {
            let level = to_level(&stack);
            assert!(matches!(level.components, Some(Tag::Compound(_))), "saved as a compound: {level:?}");
            assert_eq!(to_player(&level, default_max).as_ref(), Some(&stack));
            // As a container slot: saved and read back.
            let (_, read) = LevelStack::from_tag(&level.to_tag(4)).expect("a slot");
            assert_eq!(read, level);
            // As the client hears of an item entity.
            let text = components_text(&level).expect("components");
            assert_eq!(level_stack(&stack.id, 1, Some(&text)), level);
        }
    }

    /// Saved NBT's types land as the codecs read them: the levels as ints,
    /// the booleans as bytes.
    #[test]
    fn components_save_with_nbt_types() {
        let sword = &carried()[0];
        let tag = to_level(sword).components.expect("components");
        let level = tag.get("minecraft:enchantments").and_then(|e| e.get("minecraft:sharpness"));
        assert_eq!(level, Some(&Tag::Int(5)));
        let book = to_level(&carried()[5]).components.expect("components");
        assert_eq!(book.get("minecraft:enchantment_glint_override"), Some(&Tag::Byte(0)));
        assert_eq!(json_tag(&json!(5_000_000_000_i64)), Tag::Long(5_000_000_000));
        assert_eq!(json_tag(&json!(0.5)), Tag::Double(0.5));
    }

    /// An empty patch is no patch, so stacks keep stacking.
    #[test]
    fn empty_components_are_none() {
        let plain = PlayerStack { id: "minecraft:stone".to_owned(), count: 12, max: 64, components: Some(json!({})) };
        assert_eq!(to_level(&plain).components, None);
        let level = LevelStack::new("minecraft:stone", 12);
        let back = to_player(&level, |_| 64).expect("a stack");
        assert_eq!((back.count, back.max, back.components), (12, 64, None));
        assert_eq!(to_player(&LevelStack::empty(), |_| 64), None);
    }

    /// The size limit follows a `max_stack_size` component.
    #[test]
    fn max_stack_reads_the_component() {
        let level = level_stack("minecraft:ender_pearl", 3, Some(r#"{"minecraft:max_stack_size": 64}"#));
        assert_eq!(to_player(&level, |_| 16).map(|s| s.max), Some(64));
        assert_eq!(max_stack("minecraft:ender_pearl", None, |_| 16), 16);
    }
}
