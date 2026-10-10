//! `GrindstoneMenu` (26.3): two inputs that take damageable or enchanted
//! items, and the result `computeResult` makes of them: one item without
//! its enchantments but curses, or two of the same merged, their
//! durability added with 5% more. Taking the result pays experience for
//! the enchantments ground off, at the block.
//!
//! What it reads of the registries comes as the anvil's
//! [`EnchantmentRules`]. A server's menu is at its block
//! ([`GrindstoneMenu::set_at_block`]): only it pays the experience and
//! returns the inputs as it closes.
use super::anvil::{
    EnchantmentRules, component, damage_value, enchantments, increased_repair_cost, is_damageable,
    max_damage, set_component, set_damage, set_enchantments, set_level, set_repair_cost,
};
use super::{
    Menu, MenuContext, MenuPlace, OwnSlots, SlotDef, clear_own_slots, item, move_item_stack_to,
    put_back, return_carried, set, set_changed, standard_inventory_slots,
};
use crate::inventory::ItemStack;
use serde_json::Value;
use std::sync::Arc;

const TOP: usize = 0;
const BOTTOM: usize = 1;
/// The result's menu slot.
pub const RESULT: usize = 2;
/// The main inventory is 3-29, the hotbar 30-38.
const INVENTORY: usize = 3;
const HOTBAR: usize = 30;
const END: usize = 39;
/// `LevelEvent.SOUND_GRINDSTONE_USED`.
pub const GRINDSTONE_USED: i32 = 1042;

/// `EnchantmentHelper.hasAnyEnchantments`: enchantments, or stored ones.
fn has_any_enchantments(stack: &ItemStack) -> bool {
    ["minecraft:enchantments", "minecraft:stored_enchantments"]
        .iter()
        .any(|key| {
            component(stack, key)
                .and_then(Value::as_object)
                .is_some_and(|map| !map.is_empty())
        })
}

/// `GrindstoneMenu` (`grindstone`).
#[derive(Clone)]
pub struct GrindstoneMenu {
    slots: Vec<SlotDef>,
    /// The two inputs, then the result.
    items: OwnSlots,
    rules: Option<Arc<dyn EnchantmentRules>>,
    /// `access` is a block's (a server's menu).
    at_block: bool,
}

impl GrindstoneMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(TOP, 49, 19),
            SlotDef::own(BOTTOM, 49, 40),
            SlotDef::own(RESULT, 129, 34),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(3, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            rules: None,
            at_block: false,
        }
    }

    /// The registries the result comes from.
    pub fn set_rules(&mut self, rules: Option<Arc<dyn EnchantmentRules>>) {
        self.rules = rules;
    }

    /// The menu is at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// `computeResult`: nothing for no item or a stack of more than one; one
    /// enchanted item without its other enchantments; two, merged.
    fn compute_result(
        rules: &dyn EnchantmentRules,
        top: Option<&ItemStack>,
        bottom: Option<&ItemStack>,
    ) -> Option<ItemStack> {
        if [top, bottom].iter().flatten().any(|stack| stack.count > 1) {
            return None;
        }
        match (top, bottom) {
            (None, None) => None,
            (Some(one), None) | (None, Some(one)) => {
                has_any_enchantments(one).then(|| Self::remove_non_curses(rules, one.clone()))
            }
            (Some(top), Some(bottom)) => Self::merge_items(rules, top, bottom),
        }
    }

    /// `mergeItems`: the same item, with both items' durability and 5% of
    /// the larger maximum, or two of an undamageable one alike that
    /// stacks; the second's enchantments added (a curse only if new).
    fn merge_items(
        rules: &dyn EnchantmentRules,
        top: &ItemStack,
        bottom: &ItemStack,
    ) -> Option<ItemStack> {
        if top.id != bottom.id {
            return None;
        }
        let durability = max_damage(rules, top).max(max_damage(rules, bottom));
        let left = |stack: &ItemStack| max_damage(rules, stack) - damage_value(rules, stack);
        let remaining = left(top) + left(bottom) + durability * 5 / 100;
        let mut count = 1;
        if !is_damageable(rules, top) {
            // `ItemStack.matches`: the same count and components too.
            if top.max < 2 || top != bottom {
                return None;
            }
            count = 2;
        }
        let mut merged = ItemStack {
            count,
            ..top.clone()
        };
        if is_damageable(rules, &merged) {
            let default = rules.max_damage(&merged.id);
            let value = (durability != default).then(|| Value::from(durability));
            set_component(&mut merged, "minecraft:max_damage", value);
            set_damage(rules, &mut merged, (durability - remaining).max(0));
        }
        // `mergeEnchantsFrom`.
        if Self::stores_enchantments(&merged) {
            let mut list = enchantments(&merged);
            for (id, level) in enchantments(bottom) {
                let current = list
                    .iter()
                    .find(|(other, _)| *other == id)
                    .map_or(0, |(_, level)| *level);
                if (!rules.is_curse(&id) || current == 0) && level > 0 {
                    set_level(&mut list, &id, level.max(current));
                }
            }
            set_enchantments(&mut merged, &list);
        }
        Some(Self::remove_non_curses(rules, merged))
    }

    /// `EnchantmentHelper.updateEnchantments` changes only a stack whose
    /// kind of enchantments its patch does not remove.
    fn stores_enchantments(stack: &ItemStack) -> bool {
        let key = if stack.id == "minecraft:enchanted_book" {
            "minecraft:stored_enchantments"
        } else {
            "minecraft:enchantments"
        };
        let patch = stack.components.as_ref();
        !patch.is_some_and(|patch| {
            patch.get(format!("!{key}").as_str()).is_some()
                || patch.get(key).is_some_and(Value::is_null)
        })
    }

    /// `removeNonCursesFrom`: only the curses left; an enchanted book left
    /// with none is a book. Its `repair_cost` is the 2n + 1 sequence once
    /// for each enchantment left.
    fn remove_non_curses(rules: &dyn EnchantmentRules, mut stack: ItemStack) -> ItemStack {
        let mut list = enchantments(&stack);
        if Self::stores_enchantments(&stack) {
            list.retain(|(id, _)| rules.is_curse(id));
            set_enchantments(&mut stack, &list);
        }
        if stack.id == "minecraft:enchanted_book" && list.is_empty() {
            // `transmuteCopy(BOOK)`.
            stack.id = "minecraft:book".to_owned();
            stack.max = 64;
        }
        let cost = list.iter().fold(0, |cost, _| increased_repair_cost(cost));
        set_repair_cost(&mut stack, cost);
        stack
    }

    /// `getExperienceFromItem`: the minimum costs of its levels of every
    /// enchantment but the curses.
    fn experience_from(rules: &dyn EnchantmentRules, stack: Option<&ItemStack>) -> i32 {
        stack.map_or(0, |stack| {
            enchantments(stack)
                .iter()
                .filter(|(id, _)| !rules.is_curse(id))
                .map(|(id, level)| rules.min_cost(id, *level))
                .sum()
        })
    }

    /// The result slot's `onTake`: at the block, experience orbs for the
    /// enchantments ground off (half their cost, and up to as much again)
    /// and the grindstone's sound; then the inputs are gone.
    fn take_result(&mut self, cx: &mut MenuContext) {
        if let (true, Some(rules)) = (self.at_block, self.rules.clone()) {
            let amount = Self::experience_from(rules.as_ref(), self.items.get(TOP))
                + Self::experience_from(rules.as_ref(), self.items.get(BOTTOM));
            if amount > 0 {
                let half = (amount + 1) / 2;
                let xp = half + cx.random.next_int(half as u32) as i32;
                cx.xp_orbs.push((MenuPlace::Block, xp));
            }
            cx.level_events.push(GRINDSTONE_USED);
        }
        set(self, cx, TOP, None);
        set(self, cx, BOTTOM, None);
    }
}

impl Menu for GrindstoneMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:grindstone"
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

    /// The inputs take a damageable or an enchanted item.
    fn may_place(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        slot != RESULT
            && (has_any_enchantments(stack)
                || self
                    .rules
                    .as_ref()
                    .is_some_and(|rules| is_damageable(rules.as_ref(), stack)))
    }

    /// `GrindstoneMenu.quickMoveStack`: the result and the inputs into the
    /// inventory; the inventory into the inputs, or, with both full,
    /// between the main inventory and the hotbar.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let full = self.items.get(TOP).is_some() && self.items.get(BOTTOM).is_some();
        let moved = match slot {
            RESULT => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true),
            TOP | BOTTOM => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false),
            _ if !full => move_item_stack_to(self, cx, &mut stack, TOP, RESULT, false),
            INVENTORY..HOTBAR => move_item_stack_to(self, cx, &mut stack, HOTBAR, END, false),
            HOTBAR..END => move_item_stack_to(self, cx, &mut stack, INVENTORY, HOTBAR, false),
            _ => true,
        };
        if !moved {
            return None;
        }
        let left = stack.clone();
        put_back(self, cx, slot, stack);
        if left.count == clicked.count {
            return None;
        }
        self.on_take(cx, slot, &left);
        Some(clicked)
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

    /// The inputs changed: `createResult`.
    fn slots_changed(&mut self, _cx: &mut MenuContext, slot: usize) {
        if slot >= RESULT {
            return;
        }
        let result = self.rules.clone().and_then(|rules| {
            Self::compute_result(rules.as_ref(), self.items.get(TOP), self.items.get(BOTTOM))
        });
        self.items.set(RESULT, result);
    }

    /// `removed`: the carried stack back, then on a server the inputs
    /// (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        if self.at_block {
            clear_own_slots(self, cx, TOP..RESULT);
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
mod tests {
    use super::super::anvil::tests::{Rules, st};
    use super::super::{ContainerInput, MenuInput, handle};
    use super::*;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use serde_json::json;

    fn grindstone(top: Option<ItemStack>, bottom: Option<ItemStack>) -> GrindstoneMenu {
        let mut menu = GrindstoneMenu::new(Vec::new());
        menu.set_rules(Some(Arc::new(Rules)));
        menu.set_at_block(true);
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        set(&mut menu, &mut cx, TOP, top);
        set(&mut menu, &mut cx, BOTTOM, bottom);
        menu
    }

    #[test]
    fn grinding_keeps_the_curses_and_pays_for_the_rest_at_the_block() {
        let sword = st(
            "iron_sword",
            1,
            json!({"minecraft:enchantments": {"minecraft:sharpness": 3, "minecraft:vanishing_curse": 1}, "minecraft:repair_cost": 3}),
        );
        let mut menu = grindstone(Some(sword), None);
        let result = menu.own().get(RESULT).cloned().expect("a ground sword");
        assert_eq!(
            result.components,
            Some(
                json!({"minecraft:enchantments": {"minecraft:vanishing_curse": 1}, "minecraft:repair_cost": 1})
            )
        );
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        let take = MenuInput::Click {
            slot: RESULT as i32,
            button: 0,
            kind: ContainerInput::QuickMove,
        };
        handle(&mut menu, &mut cx, &take);
        assert_eq!(
            cx.inventory.slots[8],
            Some(result),
            "the last hotbar slot first"
        );
        // Sharpness III's minimum cost of 30: 15 and up to 14 more.
        let [(MenuPlace::Block, xp)] = cx.xp_orbs[..] else {
            panic!("one award: {:?}", cx.xp_orbs)
        };
        assert!((15..30).contains(&xp), "{xp}");
        assert_eq!(cx.level_events, [GRINDSTONE_USED]);
        assert!(menu.own().items().iter().all(Option::is_none));
    }

    #[test]
    fn two_items_merge_their_durability_and_a_bare_book_is_a_book() {
        let worn = |damage: i32, enchantments: Value| {
            st(
                "iron_sword",
                1,
                json!({"minecraft:damage": damage, "minecraft:enchantments": enchantments}),
            )
        };
        let top = worn(100, json!({"minecraft:sharpness": 2}));
        let bottom = worn(
            200,
            json!({"minecraft:mending": 1, "minecraft:vanishing_curse": 1}),
        );
        // 150 and 50 left, and 12 more.
        let menu = grindstone(Some(top), Some(bottom));
        assert_eq!(
            menu.own().get(RESULT).and_then(|s| s.components.clone()),
            Some(
                json!({"minecraft:damage": 38, "minecraft:enchantments": {"minecraft:vanishing_curse": 1}, "minecraft:repair_cost": 1})
            )
        );
        let book = st(
            "enchanted_book",
            1,
            json!({"minecraft:stored_enchantments": {"minecraft:mending": 1}}),
        );
        let menu = grindstone(None, Some(book.clone()));
        assert_eq!(
            menu.own().get(RESULT),
            Some(&ItemStack::new("minecraft:book", 1))
        );
        let menu = grindstone(Some(book.clone()), Some(book));
        assert_eq!(menu.own().get(RESULT), None, "books do not stack");
    }
}
