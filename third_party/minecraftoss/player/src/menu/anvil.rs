//! `AnvilMenu` (26.3) on `ItemCombinerMenu`: the input, the addition and
//! the result, whose `createResult` repairs with the input's material,
//! combines two items' durability and enchantments, applies a book, and
//! renames, at a cost in levels (one data value) that grows with the
//! items' `repair_cost`; taking the result pays for it and may wear the
//! anvil down.
//!
//! What the anvil reads of the registries comes as [`EnchantmentRules`]:
//! the enchantments' definitions and tags, and the items' default
//! `max_damage` and `repairable`. A menu without them computes no result.
//! A server's menu is at its block ([`AnvilMenu::set_at_block`]): only it
//! wears the anvil and returns the inputs as it closes.
//!
//! Not simulated: the text filter a rename passes on a server.
use super::{
    BlockRequest, Menu, MenuContext, OwnSlots, SlotDef, clear_own_slots, combiner, return_carried,
    set, set_changed, standard_inventory_slots,
};
use crate::inventory::ItemStack;
use serde_json::{Map, Value};
use std::sync::Arc;

const INPUT: usize = 0;
const ADDITION: usize = 1;
/// The result's menu slot. The main inventory is 3-29, the hotbar 30-38.
pub const RESULT: usize = 2;

/// `AnvilMenu.MAX_NAME_LENGTH`.
pub const MAX_NAME_LENGTH: usize = 50;
/// The cost from which a survival player is refused ("Too Expensive!").
pub const TOO_EXPENSIVE: i32 = 40;
/// `AnvilMenu`'s chance that a use damages the anvil.
const DAMAGE_CHANCE: f32 = 0.12;
/// `LevelEvent.SOUND_ANVIL_USED`.
pub const ANVIL_USED: i32 = 1030;
/// `LevelEvent.SOUND_ANVIL_BROKEN`.
pub const ANVIL_BROKEN: i32 = 1029;

const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";
const ENCHANTMENTS: &str = "minecraft:enchantments";
const STORED_ENCHANTMENTS: &str = "minecraft:stored_enchantments";
const CUSTOM_NAME: &str = "minecraft:custom_name";
const REPAIR_COST: &str = "minecraft:repair_cost";
const DAMAGE: &str = "minecraft:damage";
const MAX_DAMAGE: &str = "minecraft:max_damage";

/// The enchantment registry and the items' defaults as the anvil and the
/// grindstone read them. Enchantments and items are named by id.
pub trait EnchantmentRules: Send + Sync {
    /// `Enchantment.getMaxLevel`.
    fn max_level(&self, enchantment: &str) -> i32;
    /// `Enchantment.getAnvilCost`.
    fn anvil_cost(&self, enchantment: &str) -> i32;
    /// `Enchantment.getMinCost`.
    fn min_cost(&self, enchantment: &str, level: i32) -> i32;
    /// `Enchantment.canEnchant`: the item is one it supports.
    fn can_enchant(&self, enchantment: &str, item: &str) -> bool;
    /// `Enchantment.areCompatible`.
    fn compatible(&self, a: &str, b: &str) -> bool;
    /// In the enchantment tag `#curse`.
    fn is_curse(&self, enchantment: &str) -> bool;
    /// The item's default `max_damage`; 0 for none.
    fn max_damage(&self, item: &str) -> i32;
    /// The item is in the item tag (`minecraft:...`, without `#`).
    fn in_item_tag(&self, tag: &str, item: &str) -> bool;
}

/// The default `repairable` of an item (`Item.Properties.repairable`): the
/// item tag (`#...`) or item that repairs it. Tools and spears by their
/// `ToolMaterial`, armour by its `ArmorMaterials`, and the items that name
/// their own.
pub fn repair_items(item: &str) -> Option<&'static str> {
    let path = item.strip_prefix("minecraft:")?;
    let special = match path {
        "elytra" => Some("minecraft:phantom_membrane"),
        "mace" => Some("minecraft:breeze_rod"),
        "shield" => Some("#minecraft:wooden_tool_materials"),
        "turtle_helmet" => Some("#minecraft:repairs_turtle_helmet"),
        "wolf_armor" => Some("#minecraft:repairs_wolf_armor"),
        _ => None,
    };
    if special.is_some() {
        return special;
    }
    let (material, kind) = path.split_once('_')?;
    match kind {
        "sword" | "shovel" | "pickaxe" | "axe" | "hoe" | "spear" => Some(match material {
            "wooden" => "#minecraft:wooden_tool_materials",
            "stone" => "#minecraft:stone_tool_materials",
            "copper" => "#minecraft:copper_tool_materials",
            "iron" => "#minecraft:iron_tool_materials",
            "golden" => "#minecraft:gold_tool_materials",
            "diamond" => "#minecraft:diamond_tool_materials",
            "netherite" => "#minecraft:netherite_tool_materials",
            _ => return None,
        }),
        "helmet" | "chestplate" | "leggings" | "boots" => Some(match material {
            "leather" => "#minecraft:repairs_leather_armor",
            "copper" => "#minecraft:repairs_copper_armor",
            "chainmail" => "#minecraft:repairs_chain_armor",
            "iron" => "#minecraft:repairs_iron_armor",
            "golden" => "#minecraft:repairs_gold_armor",
            "diamond" => "#minecraft:repairs_diamond_armor",
            "netherite" => "#minecraft:repairs_netherite_armor",
            _ => return None,
        }),
        _ => None,
    }
}

/// A component of the stack's patch; none when absent or removed.
pub(super) fn component<'a>(stack: &'a ItemStack, key: &str) -> Option<&'a Value> {
    stack
        .components
        .as_ref()?
        .get(key)
        .filter(|value| !value.is_null())
}

/// Write a component of the patch, or remove it (`None`, or the default
/// the patch leaves out); an empty patch is none.
pub(super) fn set_component(stack: &mut ItemStack, key: &str, value: Option<Value>) {
    let patch = stack
        .components
        .get_or_insert_with(|| Value::Object(Map::new()));
    if let Some(map) = patch.as_object_mut() {
        match value {
            Some(value) => {
                map.insert(key.to_owned(), value);
            }
            None => {
                map.remove(key);
            }
        }
    }
    if stack
        .components
        .as_ref()
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        stack.components = None;
    }
}

fn int_component(stack: &ItemStack, key: &str) -> i32 {
    component(stack, key).and_then(Value::as_i64).unwrap_or(0) as i32
}

/// `REPAIR_COST`, 0 by default.
pub(super) fn repair_cost(stack: &ItemStack) -> i32 {
    int_component(stack, REPAIR_COST)
}

pub(super) fn set_repair_cost(stack: &mut ItemStack, cost: i32) {
    set_component(stack, REPAIR_COST, (cost != 0).then(|| Value::from(cost)));
}

/// `AnvilMenu.calculateIncreasedRepairCost`: twice and one more, at most
/// `Integer.MAX_VALUE`.
pub fn increased_repair_cost(cost: i32) -> i32 {
    (i64::from(cost) * 2 + 1).min(i64::from(i32::MAX)) as i32
}

/// `EnchantmentHelper.getComponentType`: an enchanted book's are stored.
fn enchantments_key(stack: &ItemStack) -> &'static str {
    if stack.id == ENCHANTED_BOOK {
        STORED_ENCHANTMENTS
    } else {
        ENCHANTMENTS
    }
}

/// `EnchantmentHelper.canStoreEnchantments`: the stack has its kind of
/// enchantments, as every item does unless its patch removes them.
fn can_store_enchantments(stack: &ItemStack) -> bool {
    let key = enchantments_key(stack);
    let patch = stack.components.as_ref();
    !patch.is_some_and(|patch| {
        patch.get(format!("!{key}").as_str()).is_some()
            || patch.get(key).is_some_and(Value::is_null)
    })
}

/// `EnchantmentHelper.getEnchantmentsForCrafting`: (id, level) in the
/// stack's own order.
pub(super) fn enchantments(stack: &ItemStack) -> Vec<(String, i32)> {
    component(stack, enchantments_key(stack))
        .and_then(Value::as_object)
        .map_or_else(Vec::new, |map| {
            map.iter()
                .map(|(id, level)| (id.clone(), level.as_i64().unwrap_or(0) as i32))
                .collect()
        })
}

/// `EnchantmentHelper.setEnchantments`; none is the default.
pub(super) fn set_enchantments(stack: &mut ItemStack, list: &[(String, i32)]) {
    let map: Map<String, Value> = list
        .iter()
        .map(|(id, level)| (id.clone(), Value::from(*level)))
        .collect();
    let key = enchantments_key(stack);
    set_component(stack, key, (!map.is_empty()).then_some(Value::Object(map)));
}

/// `ItemEnchantments.Mutable.set`: a level of 0 or less removes it, and
/// none is above 255.
pub(super) fn set_level(list: &mut Vec<(String, i32)>, id: &str, level: i32) {
    if level <= 0 {
        list.retain(|(other, _)| other != id);
    } else if let Some(entry) = list.iter_mut().find(|(other, _)| other == id) {
        entry.1 = level.min(255);
    } else {
        list.push((id.to_owned(), level.min(255)));
    }
}

fn level_of(list: &[(String, i32)], id: &str) -> i32 {
    list.iter()
        .find(|(other, _)| other == id)
        .map_or(0, |(_, level)| *level)
}

/// `ItemStack.getMaxDamage`: the patch's, else the item's.
pub(super) fn max_damage(rules: &dyn EnchantmentRules, stack: &ItemStack) -> i32 {
    match component(stack, MAX_DAMAGE).and_then(Value::as_i64) {
        Some(max) => max as i32,
        None => rules.max_damage(&stack.id),
    }
}

/// `ItemStack.isDamageableItem`: it has a maximum and is not unbreakable.
pub(super) fn is_damageable(rules: &dyn EnchantmentRules, stack: &ItemStack) -> bool {
    max_damage(rules, stack) > 0 && component(stack, "minecraft:unbreakable").is_none()
}

/// `ItemStack.getDamageValue`, within the maximum.
pub(super) fn damage_value(rules: &dyn EnchantmentRules, stack: &ItemStack) -> i32 {
    int_component(stack, DAMAGE).clamp(0, max_damage(rules, stack).max(0))
}

/// `ItemStack.setDamageValue`, within the maximum; 0 is the default.
pub(super) fn set_damage(rules: &dyn EnchantmentRules, stack: &mut ItemStack, damage: i32) {
    let damage = damage.clamp(0, max_damage(rules, stack).max(0));
    set_component(stack, DAMAGE, (damage != 0).then(|| Value::from(damage)));
}

/// `StringUtil.isBlank`.
fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// `AnvilMenu.validateName`: the text without the characters chat refuses
/// (`StringUtil.filterText`), if it is at most 50 long.
pub fn validate_name(name: &str) -> Option<String> {
    let filtered: String = name
        .chars()
        .filter(|&c| c != '§' && c >= ' ' && c != '\u{7f}')
        .collect();
    (filtered.encode_utf16().count() <= MAX_NAME_LENGTH).then_some(filtered)
}

/// The text of a stack's custom name, when it is plain text.
fn custom_name(stack: &ItemStack) -> Option<&str> {
    let name = component(stack, CUSTOM_NAME)?;
    name.as_str()
        .or_else(|| name.get("text").and_then(Value::as_str))
}

/// `AnvilMenu` (`anvil`).
#[derive(Clone)]
pub struct AnvilMenu {
    slots: Vec<SlotDef>,
    /// The input, the addition, then the result.
    items: OwnSlots,
    rules: Option<Arc<dyn EnchantmentRules>>,
    /// `access` is a block's (a server's menu).
    at_block: bool,
    /// The data value `cost`.
    cost: i32,
    /// `repairItemCountCost`: how many of the addition a repair uses.
    repair_item_count_cost: i32,
    /// `itemName`: the name the screen asked for.
    item_name: Option<String>,
    /// `onlyRenaming`.
    only_renaming: bool,
}

impl AnvilMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(INPUT, 27, 47),
            SlotDef::own(ADDITION, 76, 47),
            SlotDef::own(RESULT, 134, 47),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(3, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            rules: None,
            at_block: false,
            cost: 0,
            repair_item_count_cost: 0,
            item_name: None,
            only_renaming: false,
        }
    }

    /// The registries the results come from.
    pub fn set_rules(&mut self, rules: Option<Arc<dyn EnchantmentRules>>) {
        self.rules = rules;
    }

    /// The menu is at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// `itemName`.
    pub fn item_name(&self) -> Option<&str> {
        self.item_name.as_deref()
    }

    /// `getCost`.
    pub fn cost(&self) -> i32 {
        self.cost
    }

    /// `ItemStack.isValidRepairItem` by the input's `repairable`.
    fn repairs(rules: &dyn EnchantmentRules, input: &ItemStack, addition: &ItemStack) -> bool {
        repair_items(&input.id).is_some_and(|items| match items.strip_prefix('#') {
            Some(tag) => rules.in_item_tag(tag, &addition.id),
            None => items == addition.id,
        })
    }

    /// `createResult`: what the inputs make, and its cost; nothing, at a
    /// cost of 0, when they make nothing.
    fn create_result(&mut self, cx: &MenuContext) {
        self.only_renaming = false;
        self.cost = 1;
        let input = self.items.get(INPUT).cloned();
        let (Some(input), Some(rules)) = (input.filter(can_store_enchantments), self.rules.clone())
        else {
            self.fail();
            return;
        };
        let rules = rules.as_ref();
        let addition = self.items.get(ADDITION).cloned();
        let mut result = input.clone();
        let mut list = enchantments(&result);
        let tax =
            i64::from(repair_cost(&input)) + i64::from(addition.as_ref().map_or(0, repair_cost));
        let mut price = 0;
        self.repair_item_count_cost = 0;
        if let Some(addition) = &addition {
            let using_book =
                component(addition, STORED_ENCHANTMENTS).is_some() || addition.id == ENCHANTED_BOOK;
            if is_damageable(rules, &result) && Self::repairs(rules, &input, addition) {
                // Each of the material mends a quarter, for a level each.
                let quarter = |stack: &ItemStack| {
                    damage_value(rules, stack).min(max_damage(rules, stack) / 4)
                };
                let mut repair = quarter(&result);
                if repair <= 0 {
                    self.fail();
                    return;
                }
                let mut count = 0;
                while repair > 0 && count < i32::from(addition.count) {
                    let damage = damage_value(rules, &result) - repair;
                    set_damage(rules, &mut result, damage);
                    price += 1;
                    repair = quarter(&result);
                    count += 1;
                }
                self.repair_item_count_cost = count;
            } else {
                if !using_book && (result.id != addition.id || !is_damageable(rules, &result)) {
                    self.fail();
                    return;
                }
                if is_damageable(rules, &result) && !using_book {
                    // Both items' durability left, and 12% of the maximum.
                    let max = max_damage(rules, &result);
                    let left = max_damage(rules, &input) - damage_value(rules, &input);
                    let added = max_damage(rules, addition) - damage_value(rules, addition);
                    let damage = (max - (left + added + max * 12 / 100)).max(0);
                    if damage < damage_value(rules, &result) {
                        set_damage(rules, &mut result, damage);
                        price += 2;
                    }
                }
                let (mut any_compatible, mut any_incompatible) = (false, false);
                for (id, level) in enchantments(addition) {
                    let current = level_of(&list, &id);
                    let mut level = if current == level {
                        level + 1
                    } else {
                        level.max(current)
                    };
                    let mut compatible = rules.can_enchant(&id, &input.id);
                    if cx.creative || input.id == ENCHANTED_BOOK {
                        compatible = true;
                    }
                    for (other, _) in &list {
                        if *other != id && !rules.compatible(&id, other) {
                            compatible = false;
                            price += 1;
                        }
                    }
                    if !compatible {
                        any_incompatible = true;
                        continue;
                    }
                    any_compatible = true;
                    level = level.min(rules.max_level(&id));
                    set_level(&mut list, &id, level);
                    let mut fee = rules.anvil_cost(&id);
                    if using_book {
                        fee = (fee / 2).max(1);
                    }
                    price += fee * level;
                    if input.count > 1 {
                        price = TOO_EXPENSIVE;
                    }
                }
                if any_incompatible && !any_compatible {
                    self.fail();
                    return;
                }
            }
        }
        let mut naming_cost = 0;
        match self.item_name.as_deref().filter(|name| !is_blank(name)) {
            Some(name) => {
                if custom_name(&input) != Some(name) {
                    naming_cost = 1;
                    set_component(&mut result, CUSTOM_NAME, Some(Value::from(name)));
                }
            }
            None => {
                if component(&input, CUSTOM_NAME).is_some() {
                    naming_cost = 1;
                    set_component(&mut result, CUSTOM_NAME, None);
                }
            }
        }
        price += naming_cost;
        self.cost = if price <= 0 {
            0
        } else {
            (tax + i64::from(price)).clamp(0, i64::from(i32::MAX)) as i32
        };
        let mut result = (price > 0).then_some(result);
        if naming_cost == price && naming_cost > 0 {
            // A rename alone never costs too much.
            self.cost = self.cost.min(TOO_EXPENSIVE - 1);
            self.only_renaming = true;
        }
        if self.cost >= TOO_EXPENSIVE && !cx.creative {
            result = None;
        }
        if let Some(result) = result.as_mut() {
            let mut base = repair_cost(result).max(addition.as_ref().map_or(0, repair_cost));
            if naming_cost != price || naming_cost == 0 {
                base = increased_repair_cost(base);
            }
            set_repair_cost(result, base);
            set_enchantments(result, &list);
        }
        self.items.set(RESULT, result);
    }

    /// No result, at no cost.
    fn fail(&mut self) {
        self.items.set(RESULT, None);
        self.cost = 0;
    }

    /// `AnvilMenu.onTake`: the levels paid, the addition used (as much of
    /// it as a repair took, or all of it unless only renaming) and the
    /// input; then the anvil may take damage.
    fn take_result(&mut self, cx: &mut MenuContext) {
        if !cx.creative {
            // `giveExperienceLevels(-cost)`: none below 0.
            cx.xp_levels_spent += self.cost;
            cx.xp_level = (cx.xp_level - self.cost).max(0);
        }
        if self.repair_item_count_cost > 0 {
            let used = self.repair_item_count_cost;
            let addition = self
                .items
                .get(ADDITION)
                .filter(|addition| i32::from(addition.count) > used)
                .map(|addition| ItemStack {
                    count: addition.count - used as u8,
                    ..addition.clone()
                });
            set(self, cx, ADDITION, addition);
        } else if !self.only_renaming {
            set(self, cx, ADDITION, None);
        }
        self.cost = 0;
        set(self, cx, INPUT, None);
        // `access.execute`: a survival player's use damages it at times
        // (`AnvilBlock.damage`, which the block's owner applies).
        if self.at_block {
            if !cx.creative && cx.random.next_float() < DAMAGE_CHANCE {
                cx.block_requests.push(BlockRequest::DamageAnvil);
            } else {
                cx.level_events.push(ANVIL_USED);
            }
        }
    }
}

impl Menu for AnvilMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:anvil"
    }

    fn slots(&self) -> &[SlotDef] {
        &self.slots
    }

    fn own(&self) -> &OwnSlots {
        &self.items
    }

    fn own_mut(&mut self) -> &mut OwnSlots {
        &mut self.items
    }

    fn may_place(&self, _cx: &MenuContext, slot: usize, _stack: &ItemStack) -> bool {
        slot != RESULT
    }

    /// `AnvilMenu.mayPickup`: the result for the cost in levels (creative
    /// pays none), when it costs something.
    fn may_pickup(&self, cx: &MenuContext, slot: usize) -> bool {
        slot != RESULT || ((cx.creative || cx.xp_level >= self.cost) && self.cost > 0)
    }

    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        combiner::quick_move_stack(self, cx, slot, RESULT, true)
    }

    /// `ResultContainer.removeItem`: the whole result.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot != RESULT {
            return super::remove_from(self, cx, slot, amount);
        }
        let result = self.items.get(RESULT).cloned();
        self.items.set(RESULT, None);
        result
    }

    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, _taken: &ItemStack) {
        if slot == RESULT {
            self.take_result(cx);
        } else {
            set_changed(self, cx, slot);
        }
    }

    /// The inputs' container changed: `createResult`.
    fn slots_changed(&mut self, cx: &mut MenuContext, slot: usize) {
        if slot < RESULT {
            self.create_result(cx);
        }
    }

    /// `setItemName`: a new name the result takes at once, then
    /// `createResult`. A name too long is refused.
    fn rename(&mut self, cx: &mut MenuContext, name: &str) {
        let Some(name) = validate_name(name) else {
            return;
        };
        if self.item_name.as_deref() == Some(name.as_str()) {
            return;
        }
        if let Some(mut result) = self.items.get(RESULT).cloned() {
            let value = (!is_blank(&name)).then(|| Value::from(name.as_str()));
            set_component(&mut result, CUSTOM_NAME, value);
            self.items.set(RESULT, Some(result));
        }
        self.item_name = Some(name);
        self.create_result(cx);
    }

    /// `ItemCombinerMenu.removed`: the carried stack back, then on a server
    /// the inputs (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        if self.at_block {
            clear_own_slots(self, cx, 0..RESULT);
        }
    }

    fn data(&self) -> Vec<i32> {
        vec![self.cost]
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if id == 0 {
            self.cost = value;
        }
    }

    fn own_container(&self, slot: usize) -> usize {
        usize::from(slot == RESULT)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{ContainerInput, MenuInput, handle};
    use super::*;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use serde_json::json;

    /// Sharpness (anvil cost 1, up to V) and smite (2, up to V), exclusive,
    /// for swords; mending (4, I) for anything. Iron swords last 250.
    pub(in crate::menu) struct Rules;

    impl EnchantmentRules for Rules {
        fn max_level(&self, enchantment: &str) -> i32 {
            if enchantment == "minecraft:mending" {
                1
            } else {
                5
            }
        }
        fn anvil_cost(&self, enchantment: &str) -> i32 {
            match enchantment {
                "minecraft:sharpness" => 1,
                "minecraft:smite" => 2,
                _ => 4,
            }
        }
        fn min_cost(&self, _enchantment: &str, level: i32) -> i32 {
            level * 10
        }
        fn can_enchant(&self, enchantment: &str, item: &str) -> bool {
            enchantment == "minecraft:mending" || item.ends_with("_sword")
        }
        fn compatible(&self, a: &str, b: &str) -> bool {
            let pair = [a, b];
            a != b && !(pair.contains(&"minecraft:sharpness") && pair.contains(&"minecraft:smite"))
        }
        fn is_curse(&self, enchantment: &str) -> bool {
            enchantment == "minecraft:vanishing_curse"
        }
        fn max_damage(&self, item: &str) -> i32 {
            if item == "minecraft:iron_sword" {
                250
            } else {
                0
            }
        }
        fn in_item_tag(&self, tag: &str, item: &str) -> bool {
            tag == "minecraft:iron_tool_materials" && item == "minecraft:iron_ingot"
        }
    }

    pub(in crate::menu) fn st(id: &str, count: u8, components: Value) -> ItemStack {
        let mut stack = ItemStack::new(format!("minecraft:{id}"), count);
        stack.max = if id.ends_with("sword") || id == "enchanted_book" {
            1
        } else {
            64
        };
        stack.components = (!components.is_null()).then_some(components);
        stack
    }

    fn anvil(input: ItemStack, addition: Option<ItemStack>) -> AnvilMenu {
        let mut menu = AnvilMenu::new(Vec::new());
        menu.set_rules(Some(Arc::new(Rules)));
        menu.set_at_block(true);
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        set(&mut menu, &mut cx, INPUT, Some(input));
        set(&mut menu, &mut cx, ADDITION, addition);
        menu
    }

    #[test]
    fn material_repairs_a_quarter_for_each_level_and_takes_what_it_used() {
        let sword = st(
            "iron_sword",
            1,
            json!({"minecraft:damage": 200, "minecraft:repair_cost": 1}),
        );
        let mut menu = anvil(sword, Some(st("iron_ingot", 5, Value::Null)));
        // 200 less 62 three times, then the last 14: four ingots, four
        // levels, and the prior work's 1.
        assert_eq!(menu.cost(), 5);
        let result = menu.own().get(RESULT).cloned().expect("a repaired sword");
        assert_eq!(result.components, Some(json!({"minecraft:repair_cost": 3})));
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        cx.xp_level = 5;
        let take = MenuInput::Click {
            slot: RESULT as i32,
            button: 0,
            kind: ContainerInput::Pickup,
        };
        handle(&mut menu, &mut cx, &take);
        assert_eq!(cx.inventory.cursor, Some(result));
        assert_eq!((cx.xp_levels_spent, cx.xp_level), (5, 0));
        assert_eq!(
            menu.own().get(ADDITION).map(|s| s.count),
            Some(1),
            "one ingot is left"
        );
        assert!(menu.own().get(INPUT).is_none() && menu.own().get(RESULT).is_none());
        assert_eq!(menu.cost(), 0);
        let wear = cx.block_requests.len() + cx.level_events.len();
        assert_eq!(wear, 1, "the anvil is used, or damaged");
    }

    #[test]
    fn a_book_charges_half_its_fee_and_the_prior_work_and_conflicts_cost_a_level() {
        let sword = st(
            "iron_sword",
            1,
            json!({"minecraft:enchantments": {"minecraft:smite": 2}, "minecraft:repair_cost": 3}),
        );
        let book = st(
            "enchanted_book",
            1,
            json!({"minecraft:stored_enchantments": {"minecraft:sharpness": 3, "minecraft:mending": 1}, "minecraft:repair_cost": 1}),
        );
        let menu = anvil(sword, Some(book));
        // Sharpness clashes with smite (1); mending's fee 4 halves to 2;
        // the prior work is 3 + 1.
        assert_eq!(menu.cost(), 1 + 2 + 4);
        let result = menu.own().get(RESULT).cloned().expect("a mended sword");
        assert_eq!(
            result.components,
            Some(
                json!({"minecraft:enchantments": {"minecraft:smite": 2, "minecraft:mending": 1}, "minecraft:repair_cost": 7})
            )
        );
    }

    #[test]
    fn a_rename_alone_stops_at_39_and_anything_else_from_40_is_too_expensive() {
        let worn = st("iron_sword", 1, json!({"minecraft:repair_cost": 50}));
        let mut menu = anvil(worn.clone(), None);
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        menu.rename(&mut cx, "Edge");
        assert_eq!(menu.cost(), 39);
        let result = menu.own().get(RESULT).cloned().expect("a renamed sword");
        assert_eq!(
            result.components,
            Some(json!({"minecraft:repair_cost": 50, "minecraft:custom_name": "Edge"}))
        );
        // Two of the same sword combine their durability for 2 more.
        let damaged = st(
            "iron_sword",
            1,
            json!({"minecraft:repair_cost": 50, "minecraft:damage": 100}),
        );
        let mut menu = anvil(damaged.clone(), Some(st("iron_sword", 1, Value::Null)));
        assert_eq!(
            (menu.cost(), menu.own().get(RESULT)),
            (52, None),
            "too expensive"
        );
        cx.creative = true;
        set(&mut menu, &mut cx, INPUT, Some(damaged));
        assert_eq!(menu.cost(), 52);
        assert_eq!(
            menu.own().get(RESULT).and_then(|s| s.components.clone()),
            Some(json!({"minecraft:repair_cost": 101}))
        );
        assert!(validate_name(&"x".repeat(51)).is_none());
    }
}
