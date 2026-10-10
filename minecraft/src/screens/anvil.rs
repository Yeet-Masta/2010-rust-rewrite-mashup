//! `AnvilScreen` over its background: the name box (an `EditBox` without
//! a border, on its field sprite, greyed while the input slot is empty),
//! the error sprite while the inputs make nothing (`extractErrorIcon`),
//! and the cost in levels, green, or red when it can't be paid or is too
//! much (`extractLabels`).
use std::time::Instant;

use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::menu::anvil::{self, MAX_NAME_LENGTH, RESULT, TOO_EXPENSIVE};

use super::MenuView;
use crate::creative::translate;
use crate::font::rgb;
use crate::gui::{Gui, WHITE};
use crate::render::UiList;

/// The cost's colours (`-8323296` and `-40864`) and its backing
/// (`1325400064`).
const COST: u32 = 0x80FF20;
const COST_RED: u32 = 0xFF6060;
const BACKING: [f32; 4] = [0.0, 0.0, 0.0, 79.0 / 255.0];

/// The name box: its text, the input it was last reset for
/// (`slotChanged`), and when it last took focus, for the cursor's blink.
#[derive(Clone, Debug)]
pub struct NameBox {
    pub text: String,
    input: Option<ItemStack>,
    focused: Instant,
}

impl Default for NameBox {
    fn default() -> Self {
        Self {
            text: String::new(),
            input: None,
            focused: Instant::now(),
        }
    }
}

impl NameBox {
    /// `slotChanged` for the input slot: when it holds another stack, the
    /// text becomes its name (`setValue`, cut to 50), or empty. Whether it
    /// did.
    pub fn input_changed(
        &mut self,
        input: Option<&ItemStack>,
        name: impl Fn(&ItemStack) -> String,
    ) -> bool {
        if self.input.as_ref() == input {
            return false;
        }
        self.input = input.cloned();
        self.text = input.map_or_else(String::new, |stack| {
            name(stack).chars().take(MAX_NAME_LENGTH).collect()
        });
        self.focused = Instant::now();
        true
    }

    /// `EditBox.charTyped` and `keyPressed` for a key: a character chat
    /// takes, while there is room, or backspace (a word, with control).
    /// Whether the text changed.
    pub fn key(&mut self, typed: Option<char>, backspace: bool, ctrl: bool) -> bool {
        let before = self.text.clone();
        match typed {
            Some(c) => {
                if c != '§'
                    && c >= ' '
                    && c != '\u{7f}'
                    && self.text.chars().count() < MAX_NAME_LENGTH
                {
                    self.text.push(c);
                }
            }
            None if backspace && ctrl => {
                // `deleteWords(-1)`: back over spaces, then the word.
                let text = self.text.trim_end_matches(' ');
                let keep = text.rfind(' ').map_or(0, |at| at + 1);
                self.text.truncate(keep);
            }
            None if backspace => {
                self.text.pop();
            }
            None => {}
        }
        self.text != before
    }

    /// The blinking cursor shows (`(millis - focusedTime) / 300 % 2 == 0`).
    pub fn cursor(&self) -> bool {
        (self.focused.elapsed().as_millis() / 300).is_multiple_of(2)
    }
}

impl Gui {
    /// `AnvilScreen.extractBackground` and `extractErrorIcon`, the box, and
    /// `extractLabels`' cost.
    pub(super) fn anvil_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let input = view.slots.first().is_some_and(|slot| slot.stack.is_some());
        let addition = view.slots.get(1).is_some_and(|slot| slot.stack.is_some());
        let result = view
            .slots
            .get(RESULT)
            .is_some_and(|slot| slot.stack.is_some());
        let field = if input {
            "anvil_text_field"
        } else {
            "anvil_text_field_disabled"
        };
        self.sprite(ui, field, left + 59.0, top + 20.0, 110.0, 16.0);
        if (input || addition) && !result {
            self.sprite(ui, "anvil_error", left + 99.0, top + 45.0, 28.0, 21.0);
        }
        // The borderless box at (62, 24), 103 wide: as much of the text's
        // end as fits, and the cursor after it while it can be edited.
        if let Some((text, cursor)) = view.name {
            let mut shown = text;
            while self.font.width(shown) > 103.0 {
                let mut chars = shown.chars();
                chars.next();
                shown = chars.as_str();
            }
            self.text(ui, shown, left + 62.0, top + 24.0, WHITE, true);
            if cursor && input {
                let x = left + 62.0 + self.font.width(shown);
                self.text(ui, "_", x, top + 24.0, WHITE, true);
            }
        }
        let cost = view.data.first().copied().unwrap_or(0);
        if cost <= 0 {
            return;
        }
        let (line, colour) = if cost >= TOO_EXPENSIVE && !view.creative {
            (
                translate(self.language(), "container.repair.expensive", &[]),
                COST_RED,
            )
        } else if !result {
            return;
        } else {
            let line = translate(
                self.language(),
                "container.repair.cost",
                &[cost.to_string()],
            );
            let pays = view.creative || view.xp_level >= cost;
            (line, if pays { COST } else { COST_RED })
        };
        let x = 176.0 - 8.0 - self.font.width(&line) - 2.0;
        self.fill(
            ui,
            left + x - 2.0,
            top + 67.0,
            168.0 - (x - 2.0),
            12.0,
            BACKING,
        );
        self.text(ui, &line, left + x, top + 69.0, rgb(colour), true);
    }
}

/// Whether the screen sends a name for the input (`onNameChanged`): its
/// default name, unless it has a custom one, is sent as none.
pub fn name_to_send(text: &str, input: &ItemStack, default_name: &str) -> String {
    let named = input
        .components
        .as_ref()
        .and_then(|patch| patch.get("minecraft:custom_name"))
        .is_some_and(|name| !name.is_null());
    if !named && text == default_name {
        String::new()
    } else {
        text.to_owned()
    }
}

/// `setItemName` would take the name: it is valid and not the one the
/// menu has.
pub fn renames(current: Option<&str>, name: &str) -> bool {
    anvil::validate_name(name).is_some_and(|name| current != Some(name.as_str()))
}
