//! `MerchantScreen` over its background: seven offer buttons with the
//! offers in view drawn over them (the costs, the first struck through at
//! its old price when it is discounted, the arrow, the result), the
//! scroller, the out-of-stock mark of the offer picked last, the trader's
//! experience bar with what the shown result would add, the "Trades" label
//! and the tooltips. [`TradeList`] is the screen's own state: the scroll,
//! the offer picked, and the mouse made into them.
use minecraft_terrain::menus::merchant;
use minecraft_terrain::pack::PackStack;
use minecraftoss_entities::merchant::Offers;
use serde_json::{Value, json};

use super::{LABEL, MenuView};
use crate::font::rgb;
use crate::gui::{Gui, WHITE};
use crate::render::UiList;

/// `NUMBER_OF_OFFER_BUTTONS`.
const BUTTONS: usize = 7;
/// `TRADE_BUTTON_X`, the first button's y (`16 + 2`), and the buttons'
/// `TRADE_BUTTON_WIDTH` and `TRADE_BUTTON_HEIGHT`.
const BUTTON_X: f32 = 5.0;
const BUTTON_Y: f32 = 18.0;
const BUTTON_W: f32 = 88.0;
const BUTTON_H: f32 = 20.0;
/// `SCROLL_BAR_START_X`, `SCROLL_BAR_TOP_POS_Y`, `SCROLL_BAR_HEIGHT` and
/// `SCROLLER_HEIGHT`.
const SCROLLER_X: f32 = 94.0;
const SCROLL_TOP: f32 = 18.0;
const SCROLL_BAR: i32 = 139;
const SCROLLER: i32 = 27;
/// The scroller's lowest offset (`139 - 27 + 1`).
const SCROLLER_LOWEST: i32 = 113;
/// `PROGRESS_BAR_X`, `PROGRESS_BAR_Y`, and the bar's width.
const BAR_X: f32 = 136.0;
const BAR_Y: f32 = 16.0;
const BAR_W: i32 = 102;
/// `VillagerData.NEXT_LEVEL_XP_THRESHOLDS`.
const LEVEL_XP: [i32; 5] = [0, 10, 70, 150, 250];

/// `canScroll`: more offers than buttons.
fn can_scroll(offers: usize) -> bool {
    offers > BUTTONS
}

/// The offers in view, row by row: all of them when they fit the buttons,
/// else the seven from `scroll_off`.
pub fn rows(offers: usize, scroll_off: usize) -> std::ops::Range<usize> {
    if can_scroll(offers) {
        scroll_off..(scroll_off + BUTTONS).min(offers)
    } else {
        0..offers
    }
}

/// The shown button at (x, y) in the window: one of the first seven, and
/// only as many as there are offers (`TradeOfferButton.visible`).
pub fn button_at(x: f32, y: f32, offers: usize) -> Option<usize> {
    if !(BUTTON_X..BUTTON_X + BUTTON_W).contains(&x) || y < BUTTON_Y {
        return None;
    }
    let index = ((y - BUTTON_Y) / BUTTON_H) as usize;
    (index < BUTTONS && index < offers).then_some(index)
}

/// `extractScroller`: the scroller's offset down the bar, or none when it
/// is disabled (seven offers or fewer).
pub fn scroller_offset(offers: usize, scroll_off: usize) -> Option<i32> {
    let steps = offers as i32 + 1 - BUTTONS as i32;
    if steps <= 1 {
        return None;
    }
    let left_over = SCROLL_BAR - (SCROLLER + (steps - 1) * SCROLL_BAR / steps);
    let step = 1 + left_over / steps + SCROLL_BAR / steps;
    let scroll_off = scroll_off as i32;
    if scroll_off == steps - 1 {
        return Some(SCROLLER_LOWEST);
    }
    Some(SCROLLER_LOWEST.min(scroll_off * step))
}

/// A part of an offer's button, for its tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferPart {
    CostA,
    CostB,
    Result,
}

/// `TradeOfferButton.extractToolTip`: what is under the mouse `dx` across
/// a button.
pub fn offer_part(dx: f32) -> Option<OfferPart> {
    if dx < 20.0 {
        Some(OfferPart::CostA)
    } else if dx < 50.0 && dx > 30.0 {
        Some(OfferPart::CostB)
    } else if dx > 65.0 {
        Some(OfferPart::Result)
    } else {
        None
    }
}

/// `extractProgressBar`: none at the top level; otherwise the widths of the
/// trader's experience into its level and of what the shown result would
/// add (both 0 where the bar is empty).
pub fn experience_bar(level: i32, xp: i32, future_xp: i32) -> Option<(i32, i32)> {
    if level >= 5 {
        return None;
    }
    // `VillagerData.canLevelUp`, `getMinXpPerLevel`, `getMaxXpPerLevel`.
    if !(1..5).contains(&level) || xp < LEVEL_XP[level as usize - 1] {
        return Some((0, 0));
    }
    let (min, max) = (LEVEL_XP[level as usize - 1], LEVEL_XP[level as usize]);
    let multiplier = BAR_W as f32 / (max - min) as f32;
    let current = ((multiplier * (xp - min) as f32).floor() as i32).min(BAR_W);
    let future = if future_xp > 0 {
        ((future_xp as f32 * multiplier).floor() as i32).min(BAR_W - current)
    } else {
        0
    };
    Some((current, future))
}

/// The trading screen's own state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeList {
    /// `scrollOff`: the first offer in view when they do not all fit.
    pub scroll_off: usize,
    /// `shopItem`: the offer picked last.
    pub shop_item: usize,
    /// `isDragging`: the scroller is held.
    pub dragging: bool,
    /// The button the last click focused, which stays highlighted.
    pub focused: Option<usize>,
}

impl TradeList {
    /// `extractLabels`' title: the trader's name, with its level's
    /// (`merchant.title`) when it shows its progress at a level of 1 to 5.
    pub fn title(name: &Value, offers: &Offers) -> Value {
        if (1..=5).contains(&offers.level) && offers.show_progress {
            let level = json!({ "translate": format!("merchant.level.{}", offers.level) });
            json!({ "translate": "merchant.title", "with": [name, level] })
        } else {
            name.clone()
        }
    }

    /// `mouseClicked` at (x, y) in the window, before the slots see it: a
    /// press on the bar starts holding the scroller, and a shown button
    /// takes the press, the primary button picking its offer
    /// (`postButtonClick`). None when the press is the slots'; else the
    /// offer picked, if one was.
    pub fn press(&mut self, x: f32, y: f32, primary: bool, offers: usize) -> Option<Option<i32>> {
        let bar = SCROLLER_X..=SCROLLER_X + 6.0;
        if can_scroll(offers)
            && x > *bar.start()
            && x < *bar.end()
            && y > SCROLL_TOP
            && y <= SCROLL_TOP + SCROLL_BAR as f32 + 1.0
        {
            self.dragging = true;
        }
        let button = button_at(x, y, offers)?;
        if !primary {
            return Some(None);
        }
        self.focused = Some(button);
        self.shop_item = button + self.scroll_off;
        Some(Some(self.shop_item as i32))
    }

    /// `mouseDragged` while the scroller is held: the scroll the mouse's
    /// height `y` in the window gives.
    pub fn drag(&mut self, y: f32, offers: usize) {
        if !self.dragging {
            return;
        }
        let max = offers.saturating_sub(BUTTONS) as i32;
        let travel = (SCROLL_BAR - SCROLLER) as f32;
        let scrolling = (y - SCROLL_TOP - 13.5) / travel * max as f32 + 0.5;
        self.scroll_off = (scrolling as i32).clamp(0, max) as usize;
    }

    /// `mouseReleased`: the scroller is let go.
    pub fn release(&mut self) {
        self.dragging = false;
    }

    /// `mouseScrolled`: `notches` up (positive) or down.
    pub fn wheel(&mut self, notches: i32, offers: usize) {
        if can_scroll(offers) {
            let max = (offers - BUTTONS) as i32;
            self.scroll_off = (self.scroll_off as i32 - notches).clamp(0, max) as usize;
        }
    }
}

impl Gui {
    /// The mouse's whole pixel in the window, as the widgets see it.
    fn trade_mouse(&self, left: f32, top: f32) -> (f32, f32) {
        (self.mouse.0.floor() - left, self.mouse.1.floor() - top)
    }

    /// `MerchantScreen.extractBackground`'s mark, its "Trades" label, its
    /// buttons, and `extractContents`' offers, scroller and progress.
    pub(super) fn merchant_extras(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let Some(menu) = merchant(view.menu) else {
            return;
        };
        let offers = menu.offers();
        let count = offers.offers.len();
        let list = view.trades;
        if offers
            .offers
            .get(list.shop_item)
            .is_some_and(|o| o.out_of_stock())
        {
            self.sprite(
                ui,
                "villager_out_of_stock",
                left + 182.0,
                top + 35.0,
                28.0,
                21.0,
            );
        }
        let label = crate::creative::translate(&self.language, "merchant.trades", &[]);
        let x = 53.0 - (self.font.width(&label) / 2.0).floor();
        self.text(ui, &label, left + x, top + 6.0, rgb(LABEL), false);
        let (mx, my) = self.trade_mouse(left, top);
        let hovered = button_at(mx, my, count);
        for index in 0..BUTTONS.min(count) {
            let lit = hovered == Some(index) || list.focused == Some(index);
            let sprite = if lit { "button_highlighted" } else { "button" };
            let y = top + BUTTON_Y + BUTTON_H * index as f32;
            self.sprite(ui, sprite, left + BUTTON_X, y, BUTTON_W, BUTTON_H);
        }
        if count == 0 {
            return;
        }
        match scroller_offset(count, list.scroll_off) {
            Some(offset) => {
                let y = top + SCROLL_TOP + offset as f32;
                self.sprite(ui, "villager_scroller", left + SCROLLER_X, y, 6.0, 27.0);
            }
            None => {
                let y = top + SCROLL_TOP;
                self.sprite(
                    ui,
                    "villager_scroller_disabled",
                    left + SCROLLER_X,
                    y,
                    6.0,
                    27.0,
                );
            }
        }
        for (row, index) in rows(count, list.scroll_off).enumerate() {
            let offer = &offers.offers[index];
            let y = top + 19.0 + 20.0 * row as f32;
            // `extractAndDecorateCostA`.
            let (base, cost) = (offers.base_cost_a(offer), offers.cost_a(offer));
            let x = left + 10.0;
            if base.count == cost.count {
                self.item(ui, packs, view.inventory, &cost, x, y);
            } else {
                let was = base.count.to_string();
                self.item_counted(ui, packs, view.inventory, &cost, x, y, Some((&was, WHITE)));
                self.item_count(ui, &cost.count.to_string(), x + 14.0, y, WHITE);
                let strike = "villager_discount_strikethrough";
                self.sprite(ui, strike, x + 7.0, y + 12.0, 9.0, 2.0);
            }
            if let Some(cost_b) = offers.cost_b(offer) {
                self.item(ui, packs, view.inventory, &cost_b, left + 40.0, y);
            }
            let arrow = if offer.out_of_stock() {
                "villager_trade_arrow_out_of_stock"
            } else {
                "villager_trade_arrow"
            };
            self.sprite(ui, arrow, left + 60.0, y + 3.0, 10.0, 9.0);
            self.item(
                ui,
                packs,
                view.inventory,
                &offers.result(offer),
                left + 73.0,
                y,
            );
        }
        if !offers.show_progress {
            return;
        }
        let Some((current, future)) = experience_bar(offers.level, offers.xp, menu.future_xp())
        else {
            return;
        };
        let (x, y) = (left + BAR_X, top + BAR_Y);
        let background = "villager_experience_bar_background";
        self.sprite(ui, background, x, y, BAR_W as f32, 5.0);
        let (current, future) = (current as f32, future as f32);
        if current > 0.0 {
            let sprite = "villager_experience_bar_current";
            self.sprite_part(ui, sprite, x, y, 0.0, 0.0, current, 5.0);
        }
        if future > 0.0 {
            let sprite = "villager_experience_bar_result";
            self.sprite_part(ui, sprite, x + current, y, current, 0.0, future, 5.0);
        }
    }

    /// `extractContents`' tooltips: the out-of-stock mark's, for a trader
    /// that restocks, and the hovered button's cost or result.
    pub(super) fn merchant_tooltips(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let Some(menu) = merchant(view.menu) else {
            return;
        };
        let offers = menu.offers();
        let list = view.trades;
        let (mx, my) = self.trade_mouse(left, top);
        // `isHovering(186, 35, 22, 21)`, a pixel wider all round.
        let over_mark = (185.0..209.0).contains(&mx) && (34.0..57.0).contains(&my);
        let out = offers
            .offers
            .get(list.shop_item)
            .is_some_and(|o| o.out_of_stock());
        if out && over_mark && offers.can_restock {
            let text = crate::creative::translate(&self.language, "merchant.deprecated", &[]);
            self.tooltip(ui, &text);
        }
        let Some(button) = button_at(mx, my, offers.offers.len()) else {
            return;
        };
        let Some(offer) = offers.offers.get(button + list.scroll_off) else {
            return;
        };
        let stack = match offer_part(mx - BUTTON_X) {
            Some(OfferPart::CostA) => Some(offers.cost_a(offer)),
            Some(OfferPart::CostB) => offers.cost_b(offer),
            Some(OfferPart::Result) => Some(offers.result(offer)),
            None => None,
        };
        if let Some(stack) = stack {
            self.stack_tooltip(ui, &stack, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_to_seven_offers_all_show_and_more_scroll() {
        assert_eq!(rows(3, 0), 0..3);
        assert_eq!(rows(7, 2), 0..7, "seven fit: the scroll is ignored");
        assert_eq!(rows(10, 0), 0..7);
        assert_eq!(rows(10, 3), 3..10);
        // The buttons: 88 by 20 down from (5, 18), as many as offers.
        assert_eq!(button_at(5.0, 18.0, 10), Some(0));
        assert_eq!(button_at(92.9, 37.9, 10), Some(0));
        assert_eq!(button_at(50.0, 38.0, 10), Some(1));
        assert_eq!(button_at(50.0, 157.9, 10), Some(6));
        assert_eq!(button_at(50.0, 158.0, 10), None, "no eighth");
        assert_eq!(button_at(93.0, 20.0, 10), None);
        assert_eq!(button_at(50.0, 60.0, 2), None, "a third button is hidden");
    }

    #[test]
    fn the_scroller_steps_down_its_bar() {
        assert_eq!(scroller_offset(7, 0), None, "disabled");
        // Eight offers, two steps: 139 - (27 + 69) = 43 left over, steps of
        // 1 + 21 + 69 = 91, the last forced to the bottom.
        assert_eq!(scroller_offset(8, 0), Some(0));
        assert_eq!(scroller_offset(8, 1), Some(113));
        // Twelve: six steps, 139 - (27 + 115) = -3 left over, steps of
        // 1 + 0 + 23.
        assert_eq!(scroller_offset(12, 1), Some(24));
        assert_eq!(scroller_offset(12, 4), Some(96));
        assert_eq!(scroller_offset(12, 5), Some(113));
    }

    #[test]
    fn the_wheel_and_the_scroller_move_the_list() {
        let mut list = TradeList::default();
        list.wheel(-1, 5);
        assert_eq!(list.scroll_off, 0, "seven or fewer do not scroll");
        list.wheel(-2, 10);
        assert_eq!(list.scroll_off, 2);
        list.wheel(-5, 10);
        assert_eq!(list.scroll_off, 3, "at most offers - 7");
        list.wheel(9, 10);
        assert_eq!(list.scroll_off, 0);
        // A press on the bar holds the scroller; the mouse's height sets
        // the scroll; letting go ends it.
        assert_eq!(list.press(97.0, 20.0, true, 10), None, "not a button");
        assert!(list.dragging);
        list.drag(18.0 + 13.5 + 112.0, 10);
        assert_eq!(list.scroll_off, 3);
        list.drag(18.0 + 13.5 + 56.0, 10);
        assert_eq!(list.scroll_off, 2, "(0.5 * 3 + 0.5) truncated");
        list.drag(0.0, 10);
        assert_eq!(list.scroll_off, 0);
        list.release();
        list.drag(150.0, 10);
        assert_eq!(list.scroll_off, 0, "let go");
        // Without enough offers the bar is not held.
        assert_eq!(list.press(97.0, 20.0, true, 7), None);
        assert!(!list.dragging);
    }

    #[test]
    fn a_button_picks_the_offer_in_its_row() {
        let mut list = TradeList {
            scroll_off: 2,
            ..TradeList::default()
        };
        assert_eq!(list.press(20.0, 40.0, true, 10), Some(Some(3)));
        assert_eq!((list.shop_item, list.focused), (3, Some(1)));
        assert_eq!(
            list.press(20.0, 60.0, false, 10),
            Some(None),
            "taken, not picked"
        );
        assert_eq!(list.shop_item, 3);
        assert_eq!(list.press(120.0, 40.0, true, 10), None, "the slots'");
    }

    #[test]
    fn tooltips_follow_the_parts_of_a_button() {
        assert_eq!(offer_part(0.0), Some(OfferPart::CostA));
        assert_eq!(offer_part(19.0), Some(OfferPart::CostA));
        assert_eq!(offer_part(25.0), None);
        assert_eq!(offer_part(31.0), Some(OfferPart::CostB));
        assert_eq!(offer_part(60.0), None);
        assert_eq!(offer_part(66.0), Some(OfferPart::Result));
    }

    #[test]
    fn the_experience_bar_shows_the_level_and_the_trade() {
        // Novice (0 to 10) at 5, a trade worth 2: half, and a fifth more.
        assert_eq!(experience_bar(1, 5, 2), Some((51, 20)));
        // Apprentice (10 to 70) at 70: full, nothing to add.
        assert_eq!(experience_bar(2, 70, 5), Some((102, 0)));
        assert_eq!(
            experience_bar(3, 10, 0),
            Some((0, 0)),
            "below its level's start"
        );
        assert_eq!(experience_bar(5, 300, 1), None, "a master has no bar");
        let offers = Offers {
            level: 2,
            show_progress: true,
            ..Offers::default()
        };
        let name = json!({ "translate": "entity.minecraft.villager.farmer" });
        assert_eq!(
            TradeList::title(&name, &offers),
            json!({ "translate": "merchant.title", "with": [name, { "translate": "merchant.level.2" }] })
        );
        let wandering = Offers {
            level: 2,
            ..Offers::default()
        };
        assert_eq!(TradeList::title(&name, &wandering), name);
    }
}
