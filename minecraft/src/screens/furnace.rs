//! `AbstractFurnaceScreen`'s progress over its background: the flame while
//! the furnace is lit, shrinking from the top as the fuel burns, and the
//! arrow growing as the ingredient cooks, from the menu's data values. The
//! recipe book's button is not drawn: there is no recipe book screen.
use minecraft_terrain::menus::MenuKind;
use minecraftoss_player::menu::furnace::{burn_progress, is_lit, lit_progress};

use super::MenuView;
use crate::gui::Gui;
use crate::render::UiList;

/// The kind's `litProgressSprite` and `burnProgressSprite`.
fn sprites(kind: MenuKind) -> (&'static str, &'static str) {
    match kind {
        MenuKind::BlastFurnace => (
            "blast_furnace_lit_progress",
            "blast_furnace_burn_progress",
        ),
        MenuKind::Smoker => ("smoker_lit_progress", "smoker_burn_progress"),
        _ => ("furnace_lit_progress", "furnace_burn_progress"),
    }
}

/// The flame's height, 1 to 14 (`ceil(litProgress * 13) + 1`), and the
/// arrow's width, 0 to 24 (`ceil(burnProgress * 24)`).
fn progress(data: &[i32]) -> (Option<f32>, f32) {
    let flame = is_lit(data).then(|| (lit_progress(data) * 13.0).ceil() + 1.0);
    (flame, (burn_progress(data) * 24.0).ceil())
}

impl Gui {
    /// `AbstractFurnaceScreen.extractBackground`'s two sprites.
    pub(super) fn furnace_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let (lit, burn) = sprites(view.kind);
        let (flame, arrow) = progress(view.data);
        if let Some(h) = flame {
            self.sprite_part(
                ui,
                lit,
                left + 56.0,
                top + 36.0 + 14.0 - h,
                0.0,
                14.0 - h,
                14.0,
                h,
            );
        }
        if arrow > 0.0 {
            self.sprite_part(ui, burn, left + 79.0, top + 34.0, 0.0, 0.0, arrow, 16.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flame_and_arrow_follow_the_data() {
        assert_eq!(progress(&[0, 1600, 0, 200]), (None, 0.0), "unlit, nothing cooked");
        assert_eq!(progress(&[1600, 1600, 100, 200]), (Some(14.0), 12.0));
        assert_eq!(progress(&[1, 1600, 199, 200]), (Some(2.0), 24.0));
        assert_eq!(sprites(MenuKind::Smoker).0, "smoker_lit_progress");
    }
}
