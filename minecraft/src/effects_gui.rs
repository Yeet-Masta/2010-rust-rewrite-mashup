//! The player's status effects as the GUI shows them: their icons along
//! the top right of the HUD (`Hud.extractEffects`), the beneficial in the
//! top row and the rest below, blinking out over their last ten seconds;
//! and the list beside the inventory (`EffectsInInventory`), with each
//! effect's name, level and time left.
use minecraftoss_player::effects::{ActiveEffects, EffectInstance};

use super::{Gui, Screen, WHITE};
use crate::font::rgb;
use crate::render::UiList;

impl Gui {
    /// `Hud.extractEffects`: in `MobEffectInstance.compareTo`'s reverse
    /// order, each effect with an icon right to left, 25 pixels apart, on
    /// its ambient or plain background.
    pub(super) fn effect_icons(&self, ui: &mut UiList, effects: &ActiveEffects) {
        let (mut beneficial, mut harmful) = (0, 0);
        for effect in sorted(effects).into_iter().rev() {
            if !effect.show_icon {
                continue;
            }
            let (mut x, mut y) = (self.width, 1.0);
            if effect.effect.beneficial() {
                beneficial += 1;
                x -= 25.0 * beneficial as f32;
            } else {
                harmful += 1;
                x -= 25.0 * harmful as f32;
                y += 26.0;
            }
            let mut alpha = 1.0;
            if effect.ambient {
                self.sprite(ui, "effect_background_ambient", x, y, 24.0, 24.0);
            } else {
                self.sprite(ui, "effect_background", x, y, 24.0, 24.0);
                if effect.ends_within(200) {
                    let left = effect.duration;
                    let used = 10 - left / 20;
                    let pulse = minecraftoss_player::mth::cos(f64::from(left as f32 * std::f32::consts::PI / 5.0));
                    alpha = (left as f32 / 10.0 / 5.0 * 0.5).clamp(0.0, 0.5) + pulse * (used as f32 / 10.0 * 0.25).clamp(0.0, 0.25);
                    alpha = alpha.clamp(0.0, 1.0);
                }
            }
            let icon = format!("mob_effect/{}", effect.effect.name());
            self.sprite_tinted(ui, &icon, x + 3.0, y + 3.0, 18.0, 18.0, [1.0, 1.0, 1.0, alpha]);
        }
    }

    /// Where an open screen lists the effects (`EffectsInInventory`): just
    /// right of its window, by its top, when there is room for an icon
    /// (`canSeeEffects`). The survival inventory and the creative screen
    /// list them.
    pub fn effects_origin(&self, screen: Screen) -> Option<(f32, f32)> {
        let (left, top, width) = match screen {
            Screen::Inventory => (((self.width - 176.0) / 2.0).floor(), ((self.height - 166.0) / 2.0).floor(), 176.0),
            Screen::Creative => {
                let (left, top) = self.creative_corner();
                (left, top, 195.0)
            }
            _ => return None,
        };
        let x = left + width + 2.0;
        (self.width - x >= 32.0).then_some((x, top))
    }

    /// `EffectsInInventory.extractRenderState`: each effect in
    /// `compareTo` order on a background as wide as its name or time
    /// needs (as the room allows; icons alone when under 120 pixels),
    /// 33 pixels apart or closer for more than five; a clipped name's
    /// tooltip shows when hovered.
    pub fn effects_in_inventory(&self, ui: &mut UiList, effects: &ActiveEffects, (x0, top): (f32, f32)) {
        if effects.is_empty() {
            return;
        }
        let available = self.width - x0;
        let max_width = if available >= 120.0 { available - 7.0 } else { 32.0 };
        let count = effects.iter().count();
        let step = if count > 5 { (132 / (count - 1)) as f32 } else { 33.0 };
        let mut y = top;
        let mut tooltip = None;
        for effect in sorted(effects) {
            let name = self.effect_name(effect);
            let duration = effect_duration(effect, self);
            let width = (32.0 + self.font.width(&name) + 7.0)
                .max(32.0 + self.font.width(&duration) + 7.0)
                .min(max_width);
            let background = if effect.ambient {
                "inventory_effect_background_ambient"
            } else {
                "inventory_effect_background"
            };
            self.sprite(ui, background, x0, y, width, 32.0);
            let room = width - 32.0 - 7.0;
            let compact = if room > 0.0 {
                let clipped = self.font.width(&name) > room;
                let shown = if clipped { self.clip(&name, room) } else { name.clone() };
                self.text(ui, &shown, x0 + 32.0, y + 7.0, WHITE, true);
                self.text(ui, &duration, x0 + 32.0, y + 16.0, rgb(0x808080), true);
                clipped
            } else {
                true
            };
            let (mx, my) = self.mouse;
            if compact && mx >= x0 && mx <= x0 + width && my >= y && my <= y + step {
                tooltip = Some(vec![(name, 0xFFFFFF), (duration, 0xFFFFFF)]);
            }
            let icon = format!("mob_effect/{}", effect.effect.name());
            self.sprite(ui, &icon, x0 + 7.0, y + 7.0, 18.0, 18.0);
            y += step;
        }
        if let Some(lines) = tooltip {
            self.tooltip_lines(ui, &lines);
        }
    }

    /// `EffectsInInventory.getEffectName`: the effect's name, with its
    /// level in numerals from II to X.
    fn effect_name(&self, effect: &EffectInstance) -> String {
        let language = self.language();
        let key = effect.effect.description_id();
        let mut name = language.get(&key).cloned().unwrap_or(key);
        if (1..=9).contains(&effect.amplifier) {
            let level = format!("enchantment.level.{}", effect.amplifier + 1);
            name.push(' ');
            name.push_str(language.get(&level).map_or(level.as_str(), String::as_str));
        }
        name
    }

    /// `ComponentRenderUtils.clipText`: as much as fits, then an ellipsis.
    fn clip(&self, text: &str, room: f32) -> String {
        let ellipsis = "...";
        let room = room - self.font.width(ellipsis);
        let mut out = String::new();
        for c in text.chars() {
            if self.font.width(&format!("{out}{c}")) > room {
                break;
            }
            out.push(c);
        }
        out + ellipsis
    }
}

/// The effects in `MobEffectInstance.compareTo` order.
fn sorted(effects: &ActiveEffects) -> Vec<&EffectInstance> {
    let mut list: Vec<&EffectInstance> = effects.iter().collect();
    list.sort_by(|a, b| a.compare(b));
    list
}

/// `MobEffectUtil.formatDuration` at 20 ticks a second: minutes and
/// seconds (hours first when there are any), or "Infinite".
fn effect_duration(effect: &EffectInstance, gui: &Gui) -> String {
    if effect.infinite() {
        let key = "effect.duration.infinite";
        return gui.language().get(key).cloned().unwrap_or_else(|| key.to_owned());
    }
    format_ticks(effect.duration)
}

/// `StringUtil.formatTickDuration` at 20 ticks a second.
fn format_ticks(ticks: i32) -> String {
    let seconds = ticks.max(0) / 20;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations_read_as_vanilla_formats_them() {
        assert_eq!(super::format_ticks(3600), "03:00");
        assert_eq!(super::format_ticks(120000), "01:40:00");
        assert_eq!(super::format_ticks(19), "00:00");
    }
}
