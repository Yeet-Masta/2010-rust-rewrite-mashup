//! `BeaconScreen` over its background: the payment items' icons; the
//! powers' buttons by tier with their mob effects' icons (greyed for a
//! tier the pyramid does not reach, framed when chosen, lit under the
//! mouse); the secondary row's regeneration and, once a primary is chosen,
//! that primary at level II; and the confirm and cancel buttons. The
//! "Primary Power" and "Secondary Power" labels stand in for the title, and
//! the hovered button's tooltip names it. The screen's choice starts from
//! the beacon's data values, and starts over when they change
//! (`dataChanged`).
use minecraft_terrain::pack::PackStack;
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::menu::beacon::{
    BEACON_EFFECTS, LEVELS, PAYMENT, PRIMARY, SECONDARY, decode_effect,
};

use super::MenuView;
use crate::font::rgb;
use crate::gui::Gui;
use crate::render::UiList;

/// The labels' colour (`-2039584`, with a shadow).
const LABEL: u32 = 0xE0E0E0;
/// `BeaconScreenButton`'s size.
const BUTTON: f32 = 22.0;
/// `extractBackground`'s payment items, across at y 109.
const PAYMENTS: [(&str, f32); 5] = [
    ("minecraft:netherite_ingot", 20.0),
    ("minecraft:emerald", 41.0),
    ("minecraft:diamond", 63.0),
    ("minecraft:gold_ingot", 86.0),
    ("minecraft:iron_ingot", 108.0),
];

/// A button of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeaconButton {
    /// `BeaconPowerButton`: a power of its tier, as the primary or the
    /// secondary.
    Power {
        effect: &'static str,
        primary: bool,
        tier: i32,
    },
    /// `BeaconUpgradePowerButton`: the primary again, as the secondary.
    Upgrade(&'static str),
    /// `BeaconConfirmButton`.
    Confirm,
    /// `BeaconCancelButton`.
    Cancel,
}

/// The screen's `primary` and `secondary`, and the data values they were
/// last taken from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BeaconChoice {
    pub primary: Option<&'static str>,
    pub secondary: Option<&'static str>,
    seen: Vec<i32>,
}

impl BeaconChoice {
    /// The menu listener's `dataChanged`: other data values set both
    /// powers from them.
    pub fn watch(&mut self, data: &[i32]) {
        if self.seen == data {
            return;
        }
        self.seen = data.to_vec();
        let value = |id: usize| data.get(id).copied().unwrap_or(0);
        self.primary = decode_effect(value(PRIMARY));
        self.secondary = decode_effect(value(SECONDARY));
    }

    /// `init`'s buttons, from the window's corner: each primary tier's
    /// centred on x 76, 25 pixels a tier; the secondary row centred on
    /// 167, its upgrade only with a primary (`visible`); confirm, cancel.
    pub fn buttons(&self) -> Vec<(f32, f32, BeaconButton)> {
        let width = |count: i32| count * 22 + (count - 1) * 2;
        let mut buttons = Vec::new();
        for (tier, effects) in BEACON_EFFECTS.iter().enumerate().take(3) {
            let left = 76 - width(effects.len() as i32) / 2;
            for (c, &effect) in effects.iter().enumerate() {
                let power = BeaconButton::Power {
                    effect,
                    primary: true,
                    tier: tier as i32,
                };
                buttons.push((
                    (left + c as i32 * 24) as f32,
                    22.0 + 25.0 * tier as f32,
                    power,
                ));
            }
        }
        let left = 167 - width(BEACON_EFFECTS[3].len() as i32 + 1) / 2;
        for (c, &effect) in BEACON_EFFECTS[3].iter().enumerate() {
            let power = BeaconButton::Power {
                effect,
                primary: false,
                tier: 3,
            };
            buttons.push(((left + c as i32 * 24) as f32, 47.0, power));
        }
        if let Some(primary) = self.primary {
            let x = left + BEACON_EFFECTS[3].len() as i32 * 24;
            buttons.push((x as f32, 47.0, BeaconButton::Upgrade(primary)));
        }
        buttons.push((164.0, 107.0, BeaconButton::Confirm));
        buttons.push((190.0, 107.0, BeaconButton::Cancel));
        buttons
    }

    /// `updateStatus`: whether a button is active, and whether selected.
    pub fn status(&self, button: BeaconButton, levels: i32, has_payment: bool) -> (bool, bool) {
        match button {
            BeaconButton::Power {
                effect,
                primary,
                tier,
            } => {
                let chosen = if primary {
                    self.primary
                } else {
                    self.secondary
                };
                (tier < levels, chosen == Some(effect))
            }
            BeaconButton::Upgrade(effect) => (3 < levels, self.secondary == Some(effect)),
            BeaconButton::Confirm => (has_payment && self.primary.is_some(), false),
            BeaconButton::Cancel => (true, false),
        }
    }

    /// The button under (x, y) from the window's corner, and where it is.
    fn at(&self, x: f32, y: f32) -> Option<(f32, f32, BeaconButton)> {
        self.buttons()
            .into_iter()
            .find(|&(bx, by, _)| x >= bx && y >= by && x < bx + BUTTON && y < by + BUTTON)
    }

    /// `mouseClicked` with the left button at (x, y): an active button
    /// takes it. A power not chosen yet is chosen (`onPress`; a primary
    /// drops a secondary that is not the same); confirm and cancel are for
    /// the screen to do. The button that took the press.
    pub fn press(
        &mut self,
        x: f32,
        y: f32,
        levels: i32,
        has_payment: bool,
    ) -> Option<BeaconButton> {
        let (_, _, button) = self.at(x, y)?;
        let (active, selected) = self.status(button, levels, has_payment);
        if !active {
            return None;
        }
        match button {
            BeaconButton::Power {
                effect,
                primary: true,
                ..
            } if !selected => {
                self.primary = Some(effect);
                if self.secondary != Some(effect) {
                    self.secondary = None;
                }
            }
            BeaconButton::Power {
                effect,
                primary: false,
                ..
            }
            | BeaconButton::Upgrade(effect)
                if !selected =>
            {
                self.secondary = Some(effect);
            }
            _ => {}
        }
        Some(button)
    }
}

/// `Hud.getMobEffectSprite` for a beacon's power.
fn effect_icon(effect: &str) -> &'static str {
    match effect {
        "minecraft:speed" => "mob_effect_speed",
        "minecraft:haste" => "mob_effect_haste",
        "minecraft:resistance" => "mob_effect_resistance",
        "minecraft:jump_boost" => "mob_effect_jump_boost",
        "minecraft:strength" => "mob_effect_strength",
        _ => "mob_effect_regeneration",
    }
}

/// The beacon's levels and whether its payment slot holds anything, as the
/// screen shows them.
fn levels_and_payment(view: &MenuView<'_>) -> (i32, bool) {
    let levels = view.data.get(LEVELS).copied().unwrap_or(0);
    let paid = view
        .slots
        .get(PAYMENT)
        .is_some_and(|slot| slot.stack.is_some());
    (levels, paid)
}

impl Gui {
    /// `BeaconScreen.extractBackground`'s payment items, the buttons
    /// (`BeaconScreenButton.extractContents`) and `extractLabels`.
    pub(super) fn beacon_extras(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        for (id, x) in PAYMENTS {
            self.item(
                ui,
                packs,
                view.inventory,
                &ItemStack::new(id, 1),
                left + x,
                top + 109.0,
            );
        }
        let (levels, paid) = levels_and_payment(view);
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        for (x, y, button) in view.beacon.buttons() {
            let (active, selected) = view.beacon.status(button, levels, paid);
            let hovered = mx >= x && my >= y && mx < x + BUTTON && my < y + BUTTON;
            let frame = if !active {
                "beacon_button_disabled"
            } else if selected {
                "beacon_button_selected"
            } else if hovered {
                "beacon_button_highlighted"
            } else {
                "beacon_button"
            };
            self.sprite(ui, frame, left + x, top + y, BUTTON, BUTTON);
            let icon = match button {
                BeaconButton::Power { effect, .. } | BeaconButton::Upgrade(effect) => {
                    effect_icon(effect)
                }
                BeaconButton::Confirm => "beacon_confirm",
                BeaconButton::Cancel => "beacon_cancel",
            };
            self.sprite(ui, icon, left + x + 2.0, top + y + 2.0, 18.0, 18.0);
        }
        // `centeredText` with a shadow.
        for (key, x) in [
            ("block.minecraft.beacon.primary", 62.0),
            ("block.minecraft.beacon.secondary", 169.0),
        ] {
            let text = crate::creative::translate(self.language(), key, &[]);
            let x = x - (self.font.width(&text) / 2.0).floor();
            self.text(ui, &text, left + x, top + 10.0, rgb(LABEL), true);
        }
    }

    /// The hovered button's tooltip: its power's name (the upgrade's with
    /// " II"), "Done" or "Cancel".
    pub(super) fn beacon_tooltip(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let Some((_, _, button)) = view.beacon.at(mx, my) else {
            return;
        };
        let name = |effect: &str| {
            let key = format!("effect.{}", effect.replace(':', "."));
            crate::creative::translate(self.language(), &key, &[])
        };
        let text = match button {
            BeaconButton::Power { effect, .. } => name(effect),
            BeaconButton::Upgrade(effect) => format!("{} II", name(effect)),
            BeaconButton::Confirm => crate::creative::translate(self.language(), "gui.done", &[]),
            BeaconButton::Cancel => crate::creative::translate(self.language(), "gui.cancel", &[]),
        };
        self.tooltip(ui, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_follow_the_tiers_and_choose_the_powers() {
        let mut choice = BeaconChoice::default();
        // Speed (1) and none from the data values.
        choice.watch(&[4, 1, 0]);
        assert_eq!(
            (choice.primary, choice.secondary),
            (Some("minecraft:speed"), None)
        );
        let at = |choice: &BeaconChoice, wanted: BeaconButton| {
            choice
                .buttons()
                .into_iter()
                .find(|&(_, _, b)| b == wanted)
                .map(|(x, y, _)| (x, y))
        };
        let power = |effect, primary, tier| BeaconButton::Power {
            effect,
            primary,
            tier,
        };
        assert_eq!(
            at(&choice, power("minecraft:haste", true, 0)),
            Some((77.0, 22.0))
        );
        assert_eq!(
            at(&choice, power("minecraft:strength", true, 2)),
            Some((65.0, 72.0))
        );
        assert_eq!(
            at(&choice, power("minecraft:regeneration", false, 3)),
            Some((144.0, 47.0))
        );
        assert_eq!(
            at(&choice, BeaconButton::Upgrade("minecraft:speed")),
            Some((168.0, 47.0))
        );
        // Speed II, then haste, which drops it; strength is out of reach
        // at two levels, as is the secondary row.
        assert!(choice.press(170.0, 50.0, 4, true).is_some());
        assert_eq!(choice.secondary, Some("minecraft:speed"));
        choice.press(80.0, 25.0, 4, true);
        assert_eq!(
            (choice.primary, choice.secondary),
            (Some("minecraft:haste"), None)
        );
        assert_eq!(choice.press(66.0, 73.0, 2, true), None);
        assert_eq!(choice.press(145.0, 48.0, 2, true), None);
        assert_eq!(
            choice.status(BeaconButton::Confirm, 2, false),
            (false, false)
        );
    }
}
