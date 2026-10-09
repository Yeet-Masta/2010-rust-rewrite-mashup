//! `MerchantMenu`, `MerchantContainer` and `MerchantResultSlot` (26.3) on
//! the menu engine: the trading screen's two payment slots and result over
//! the player's inventory. Every change to a payment slot re-reads the
//! offers (`updateSellItem`: the selected offer, or the first the payments
//! satisfy, either way round) and tells the merchant (`notifyTradeUpdated`,
//! which is when a villager says yes or no); taking the result pays for it
//! and tells the merchant of the trade (`notifyTrade`). How often the offers
//! are re-read decides which of those calls speak, so the menu re-reads
//! them where vanilla's container and slots do.
//!
//! The menu keeps a copy of the trader's offers ([`Offers`], as
//! `ClientboundMerchantOffersPacket` carries them), which its owner loads:
//! the server from the villager before each batch, a client from the
//! server's last update. What it tells the merchant waits in order
//! ([`MerchantMenu::take_events`]) for the server to tell the villager.
use crate::trading::{ItemCost, MerchantOffer};
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::menu::{self, Menu, MenuContext, OwnSlots, SlotDef};
use serde_json::Value;

const PAYMENT_A: usize = 0;
const PAYMENT_B: usize = 1;
const RESULT: usize = 2;
/// `INV_SLOT_START`, `USE_ROW_SLOT_START` and `USE_ROW_SLOT_END`: the main
/// inventory is 3-29, the hotbar 30-38.
const INVENTORY: usize = 3;
const HOTBAR: usize = 30;
const END: usize = 39;

/// What a trader shows (`ClientboundMerchantOffersPacket`): its offers,
/// level and experience, and whether the screen shows its progress and
/// that it restocks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Offers {
    pub offers: Vec<MerchantOffer>,
    /// The maximum stack sizes of the items the offers name, where not 64.
    pub max_stacks: Vec<(String, i32)>,
    pub level: i32,
    pub xp: i32,
    /// `showProgressBar`.
    pub show_progress: bool,
    /// `canRestock`.
    pub can_restock: bool,
}

impl Offers {
    /// An item's maximum stack size.
    pub fn max_stack(&self, item: &str) -> i32 {
        self.max_stacks.iter().find(|(id, _)| id == item).map_or(64, |&(_, max)| max)
    }

    fn stack(&self, id: &str, count: i32, components: Option<Value>) -> ItemStack {
        let max = self.max_stack(id).clamp(1, i32::from(u8::MAX)) as u8;
        ItemStack { id: id.to_owned(), count: count.clamp(0, i32::from(u8::MAX)) as u8, max, components }
    }

    /// `ItemCost.itemStack`: the cost's item with the components it names.
    fn cost(&self, cost: &ItemCost) -> ItemStack {
        self.stack(&cost.id, cost.count, cost.components.clone())
    }

    /// `getBaseCostA`.
    pub fn base_cost_a(&self, offer: &MerchantOffer) -> ItemStack {
        self.cost(&offer.buy)
    }

    /// `getCostA`: the first cost at its price now.
    pub fn cost_a(&self, offer: &MerchantOffer) -> ItemStack {
        ItemStack { count: self.cost_a_count(offer).clamp(0, i32::from(u8::MAX)) as u8, ..self.cost(&offer.buy) }
    }

    /// `getCostB`.
    pub fn cost_b(&self, offer: &MerchantOffer) -> Option<ItemStack> {
        offer.buy_b.as_ref().map(|cost| self.cost(cost))
    }

    /// `getResult`, and `assemble`'s copy of it.
    pub fn result(&self, offer: &MerchantOffer) -> ItemStack {
        let components = (!offer.sell.components.is_empty()).then(|| Value::Object(offer.sell.components.clone()));
        self.stack(&offer.sell.id, offer.sell.count, components)
    }

    fn cost_a_count(&self, offer: &MerchantOffer) -> i32 {
        offer.cost_a_count(self.max_stack(&offer.buy.id))
    }

    /// `satisfiedBy(a, b)`.
    fn satisfied_by(&self, offer: &MerchantOffer, a: Option<&ItemStack>, b: Option<&ItemStack>) -> bool {
        offer.satisfied_by(view(a), view(b), self.max_stack(&offer.buy.id))
    }

    /// `MerchantOffers.getRecipeFor`: the selected offer if the payments
    /// satisfy it (a hint of 0 is no hint), else the first they do.
    fn recipe_for(&self, a: Option<&ItemStack>, b: Option<&ItemStack>, hint: i32) -> Option<usize> {
        if hint > 0 && (hint as usize) < self.offers.len() {
            return self.satisfied_by(&self.offers[hint as usize], a, b).then_some(hint as usize);
        }
        self.offers.iter().position(|offer| self.satisfied_by(offer, a, b))
    }
}

type View<'a> = Option<(&'a str, i32, Option<&'a Value>)>;

fn view(stack: Option<&ItemStack>) -> View<'_> {
    stack.filter(|s| s.count > 0).map(|s| (s.id.as_str(), i32::from(s.count), s.components.as_ref()))
}

/// What the menu told its merchant, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MerchantEvent {
    /// `notifyTradeUpdated`: whether a result shows now.
    Updated(bool),
    /// `notifyTrade` for the offer at this index (which the menu's copy
    /// counts a use of already).
    Trade(usize),
}

/// A player's trading screen (`MerchantMenu` with its `MerchantContainer`).
#[derive(Clone, Debug)]
pub struct MerchantMenu {
    slots: Vec<SlotDef>,
    /// The payments and the result.
    items: OwnSlots,
    offers: Offers,
    /// `selectionHint`: the offer picked in the list.
    selection_hint: i32,
    /// `activeOffer`, by index.
    active_offer: Option<usize>,
    /// `futureXp`: what the shown result would bring the trader.
    future_xp: i32,
    /// `MerchantResultSlot.removeCount`: taken since the last statistics.
    remove_count: i32,
    events: Vec<MerchantEvent>,
}

impl MerchantMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![SlotDef::own(PAYMENT_A, 136, 37), SlotDef::own(PAYMENT_B, 162, 37), SlotDef::own(RESULT, 220, 37)];
        slots.extend(menu::standard_inventory_slots(108, 84));
        items.resize(3, None);
        Self { slots, items: OwnSlots::new(items), offers: Offers::default(), selection_hint: 0, active_offer: None, future_xp: 0, remove_count: 0, events: Vec::new() }
    }

    /// The trader's offers as the menu has them.
    pub fn offers(&self) -> &Offers {
        &self.offers
    }

    /// `overrideOffers`, or the server's villager as it now is.
    pub fn set_offers(&mut self, offers: Offers) {
        self.offers = offers;
    }

    /// A client's copy, as the server last sent it: the offers, and the
    /// active offer and future experience its slots give (the client's
    /// `setItem` of each slot from the server runs `updateSellItem`, before
    /// the server's own result arrives over it). What it would tell the
    /// merchant is no one's.
    pub fn sync(&mut self, offers: Offers) {
        self.offers = offers;
        let result = self.items.get(RESULT).cloned();
        self.update_sell_item();
        self.items.load(vec![self.items.get(PAYMENT_A).cloned(), self.items.get(PAYMENT_B).cloned(), result]);
        self.events.clear();
    }

    /// `getFutureTraderXp`.
    pub fn future_xp(&self) -> i32 {
        self.future_xp
    }

    /// What the menu told the merchant since the last call, in order.
    pub fn take_events(&mut self) -> Vec<MerchantEvent> {
        std::mem::take(&mut self.events)
    }

    fn payment(&self, slot: usize) -> Option<&ItemStack> {
        self.items.get(slot).filter(|s| s.count > 0)
    }

    /// `MerchantContainer.updateSellItem`.
    pub fn update_sell_item(&mut self) {
        self.active_offer = None;
        let (a, b) = match self.payment(PAYMENT_A) {
            None => (self.payment(PAYMENT_B).cloned(), None),
            Some(a) => (Some(a.clone()), self.payment(PAYMENT_B).cloned()),
        };
        if a.is_none() {
            self.items.set(RESULT, None);
            self.future_xp = 0;
            return;
        }
        if !self.offers.offers.is_empty() {
            let mut offer = self.offers.recipe_for(a.as_ref(), b.as_ref(), self.selection_hint);
            if offer.is_none_or(|i| self.offers.offers[i].out_of_stock()) {
                self.active_offer = offer;
                offer = self.offers.recipe_for(b.as_ref(), a.as_ref(), self.selection_hint);
            }
            match offer.filter(|&i| !self.offers.offers[i].out_of_stock()) {
                Some(i) => {
                    self.active_offer = Some(i);
                    let result = self.offers.result(&self.offers.offers[i]);
                    self.items.set(RESULT, Some(result));
                    self.future_xp = self.offers.offers[i].xp;
                }
                None => {
                    self.items.set(RESULT, None);
                    self.future_xp = 0;
                }
            }
        }
        self.events.push(MerchantEvent::Updated(self.items.get(RESULT).is_some()));
    }

    /// `MerchantContainer.setItem` on a payment slot: the stack, then the
    /// offers re-read.
    fn set_payment(&mut self, cx: &mut MenuContext, slot: usize, stack: Option<ItemStack>) {
        menu::write(self, cx, slot, stack.filter(|s| s.count > 0));
        self.update_sell_item();
    }

    /// `MerchantMenu.tryMoveItems`: the payments go back into the
    /// inventory, last slot first; if both slots are then empty, the
    /// offer's costs come from the inventory.
    fn try_move_items(&mut self, cx: &mut MenuContext, index: i32) {
        let Some(offer) = usize::try_from(index).ok().and_then(|i| self.offers.offers.get(i)).cloned() else { return };
        for slot in [PAYMENT_A, PAYMENT_B] {
            if let Some(mut old) = self.payment(slot).cloned() {
                if !menu::move_item_stack_to(self, cx, &mut old, INVENTORY, END, true) {
                    return;
                }
                self.set_payment(cx, slot, Some(old));
            }
        }
        if self.payment(PAYMENT_A).is_none() && self.payment(PAYMENT_B).is_none() {
            self.move_from_inventory_to_payment_slot(cx, PAYMENT_A, &offer.buy);
            if let Some(cost) = &offer.buy_b {
                self.move_from_inventory_to_payment_slot(cx, PAYMENT_B, cost);
            }
        }
    }

    /// `moveFromInventoryToPaymentSlot`: matching stacks, main inventory
    /// first, into the payment slot until it holds a stack.
    fn move_from_inventory_to_payment_slot(&mut self, cx: &mut MenuContext, slot: usize, cost: &ItemCost) {
        for index in INVENTORY..END {
            let Some(item) = menu::item(self, cx, index).cloned() else { continue };
            if !cost.test(&item.id, item.components.as_ref()) {
                continue;
            }
            let current = self.payment(slot).cloned();
            if current.as_ref().is_some_and(|c| !c.same_item(&item)) {
                continue;
            }
            let max = i32::from(item.max);
            let have = current.map_or(0, |c| i32::from(c.count));
            let moved = (max - have).min(i32::from(item.count));
            let paid = ItemStack { count: (have + moved).clamp(0, i32::from(u8::MAX)) as u8, ..item.clone() };
            let left = i32::from(item.count) - moved;
            menu::write(self, cx, index, (left > 0).then_some(ItemStack { count: left.clamp(0, i32::from(u8::MAX)) as u8, ..item }));
            let full = i32::from(paid.count) >= max;
            self.set_payment(cx, slot, Some(paid));
            if full {
                break;
            }
        }
    }

    /// `MerchantResultSlot.mayPickup`: the payments, either way round,
    /// satisfy the active offer.
    fn result_may_pickup(&self) -> bool {
        let Some(offer) = self.active_offer.and_then(|i| self.offers.offers.get(i)) else { return false };
        let (a, b) = (self.payment(PAYMENT_A), self.payment(PAYMENT_B));
        self.offers.satisfied_by(offer, a, b) || self.offers.satisfied_by(offer, b, a)
    }

    /// `MerchantOffer.take(a, b)`: the payments shrink by the costs, if
    /// they satisfy the offer.
    fn take(&self, offer: &MerchantOffer, a: &mut Option<ItemStack>, b: &mut Option<ItemStack>) -> bool {
        if !self.offers.satisfied_by(offer, a.as_ref(), b.as_ref()) {
            return false;
        }
        let shrink = |stack: &mut Option<ItemStack>, by: i32| {
            if let Some(s) = stack.as_mut() {
                s.count = (i32::from(s.count) - by).max(0) as u8;
            }
        };
        shrink(a, self.offers.cost_a_count(offer));
        if let Some(cost) = &offer.buy_b {
            shrink(b, cost.count);
        }
        true
    }

    /// `MerchantResultSlot.checkTakeAchievements`: what was taken counts as
    /// crafted (`onCraftedBy`).
    fn check_take_achievements(&mut self, cx: &mut MenuContext, carried: &ItemStack) {
        if self.remove_count > 0 {
            let crafted = ItemStack { count: self.remove_count.min(i32::from(u8::MAX)) as u8, ..carried.clone() };
            cx.inventory.record_crafted(&crafted);
        }
        self.remove_count = 0;
    }

    /// `MerchantResultSlot.onTake`: the payments pay for the active offer,
    /// either way round, and the merchant hears of the trade
    /// (`notifyTrade`, which uses the offer); the trader's experience is
    /// what it was and the offer's (`overrideXp`, which only a client's
    /// merchant keeps: the server's villager earned it in `notifyTrade`).
    fn take_result(&mut self, cx: &mut MenuContext, carried: &ItemStack) {
        self.check_take_achievements(cx, carried);
        let Some(index) = self.active_offer.filter(|&i| i < self.offers.offers.len()) else { return };
        let offer = self.offers.offers[index].clone();
        let (mut a, mut b) = (self.payment(PAYMENT_A).cloned(), self.payment(PAYMENT_B).cloned());
        if self.take(&offer, &mut a, &mut b) || self.take(&offer, &mut b, &mut a) {
            self.offers.offers[index].uses += 1;
            self.events.push(MerchantEvent::Trade(index));
            self.set_payment(cx, PAYMENT_A, a);
            self.set_payment(cx, PAYMENT_B, b);
        }
        self.offers.xp += offer.xp;
    }
}

impl Menu for MerchantMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:merchant"
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

    /// `MerchantResultSlot.mayPlace`: nothing.
    fn may_place(&self, _cx: &MenuContext, slot: usize, _stack: &ItemStack) -> bool {
        slot != RESULT
    }

    fn may_pickup(&self, _cx: &MenuContext, slot: usize) -> bool {
        slot != RESULT || self.result_may_pickup()
    }

    /// `MerchantResultSlot.remove` counts what it gives, and
    /// `MerchantContainer.removeItem` gives the whole result without
    /// re-reading the offers; a payment's take re-reads them.
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot != RESULT {
            return menu::remove_from(self, cx, slot, amount);
        }
        let result = self.items.get(RESULT)?.clone();
        self.remove_count += amount.min(i32::from(result.count));
        self.items.set(RESULT, None);
        Some(result)
    }

    /// `MerchantResultSlot.onTake`; a payment's is `Slot.onTake`.
    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, taken: &ItemStack) {
        if slot == RESULT {
            self.take_result(cx, taken);
        } else {
            menu::set_changed(self, cx, slot);
        }
    }

    /// `MerchantMenu.quickMoveStack`: the result into the inventory, last
    /// first, paying as a take (`onQuickCraft`; its trade sound plays only
    /// through `playLocalSound`, which a server level does not play); the
    /// payments into the inventory; the main inventory to the hotbar and
    /// back. Nothing goes to the payment slots.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = menu::item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = match slot {
            RESULT => {
                let moved = menu::move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true);
                // `Slot.onQuickCraft(picked, original)`.
                let count = i32::from(clicked.count) - i32::from(stack.count);
                if moved && count > 0 {
                    self.remove_count += count;
                    self.check_take_achievements(cx, &clicked);
                }
                moved
            }
            PAYMENT_A | PAYMENT_B => menu::move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false),
            INVENTORY..HOTBAR => menu::move_item_stack_to(self, cx, &mut stack, HOTBAR, END, false),
            HOTBAR..END => menu::move_item_stack_to(self, cx, &mut stack, INVENTORY, HOTBAR, false),
            _ => true,
        };
        if !moved {
            return None;
        }
        let left = stack.clone();
        menu::put_back(self, cx, slot, stack);
        if left.count == clicked.count {
            return None;
        }
        self.on_take(cx, slot, &left);
        Some(clicked)
    }

    /// `MerchantContainer.setItem` of a payment and `setChanged`.
    fn slots_changed(&mut self, _cx: &mut MenuContext, _slot: usize) {
        self.update_sell_item();
    }

    /// `ServerboundSelectTradePacket`: `setSelectionHint`, then
    /// `tryMoveItems`.
    fn select_trade(&mut self, cx: &mut MenuContext, index: i32) {
        self.selection_hint = index;
        self.update_sell_item();
        self.try_move_items(cx, index);
    }

    /// `MerchantMenu.removed`: the carried stack and the payments go back
    /// into the inventory (`removeItemNoUpdate`, `placeItemBackInInventory`);
    /// the trader stops trading, which is its owner's to do.
    fn removed(&mut self, cx: &mut MenuContext) {
        menu::return_carried(cx);
        menu::clear_own_slots(self, cx, PAYMENT_A..RESULT);
    }

    fn can_take_for_pick_all(&self, _carried: &ItemStack, _slot: usize) -> bool {
        false
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
    use super::*;
    use crate::trading::TradeItem;
    use minecraftoss_player::inventory::Inventory;
    use minecraftoss_player::menu::{handle, ContainerInput, MenuInput};
    use minecraftoss_player::rng::LegacyRandom;

    fn offer(buy: (&str, i32), buy_b: Option<(&str, i32)>, sell: (&str, i32), max_uses: i32) -> MerchantOffer {
        let cost = |(id, count): (&str, i32)| ItemCost { id: format!("minecraft:{id}"), count, components: None };
        MerchantOffer {
            buy: cost(buy),
            buy_b: buy_b.map(cost),
            sell: TradeItem { id: format!("minecraft:{}", sell.0), count: sell.1, components: Default::default() },
            uses: 0,
            max_uses,
            reward_exp: true,
            special_price: 0,
            demand: 0,
            price_multiplier: 0.05,
            xp: 2,
        }
    }

    fn st(id: &str, count: u8) -> ItemStack {
        ItemStack::new(format!("minecraft:{id}"), count)
    }

    fn menu(offers: Vec<MerchantOffer>) -> MerchantMenu {
        let mut menu = MerchantMenu::new(Vec::new());
        menu.set_offers(Offers { offers, level: 1, show_progress: true, can_restock: true, ..Offers::default() });
        menu
    }

    fn run(menu: &mut MerchantMenu, inventory: &mut Inventory, inputs: &[MenuInput]) {
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(inventory, &mut random);
        for input in inputs {
            handle(menu, &mut cx, input);
        }
    }

    fn click(slot: i32, kind: ContainerInput) -> MenuInput {
        MenuInput::Click { slot, button: 0, kind }
    }

    fn count(menu: &MerchantMenu, slot: usize) -> Option<u8> {
        menu.own().get(slot).map(|s| s.count)
    }

    #[test]
    fn a_picked_trade_takes_its_payment_and_shift_click_trades_it_out() {
        let mut menu = menu(vec![offer(("wheat", 20), None, ("emerald", 1), 2)]);
        let mut inventory = Inventory::default();
        inventory.slots[9] = Some(st("wheat", 50));
        run(&mut menu, &mut inventory, &[MenuInput::SelectTrade(0)]);
        assert_eq!(count(&menu, 0), Some(50), "all the wheat");
        assert!(inventory.slots[9].is_none());
        assert_eq!(menu.own().get(2).map(|s| s.id.as_str()), Some("minecraft:emerald"));
        assert_eq!(menu.future_xp(), 2);
        // Two uses: two trades, then the offer is out of stock.
        run(&mut menu, &mut inventory, &[click(2, ContainerInput::QuickMove)]);
        let trades: Vec<_> = menu.take_events().into_iter().filter(|e| matches!(e, MerchantEvent::Trade(_))).collect();
        assert_eq!(trades, [MerchantEvent::Trade(0); 2]);
        assert_eq!(menu.offers().offers[0].uses, 2);
        assert_eq!(count(&menu, 0), Some(10));
        assert!(menu.own().get(2).is_none(), "out of stock");
        assert_eq!(inventory.count("minecraft:emerald"), 2);
        // Traded results count as crafted.
        let crafted: i32 = inventory.take_stat_events().iter().filter(|(kind, ..)| kind == "minecraft:crafted").map(|e| e.2).sum();
        assert_eq!(crafted, 2);
        // Closing puts the rest back.
        run(&mut menu, &mut inventory, &[MenuInput::Close]);
        assert_eq!(inventory.count("minecraft:wheat"), 10);
        assert!(menu.own().get(0).is_none());
    }

    #[test]
    fn shift_click_never_fills_the_payment_slots() {
        let mut menu = menu(vec![offer(("wheat", 20), None, ("emerald", 1), 4)]);
        let mut inventory = Inventory::default();
        inventory.slots[9] = Some(st("wheat", 30));
        // Slot 3 is inventory 9: it goes to the hotbar, not the payments.
        run(&mut menu, &mut inventory, &[click(3, ContainerInput::QuickMove)]);
        assert!(menu.own().get(0).is_none() && menu.own().get(1).is_none());
        assert_eq!(inventory.slots[0].as_ref().map(|s| s.count), Some(30));
        // And back from the hotbar to the main inventory.
        run(&mut menu, &mut inventory, &[click(30, ContainerInput::QuickMove)]);
        assert_eq!(inventory.slots[9].as_ref().map(|s| s.count), Some(30));
    }

    #[test]
    fn a_hint_of_zero_scans_every_offer_and_swapped_payments_are_accepted() {
        // The book first and the emerald second: the second offer, either
        // way round, though the hint (0) names the first.
        let mut menu = menu_with_payments(&[Some(st("book", 1)), Some(st("emerald", 1))], 0);
        assert_eq!(menu.own().get(2).map(|s| s.id.as_str()), Some("minecraft:bookshelf"));
        // Taking the result pays with the swapped payments.
        let mut inventory = Inventory::default();
        run(&mut menu, &mut inventory, &[click(2, ContainerInput::Pickup)]);
        assert_eq!(inventory.cursor.as_ref().map(|s| s.id.as_str()), Some("minecraft:bookshelf"));
        assert!(menu.own().get(0).is_none() && menu.own().get(1).is_none(), "both paid");
        assert_eq!(menu.take_events().iter().filter(|e| **e == MerchantEvent::Trade(1)).count(), 1);
        // A hint above 0 tests only its own offer.
        let menu = menu_with_payments(&[Some(st("wheat", 20)), None], 1);
        assert!(menu.own().get(2).is_none());
        assert!(menu_with_payments(&[Some(st("wheat", 20)), None], 0).own().get(2).is_some());
    }

    /// The two offers with these payments in place, `hint` selected.
    fn menu_with_payments(payments: &[Option<ItemStack>], hint: i32) -> MerchantMenu {
        let offers = vec![offer(("wheat", 20), None, ("emerald", 1), 4), offer(("emerald", 1), Some(("book", 1)), ("bookshelf", 1), 4)];
        let mut menu = menu(offers);
        menu.selection_hint = hint;
        menu.items.load(vec![payments[0].clone(), payments[1].clone(), None]);
        menu.update_sell_item();
        menu
    }

    #[test]
    fn only_the_first_cost_takes_demand_and_discounts() {
        let mut o = offer(("emerald", 10), Some(("book", 1)), ("enchanted_book", 1), 4);
        o.demand = 4;
        o.special_price = -3;
        let offers = Offers { offers: vec![o.clone()], ..Offers::default() };
        // 10 + floor(10 * 4 * 0.05) - 3.
        assert_eq!(offers.cost_a(&o).count, 9);
        assert_eq!(offers.base_cost_a(&o).count, 10);
        assert_eq!(offers.cost_b(&o).map(|s| s.count), Some(1));
        // A negative demand never lowers the price, and it stays within a
        // stack.
        o.demand = -40;
        o.special_price = -30;
        assert_eq!(Offers { offers: vec![o.clone()], ..Offers::default() }.cost_a(&o).count, 1);
    }

    #[test]
    fn an_out_of_stock_offer_shows_no_result() {
        let mut o = offer(("wheat", 20), None, ("emerald", 1), 1);
        o.uses = 1;
        let mut menu = menu(vec![o]);
        let mut inventory = Inventory::default();
        inventory.cursor = Some(st("wheat", 20));
        run(&mut menu, &mut inventory, &[click(0, ContainerInput::Pickup)]);
        assert!(menu.own().get(2).is_none());
        assert_eq!(menu.future_xp(), 0);
        assert!(menu.take_events().iter().all(|e| *e == MerchantEvent::Updated(false)), "the villager says no");
    }

    #[test]
    fn picking_an_offer_returns_the_old_payment_first() {
        let offers = vec![offer(("wheat", 20), None, ("emerald", 1), 4), offer(("emerald", 1), Some(("book", 1)), ("bookshelf", 1), 4)];
        let mut menu = menu(offers);
        let mut inventory = Inventory::default();
        inventory.slots[9] = Some(st("wheat", 25));
        inventory.slots[10] = Some(st("emerald", 3));
        inventory.slots[0] = Some(st("book", 2));
        run(&mut menu, &mut inventory, &[MenuInput::SelectTrade(0)]);
        assert_eq!(count(&menu, 0), Some(25));
        run(&mut menu, &mut inventory, &[MenuInput::SelectTrade(1)]);
        // The wheat went back (into the hotbar's end, `moveItemStackTo`
        // backwards), and the emerald and book came in.
        assert_eq!(inventory.count("minecraft:wheat"), 25);
        assert_eq!(menu.own().get(0).map(|s| (s.id.as_str(), s.count)), Some(("minecraft:emerald", 3)));
        assert_eq!(menu.own().get(1).map(|s| (s.id.as_str(), s.count)), Some(("minecraft:book", 2)));
        assert_eq!(menu.own().get(2).map(|s| s.id.as_str()), Some("minecraft:bookshelf"));
    }

    #[test]
    fn the_client_copy_keeps_the_servers_result() {
        let mut menu = menu_with_payments(&[Some(st("wheat", 20)), None], 0);
        menu.items.load(vec![Some(st("wheat", 20)), None, None]);
        let offers = menu.offers().clone();
        menu.sync(offers);
        assert!(menu.own().get(2).is_none(), "the server's (empty) result stays");
        assert_eq!(menu.future_xp(), 2, "but the future experience is the slots'");
        assert!(menu.take_events().is_empty());
    }
}
