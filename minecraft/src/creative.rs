//! The creative screen's tabs and what each holds, in the order 26.3's
//! `CreativeModeTabs.bootstrap` fills them (extracted from the game's code
//! by `tools/creative_tabs.py`), and the names and tooltip lines item
//! stacks show (`ItemStack.getStyledHoverName`, `getTooltipLines`).
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use minecraftoss_player::inventory::ItemStack;
use serde_json::Value;

const TABLES: &str = include_str!("../../crates/assets/data/minecraft/creative-tabs-26.3.json");

/// `CreativeModeTab.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Category,
    Hotbar,
    Search,
    Inventory,
}

/// A stack a tab shows.
pub struct Entry {
    pub id: String,
    pub components: Option<Value>,
    /// `TabVisibility`: whether the search tab shows it too.
    pub searched: bool,
}

pub struct Tab {
    /// Its title's translation key.
    pub title: String,
    pub top: bool,
    pub column: usize,
    pub icon: String,
    pub aligned_right: bool,
    pub show_title: bool,
    pub scroll_bar: bool,
    pub kind: Kind,
    /// `gui/container/creative_inventory/tab_<background>`.
    pub background: String,
    /// Shown only to operators with the Operator Items Tab option on.
    pub op_only: bool,
    pub items: Vec<Entry>,
    /// Stacks only the search tab shows (every enchanted book's level).
    pub search_only: Vec<Entry>,
}

pub struct Data {
    pub tabs: Vec<Tab>,
    tables: Value,
}

/// The tabs, read once.
pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        let tables: Value = serde_json::from_str(TABLES).unwrap_or(Value::Null);
        let entries = |value: &Value| -> Vec<Entry> {
            value
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .filter_map(|entry| {
                    Some(Entry {
                        id: entry["id"].as_str()?.to_owned(),
                        components: entry.get("components").cloned(),
                        searched: entry.get("tab_only").is_none(),
                    })
                })
                .collect()
        };
        let tabs = tables["tabs"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|tab| Tab {
                title: tab["title"].as_str().unwrap_or("").to_owned(),
                top: tab["row"] == "top",
                column: tab["column"].as_u64().unwrap_or(0) as usize,
                icon: tab["icon"].as_str().unwrap_or("").to_owned(),
                aligned_right: tab["aligned_right"] == true,
                show_title: tab["show_title"] != false,
                scroll_bar: tab["scroll_bar"] != false,
                kind: match tab["type"].as_str() {
                    Some("hotbar") => Kind::Hotbar,
                    Some("search") => Kind::Search,
                    Some("inventory") => Kind::Inventory,
                    _ => Kind::Category,
                },
                background: tab["background"].as_str().unwrap_or("items").to_owned(),
                op_only: tab["op_only"] == true,
                items: entries(&tab["items"]),
                search_only: entries(&tab["search_only"]),
            })
            .collect();
        Data { tabs, tables }
    })
}

impl Data {
    /// `CreativeModeTabs.tabs()`: those shown, in order; a category tab
    /// with nothing to show (the operator tab, for others) is left out.
    pub fn shown(&self, operator: bool) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&i| {
                let tab = &self.tabs[i];
                tab.kind != Kind::Category || !tab.items.is_empty() && (!tab.op_only || operator)
            })
            .collect()
    }

    /// `CreativeModeTabs.getDefaultTab`: building blocks.
    pub fn default_tab(&self) -> usize {
        0
    }

    pub fn search_tab(&self) -> usize {
        self.tabs
            .iter()
            .position(|tab| tab.kind == Kind::Search)
            .unwrap_or(0)
    }

    /// The search tab's stacks: every tab's searched stacks, in tab order,
    /// each kind once (`ItemStackLinkedSet.createTypeAndComponentsSet`).
    pub fn search_entries(&self, operator: bool) -> Vec<&Entry> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for tab in &self.tabs {
            if tab.kind != Kind::Category || tab.op_only && !operator {
                continue;
            }
            for entry in tab
                .items
                .iter()
                .filter(|e| e.searched)
                .chain(&tab.search_only)
            {
                let key = (
                    entry.id.as_str(),
                    entry.components.as_ref().map(Value::to_string),
                );
                if seen.insert(key) {
                    found.push(entry);
                }
            }
        }
        found
    }

    /// The tabs other than search that hold a stack, for its tooltip.
    pub fn tabs_holding(&self, stack: &ItemStack) -> Vec<&Tab> {
        self.tabs
            .iter()
            .filter(|tab| tab.kind == Kind::Category)
            .filter(|tab| {
                tab.items
                    .iter()
                    .chain(&tab.search_only)
                    .any(|entry| entry.id == stack.id && entry.components == stack.components)
            })
            .collect()
    }
}

/// ChatFormatting's colours.
pub const WHITE: u32 = 0xFFFFFF;
pub const GRAY: u32 = 0xAAAAAA;
pub const BLUE: u32 = 0x5555FF;
pub const RED: u32 = 0xFF5555;
pub const YELLOW: u32 = 0xFFFF55;
pub const AQUA: u32 = 0x55FFFF;
pub const LIGHT_PURPLE: u32 = 0xFF55FF;

fn named_color(name: &str) -> Option<u32> {
    Some(match name {
        "black" => 0x000000,
        "dark_blue" => 0x0000AA,
        "dark_green" => 0x00AA00,
        "dark_aqua" => 0x00AAAA,
        "dark_red" => 0xAA0000,
        "dark_purple" => 0xAA00AA,
        "gold" => 0xFFAA00,
        "gray" => GRAY,
        "dark_gray" => 0x555555,
        "blue" => BLUE,
        "green" => 0x55FF55,
        "aqua" => AQUA,
        "red" => RED,
        "light_purple" => LIGHT_PURPLE,
        "yellow" => YELLOW,
        "white" => WHITE,
        _ => {
            return name
                .strip_prefix('#')
                .and_then(|hex| u32::from_str_radix(hex, 16).ok());
        }
    })
}

/// A translation, with its `%s` and `%1$s` arguments filled in.
pub fn translate(language: &HashMap<String, String>, key: &str, args: &[String]) -> String {
    let template = language.get(key).map(String::as_str).unwrap_or(key);
    let mut out = String::new();
    let mut next = 0;
    let mut rest = template;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(tail) = rest.strip_prefix('%') {
            out.push('%');
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix('s') {
            out.push_str(args.get(next).map(String::as_str).unwrap_or(""));
            next += 1;
            rest = tail;
        } else if let Some(dollar) = rest.find("$s")
            && let Ok(index) = rest[..dollar].parse::<usize>()
        {
            out.push_str(args.get(index - 1).map(String::as_str).unwrap_or(""));
            rest = &rest[dollar + 2..];
        } else {
            out.push('%');
        }
    }
    out.push_str(rest);
    out
}

/// A text component's string and colour: a translation or plain text.
fn component(language: &HashMap<String, String>, value: &Value) -> Option<(String, Option<u32>)> {
    if let Some(text) = value.as_str() {
        return Some((text.to_owned(), None));
    }
    let text = if let Some(key) = value["translate"].as_str() {
        let args: Vec<String> = value["with"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .filter_map(|arg| component(language, arg).map(|(text, _)| text))
            .collect();
        translate(language, key, &args)
    } else {
        value["text"].as_str()?.to_owned()
    };
    Some((text, value["color"].as_str().and_then(named_color)))
}

fn components(stack: &ItemStack) -> &Value {
    static NONE: Value = Value::Null;
    stack.components.as_ref().unwrap_or(&NONE)
}

fn path(id: &str) -> &str {
    id.split_once(':').map_or(id, |(_, path)| path)
}

/// The potion a stack holds (`potion_contents`, or its bare potion id).
fn potion(stack: &ItemStack) -> Option<&str> {
    let contents = &components(stack)["minecraft:potion_contents"];
    contents["potion"].as_str().or_else(|| contents.as_str())
}

/// `PotionContents.getColor`, when the stack's potion has a colour of its
/// own: its custom colour, else its effects' mix.
pub fn potion_color(stack: &ItemStack) -> Option<u32> {
    let contents = &components(stack)["minecraft:potion_contents"];
    if let Some(color) = contents["custom_color"].as_i64() {
        return Some(color as u32 & 0xFFFFFF);
    }
    let potion = potion(stack)?;
    data().tables["potions"][potion]["color"]
        .as_u64()
        .map(|color| color as u32)
}

/// `ItemStack.getHoverName`.
pub fn name(language: &HashMap<String, String>, stack: &ItemStack) -> String {
    let parts = components(stack);
    for key in ["minecraft:custom_name", "minecraft:item_name"] {
        if let Some((text, _)) = component(language, &parts[key]) {
            return text;
        }
    }
    let item = path(&stack.id);
    if matches!(
        item,
        "potion" | "splash_potion" | "lingering_potion" | "tipped_arrow"
    ) {
        let name = potion(stack)
            .and_then(|potion| data().tables["potions"][potion]["name"].as_str())
            .unwrap_or("empty");
        return translate(
            language,
            &format!("item.minecraft.{item}.effect.{name}"),
            &[],
        );
    }
    minecraft_terrain::item_icons::item_name(language, &stack.id)
}

/// `ItemStack.hasFoil`: the glint override, else an item glinting of its
/// own (`Items`' `ENCHANTMENT_GLINT_OVERRIDE` defaults), else enchanted.
pub fn foil(stack: &ItemStack) -> bool {
    let parts = components(stack);
    if let Some(glint) = parts["minecraft:enchantment_glint_override"].as_bool() {
        return glint;
    }
    matches!(
        path(&stack.id),
        "enchanted_golden_apple"
            | "experience_bottle"
            | "written_book"
            | "nether_star"
            | "enchanted_book"
            | "end_crystal"
            | "debug_stick"
    ) || parts["minecraft:enchantments"]
        .as_object()
        .is_some_and(|map| !map.is_empty())
}

/// `ItemStack.getRarity`'s colour: the item's rarity, raised a step when
/// it is enchanted.
pub fn rarity_color(stack: &ItemStack) -> u32 {
    let parts = components(stack);
    let base = parts["minecraft:rarity"]
        .as_str()
        .or_else(|| data().tables["rarity"][stack.id.as_str()].as_str())
        .unwrap_or("common");
    let enchanted = parts["minecraft:enchantments"]
        .as_object()
        .is_some_and(|map| !map.is_empty());
    match (base, enchanted) {
        ("common" | "uncommon", true) | ("rare", false) => AQUA,
        ("rare" | "epic", true) | ("epic", false) => LIGHT_PURPLE,
        ("uncommon", false) => YELLOW,
        _ => WHITE,
    }
}

/// `StringUtil.formatTickDuration` at 20 ticks a second.
fn duration(ticks: i64) -> String {
    let seconds = ticks.max(0) / 20;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// `PotionContents.addPotionTooltip`'s effect lines.
fn effect_lines(
    language: &HashMap<String, String>,
    effects: &[(String, i64, i64)],
    scale: f64,
    lines: &mut Vec<(String, u32)>,
) {
    if effects.is_empty() {
        lines.push((translate(language, "effect.none", &[]), GRAY));
    }
    for (effect, ticks, amplifier) in effects {
        let mut line = translate(
            language,
            &format!("effect.{}", effect.replace(':', ".")),
            &[],
        );
        if *amplifier > 0 {
            let potency = translate(language, &format!("potion.potency.{amplifier}"), &[]);
            line = translate(language, "potion.withAmplifier", &[line, potency]);
        }
        // `endsWithin(20)`: instant effects show no time.
        if *ticks > 20 {
            let time = duration((*ticks as f64 * scale).floor() as i64);
            line = translate(language, "potion.withDuration", &[line, time]);
        }
        let harmful = data().tables["effects"][effect.as_str()]["category"] == "harmful";
        lines.push((line, if harmful { RED } else { BLUE }));
    }
}

/// The stack's tooltip: its name in its rarity's colour, then what its
/// components add.
pub fn tooltip(language: &HashMap<String, String>, stack: &ItemStack) -> Vec<(String, u32)> {
    let parts = components(stack);
    let mut lines = vec![(name(language, stack), rarity_color(stack))];
    let tables = &data().tables;
    let item = path(&stack.id);
    if matches!(
        item,
        "potion" | "splash_potion" | "lingering_potion" | "tipped_arrow"
    ) {
        let scale = match item {
            "lingering_potion" => 0.25,
            "tipped_arrow" => 0.125,
            _ => 1.0,
        };
        let effects: Vec<(String, i64, i64)> = potion(stack)
            .and_then(|potion| tables["potions"][potion]["effects"].as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .filter_map(|effect| {
                Some((
                    effect[0].as_str()?.to_owned(),
                    effect[1].as_i64()?,
                    effect[2].as_i64()?,
                ))
            })
            .collect();
        effect_lines(language, &effects, scale, &mut lines);
    }
    if let Some(amplifier) = parts["minecraft:ominous_bottle_amplifier"].as_i64() {
        let effects = [("minecraft:bad_omen".to_owned(), 120_000, amplifier)];
        effect_lines(language, &effects, 1.0, &mut lines);
    }
    if let Some(flight) = parts["minecraft:fireworks"]["flight_duration"].as_i64() {
        let label = translate(language, "item.minecraft.firework_rocket.flight", &[]);
        lines.push((format!("{label} {flight}"), GRAY));
    }
    if let Some(instrument) = parts["minecraft:instrument"].as_str() {
        let key = format!("instrument.{}", instrument.replace(':', "."));
        lines.push((translate(language, &key, &[]), GRAY));
    }
    if let Some(variant) = parts["minecraft:painting/variant"].as_str() {
        let painting = &tables["paintings"][variant];
        for field in ["title", "author"] {
            if let Some((text, color)) = component(language, &painting[field]) {
                lines.push((text, color.unwrap_or(WHITE)));
            }
        }
        let size = |field: &str| painting[field].as_i64().unwrap_or(1).to_string();
        let dimensions = translate(
            language,
            "painting.dimensions",
            &[size("width"), size("height")],
        );
        lines.push((dimensions, GRAY));
    }
    for key in ["minecraft:stored_enchantments", "minecraft:enchantments"] {
        let Some(map) = parts[key].as_object() else {
            continue;
        };
        for (enchantment, level) in map {
            let info = &tables["enchantments"][enchantment.as_str()];
            let mut line = translate(
                language,
                &format!("enchantment.{}", enchantment.replace(':', ".")),
                &[],
            );
            let level = level.as_i64().unwrap_or(1);
            // `Enchantment.getFullname`.
            if level != 1 || info["max"].as_i64() != Some(1) {
                line.push(' ');
                line.push_str(&translate(
                    language,
                    &format!("enchantment.level.{level}"),
                    &[],
                ));
            }
            lines.push((line, if info["curse"] == true { RED } else { GRAY }));
        }
    }
    lines
}

/// Whether the search tab's text finds a stack (`FullTextSearchTree`):
/// any of its tooltip lines holding the text, or with a colon, its id's
/// namespace holding what is before it and its path or lines what after.
pub fn matches(language: &HashMap<String, String>, stack: &ItemStack, text: &str) -> bool {
    let text = text.to_lowercase();
    let lines: Vec<String> = tooltip(language, stack)
        .into_iter()
        .map(|(line, _)| line.trim().to_lowercase())
        .filter(|line| !line.is_empty())
        .collect();
    match text.split_once(':') {
        None => lines.iter().any(|line| line.contains(&text)),
        Some((namespace, rest)) => {
            let (namespace, rest) = (namespace.trim(), rest.trim());
            let (item_namespace, item_path) =
                stack.id.split_once(':').unwrap_or(("minecraft", &stack.id));
            item_namespace.contains(namespace)
                && (item_path.contains(rest) || lines.iter().any(|line| line.contains(rest)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_are_vanillas() {
        let data = data();
        let titles: Vec<&str> = data.tabs.iter().map(|tab| tab.title.as_str()).collect();
        assert_eq!(titles.len(), 14);
        assert_eq!(titles[0], "itemGroup.buildingBlocks");
        assert_eq!(data.tabs[0].items[0].id, "minecraft:oak_log");
        assert_eq!(data.tabs[data.search_tab()].kind, Kind::Search);
        // The operator tab is hidden from others.
        assert_eq!(data.shown(false).len(), 13);
        assert_eq!(data.shown(true).len(), 14);
        // Max-level books show in their tab only, every level in search.
        let search = data.search_entries(false);
        let books = search
            .iter()
            .filter(|e| e.id == "minecraft:enchanted_book")
            .count();
        assert!(books > 100);
    }

    #[test]
    fn names_and_lines() {
        let mut language = HashMap::new();
        for (key, text) in [
            (
                "item.minecraft.potion.effect.swiftness",
                "Potion of Swiftness",
            ),
            ("effect.minecraft.speed", "Speed"),
            ("potion.withDuration", "%s (%s)"),
            ("potion.withAmplifier", "%s %s"),
            ("potion.potency.1", "II"),
            ("enchantment.minecraft.sharpness", "Sharpness"),
            ("enchantment.level.5", "V"),
        ] {
            language.insert(key.to_owned(), text.to_owned());
        }
        let mut potion = ItemStack::new("minecraft:potion", 1);
        potion.components = Some(
            serde_json::json!({"minecraft:potion_contents": {"potion": "minecraft:long_swiftness"}}),
        );
        let lines = tooltip(&language, &potion);
        assert_eq!(lines[0].0, "Potion of Swiftness");
        assert_eq!(lines[1], ("Speed (08:00)".to_owned(), BLUE));
        assert_eq!(potion_color(&potion), Some(3402751));
        let mut book = ItemStack::new("minecraft:enchanted_book", 1);
        book.components =
            Some(serde_json::json!({"minecraft:stored_enchantments": {"minecraft:sharpness": 5}}));
        let lines = tooltip(&language, &book);
        assert_eq!(lines[0].1, AQUA);
        assert_eq!(lines[1], ("Sharpness V".to_owned(), GRAY));
        assert!(matches(&language, &book, "sharp"));
        assert!(matches(&language, &book, "minecraft:book"));
        assert!(!matches(&language, &book, "axe"));
    }
}
