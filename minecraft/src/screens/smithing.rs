//! `SmithingScreen` over its background: the empty inputs' cycling icons
//! (`CyclingSlotBackground`: the templates, then what the template in
//! place takes), the error sprite while three inputs make nothing, and the
//! onboarding tooltips over the error and the empty inputs, wrapped at
//! 115 pixels. The armour stand that previews the result is not drawn:
//! there is no armour stand model to draw it with.
use minecraftoss_player::inventory::ItemStack;

use super::MenuView;
use crate::creative::translate;
use crate::gui::Gui;
use crate::render::UiList;

/// `CyclingSlotBackground`'s ticks between icons, and the cross-fade's.
const ICON_CHANGE_TICK_RATE: u32 = 30;
const FADE_TICKS: f32 = 4.0;
/// `SmithingScreen.TOOLTIP_WIDTH`.
const TOOLTIP_WIDTH: f32 = 115.0;

/// `EMPTY_SLOT_SMITHING_TEMPLATES`.
const TEMPLATES: &[&str] = &[
    "container/slot/smithing_template_armor_trim",
    "container/slot/smithing_template_netherite_upgrade",
];
/// `SmithingTemplateItem.createTrimmableArmorIconList` and
/// `createTrimmableMaterialIconList`.
const TRIM_BASES: &[&str] = &[
    "container/slot/helmet",
    "container/slot/chestplate",
    "container/slot/leggings",
    "container/slot/boots",
];
const TRIM_ADDITIONS: &[&str] = &[
    "container/slot/ingot",
    "container/slot/redstone_dust",
    "container/slot/lapis_lazuli",
    "container/slot/quartz",
    "container/slot/diamond",
    "container/slot/emerald",
    "container/slot/amethyst_shard",
];
/// `createNetheriteUpgradeIconList` and `createNetheriteUpgradeMaterialList`.
const UPGRADE_BASES: &[&str] = &[
    "container/slot/helmet",
    "container/slot/sword",
    "container/slot/chestplate",
    "container/slot/pickaxe",
    "container/slot/leggings",
    "container/slot/axe",
    "container/slot/boots",
    "container/slot/hoe",
    "container/slot/shovel",
    "container/slot/nautilus_armor",
    "container/slot/spear",
];
const UPGRADE_ADDITIONS: &[&str] = &["container/slot/ingot"];

/// A `SmithingTemplateItem`: its icons and descriptions for the base and
/// addition slots.
struct Template {
    bases: &'static [&'static str],
    additions: &'static [&'static str],
    base_description: &'static str,
    addition_description: &'static str,
}

/// The template a stack is, if any: an armour trim or the netherite
/// upgrade.
fn template(stack: Option<&ItemStack>) -> Option<Template> {
    let id = stack?.id.as_str();
    if id == "minecraft:netherite_upgrade_smithing_template" {
        Some(Template {
            bases: UPGRADE_BASES,
            additions: UPGRADE_ADDITIONS,
            base_description: "item.minecraft.smithing_template.netherite_upgrade.base_slot_description",
            addition_description: "item.minecraft.smithing_template.netherite_upgrade.additions_slot_description",
        })
    } else if id.ends_with("_armor_trim_smithing_template") {
        Some(Template {
            bases: TRIM_BASES,
            additions: TRIM_ADDITIONS,
            base_description: "item.minecraft.smithing_template.armor_trim.base_slot_description",
            addition_description: "item.minecraft.smithing_template.armor_trim.additions_slot_description",
        })
    } else {
        None
    }
}

/// One slot's `CyclingSlotBackground`.
#[derive(Clone, Debug, Default)]
struct Cycling {
    icons: &'static [&'static str],
    index: usize,
    tick: u32,
}

impl Cycling {
    /// `tick`: new icons start over; each 30 ticks the next shows.
    fn tick(&mut self, icons: &'static [&'static str]) {
        if self.icons != icons {
            self.icons = icons;
            self.index = 0;
        }
        if !self.icons.is_empty() {
            self.tick += 1;
            if self.tick.is_multiple_of(ICON_CHANGE_TICK_RATE) {
                self.index = (self.index + 1) % self.icons.len();
            }
        }
    }

    /// `extractRenderState`'s icons: the current one fading in over the
    /// last for 4 ticks after each change. (sprite, alpha).
    fn icons(&self, partial: f32) -> Vec<(&'static str, f32)> {
        let Some(&current) = self.icons.get(self.index) else {
            return Vec::new();
        };
        let fades = self.icons.len() > 1 && self.tick >= ICON_CHANGE_TICK_RATE;
        let alpha = if fades {
            ((self.tick % ICON_CHANGE_TICK_RATE) as f32 + partial).min(FADE_TICKS) / FADE_TICKS
        } else {
            1.0
        };
        let mut icons = Vec::new();
        if alpha < 1.0 {
            let previous = (self.index + self.icons.len() - 1) % self.icons.len();
            icons.push((self.icons[previous], 1.0 - alpha));
        }
        icons.push((current, alpha));
        icons
    }
}

/// The template, base and addition slots' cycling icons.
#[derive(Clone, Debug, Default)]
pub struct SmithingIcons([Cycling; 3]);

impl SmithingIcons {
    /// `SmithingScreen.containerTick`, with the template slot's stack.
    pub fn tick(&mut self, template_stack: Option<&ItemStack>) {
        let template = template(template_stack);
        self.0[0].tick(TEMPLATES);
        self.0[1].tick(template.as_ref().map_or(&[], |t| t.bases));
        self.0[2].tick(template.as_ref().map_or(&[], |t| t.additions));
    }

    /// The icons the empty inputs show: (slot, sprite, alpha).
    pub fn shown(
        &self,
        partial: f32,
        empty: impl Fn(usize) -> bool,
    ) -> Vec<(usize, &'static str, f32)> {
        (0..3)
            .filter(|&slot| empty(slot))
            .flat_map(|slot| {
                self.0[slot]
                    .icons(partial)
                    .into_iter()
                    .map(move |(icon, alpha)| (slot, icon, alpha))
            })
            .collect()
    }
}

impl Gui {
    /// `SmithingScreen.extractBackground`: the cycling icons and the error
    /// sprite.
    pub(super) fn smithing_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        for &(slot, icon, alpha) in &view.slot_icons {
            if let Some(slot) = view.slots.get(slot) {
                let (x, y) = (left + slot.x as f32, top + slot.y as f32);
                self.sprite_tinted(ui, icon, x, y, 16.0, 16.0, [1.0, 1.0, 1.0, alpha]);
            }
        }
        if view.data.first().is_some_and(|&error| error > 0) {
            self.sprite(ui, "smithing_error", left + 65.0, top + 46.0, 28.0, 21.0);
        }
    }

    /// `extractOnboardingTooltips`: the error's, or an empty input's.
    pub(super) fn smithing_tooltip(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let error = view.data.first().is_some_and(|&error| error > 0);
        let mut key = None;
        if error && (64.0..94.0).contains(&mx) && (45.0..68.0).contains(&my) {
            key = Some("container.upgrade.error_tooltip");
        }
        let held = |slot: usize| view.slots.get(slot).and_then(|slot| slot.stack.as_ref());
        match (view.hovered, held(0)) {
            (Some(0), None) => key = Some("container.upgrade.missing_template_tooltip"),
            (Some(slot @ 1..=2), Some(stack)) if held(slot).is_none() => {
                if let Some(template) = template(Some(stack)) {
                    key = Some(if slot == 1 {
                        template.base_description
                    } else {
                        template.addition_description
                    });
                }
            }
            _ => {}
        }
        let Some(key) = key else {
            return;
        };
        let text = translate(self.language(), key, &[]);
        let lines: Vec<(String, u32)> = self
            .wrap(&text, TOOLTIP_WIDTH)
            .into_iter()
            .map(|line| (line, 0xFFFFFF))
            .collect();
        self.tooltip_lines(ui, &lines);
    }

    /// `Font.split`: the text in lines no wider than `width`, broken at
    /// spaces where it can be.
    fn wrap(&self, text: &str, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if self.font.width(&candidate) <= width || line.is_empty() {
                line = candidate;
            } else {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            }
        }
        lines.push(line);
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icons_cycle_every_30_ticks_and_fade_for_4() {
        let mut cycling = Cycling::default();
        for _ in 0..29 {
            cycling.tick(TEMPLATES);
        }
        assert_eq!(
            cycling.icons(0.5),
            [(TEMPLATES[0], 1.0)],
            "no fade before the first change"
        );
        cycling.tick(TEMPLATES);
        assert_eq!(cycling.index, 1);
        assert_eq!(
            cycling.icons(1.0),
            [(TEMPLATES[0], 0.75), (TEMPLATES[1], 0.25)]
        );
        for _ in 0..4 {
            cycling.tick(TEMPLATES);
        }
        assert_eq!(cycling.icons(0.0), [(TEMPLATES[1], 1.0)]);
        // Other icons start over, without the ticks counted.
        cycling.tick(UPGRADE_ADDITIONS);
        assert_eq!((cycling.index, cycling.tick), (0, 35));
    }
}
