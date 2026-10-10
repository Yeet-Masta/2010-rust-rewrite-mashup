//! `CartographyTableScreen` over its background: the error sprite when the
//! map is locked and would be zoomed or locked, or zoomed past its last
//! scale, and the frame of the map the inputs make (zoomed, two, locked,
//! or as it is), from the map's data as the server sent it.
//!
//! Not drawn: the map itself in the frame, as there is no map rendering
//! (no map system) yet.
use minecraft_terrain::menus::cartography;
use minecraftoss_player::menu::cartography::{ADDITIONAL, MAP, MAX_SCALE, map_id};

use super::MenuView;
use crate::gui::Gui;
use crate::render::UiList;

impl Gui {
    /// `CartographyTableScreen.extractBackground` and `extractResultingMap`.
    pub(super) fn cartography_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let stack = |slot: usize| view.slots.get(slot).and_then(|slot| slot.stack.as_ref());
        let additional = stack(ADDITIONAL).map(|stack| stack.id.as_str());
        let duplicating = additional == Some("minecraft:map");
        let scaling = additional == Some("minecraft:paper");
        let locking = additional == Some("minecraft:glass_pane");
        let mut locked = false;
        let map = stack(MAP).filter(|map| map_id(map).is_some());
        let data = map.and_then(|_| cartography(view.menu)?.map_info());
        if let (Some(map), Some(data)) = (map, data) {
            if data.locked {
                locked = true;
                if scaling || locking {
                    self.sprite(ui, "cartography_table_error", left + 35.0, top + 31.0, 28.0, 21.0);
                }
            }
            let extendable = view
                .inventory
                .recipes
                .item_in_tag("minecraft:extendable_maps", &map.id);
            if scaling && (!extendable || data.scale >= MAX_SCALE) {
                locked = true;
                self.sprite(ui, "cartography_table_error", left + 35.0, top + 31.0, 28.0, 21.0);
            }
        }
        if scaling && !locked {
            self.sprite(ui, "cartography_table_scaled_map", left + 67.0, top + 13.0, 66.0, 66.0);
        } else if duplicating {
            self.sprite(ui, "cartography_table_duplicated_map", left + 83.0, top + 13.0, 50.0, 66.0);
            self.sprite(ui, "cartography_table_duplicated_map", left + 67.0, top + 29.0, 50.0, 66.0);
        } else {
            self.sprite(ui, "cartography_table_map", left + 67.0, top + 13.0, 66.0, 66.0);
            if locking {
                self.sprite(ui, "cartography_table_locked", left + 118.0, top + 60.0, 10.0, 14.0);
            }
        }
    }
}
