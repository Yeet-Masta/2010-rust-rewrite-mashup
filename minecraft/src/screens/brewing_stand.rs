//! `BrewingStandScreen`'s progress over its background, from the menu's
//! data values: the fuel bar, shorter as the fuel's uses go, and while a
//! brew is on, the arrow growing down and the bubbles rising in their
//! cycle (`BUBBLELENGTHS`, a step every 2 ticks).
use minecraftoss_player::menu::brewing::{brew_progress, fuel_length};

use super::MenuView;
use crate::gui::Gui;
use crate::render::UiList;

impl Gui {
    /// `BrewingStandScreen.extractBackground`'s three sprites.
    pub(super) fn brewing_stand_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let fuel = fuel_length(&view.data) as f32;
        if fuel > 0.0 {
            let (x, y) = (left + 60.0, top + 44.0);
            self.sprite_part(ui, "brewing_stand_fuel_length", x, y, 0.0, 0.0, fuel, 4.0);
        }
        let Some((arrow, bubbles)) = brew_progress(&view.data) else {
            return;
        };
        if arrow > 0 {
            let (x, y) = (left + 97.0, top + 16.0);
            let h = arrow as f32;
            self.sprite_part(ui, "brewing_stand_brew_progress", x, y, 0.0, 0.0, 9.0, h);
        }
        if bubbles > 0 {
            let h = bubbles as f32;
            let (x, y) = (left + 63.0, top + 14.0 + 29.0 - h);
            self.sprite_part(ui, "brewing_stand_bubbles", x, y, 0.0, 29.0 - h, 12.0, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bars_follow_the_data() {
        // `brewTime`, `fuel`, `totalBrewTime`, `totalFuel`.
        assert_eq!(fuel_length(&[0, 20, 400, 20]), 18);
        assert_eq!(fuel_length(&[0, 1, 400, 20]), 1, "a ceiling");
        assert_eq!(fuel_length(&[0, 5, 400, 0]), 0);
        assert_eq!(brew_progress(&[0, 19, 400, 20]), None);
        assert_eq!(
            brew_progress(&[400, 19, 400, 20]),
            Some((0, 11)),
            "200 % 7 is 4"
        );
        assert_eq!(brew_progress(&[200, 19, 400, 20]), Some((14, 20)));
        assert_eq!(brew_progress(&[3, 19, 400, 20]), Some((27, 24)));
        assert_eq!(brew_progress(&[13, 19, 400, 20]), Some((27, 0)));
    }
}
