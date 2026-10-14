//! Items held in use, and what a bow does with them (26.3
//! `LivingEntity.startUsingItem`/`updatingUsingItem`/`releaseUsingItem`,
//! `BowItem.use`/`releaseUsing`/`getPowerForTime`, `Player.getProjectile`,
//! `ProjectileWeaponItem.getHeldProjectile`/`draw`/`useAmmo`). A use runs
//! from the press until the button is let go (the bow looses then), the
//! held item changes, or a screen opens; meanwhile the player walks at a
//! fifth of its speed, cannot sprint, and attacks nothing.
use minecraft_terrain::held_model::HeldState;
use minecraftoss_entities::world::PlayerShot;
use minecraftoss_player::inventory::ItemStack;

use super::Game;
use crate::hand::UsePose;

/// The offhand's inventory slot.
pub const OFFHAND: usize = 40;

/// What an item in use is (`Item.getUseAnimation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseKind {
    Bow,
}

impl UseKind {
    fn of(id: &str) -> Option<Self> {
        match id {
            "minecraft:bow" => Some(Self::Bow),
            _ => None,
        }
    }
}

/// An item the player is using (`useItem`, `useItemRemaining` counted up).
#[derive(Clone, Debug)]
pub struct ItemUse {
    pub kind: UseKind,
    /// The inventory slot of the hand it is used in.
    pub slot: usize,
    /// The item used.
    pub id: String,
    /// `getUseDuration - useItemRemaining`: the ticks it has been used.
    pub ticks: i32,
}

/// Where a projectile comes from (`Player.getProjectile`): an inventory
/// slot, or a fresh arrow for a creative player without one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ammo {
    Slot(usize),
    Fresh,
}

/// `BowItem.getPowerForTime`: the draw over a second, eased.
pub fn bow_power(ticks: i32) -> f32 {
    let pow = ticks as f32 / 20.0;
    ((pow * pow + pow * 2.0) / 3.0).min(1.0)
}

impl Game {
    /// The stack in an inventory slot.
    fn slot_stack(&self, slot: usize) -> Option<&ItemStack> {
        self.entities.inventory.slots.get(slot).and_then(Option::as_ref)
    }

    /// Whether a stack is an arrow (`#arrows`, `ARROW_ONLY`).
    fn is_arrow(&self, stack: &ItemStack) -> bool {
        self.entities.inventory.recipes.enchantments().in_tag("item", "arrows", &stack.id)
            || matches!(stack.id.as_str(), "minecraft:arrow" | "minecraft:tipped_arrow" | "minecraft:spectral_arrow")
    }

    /// `Player.getProjectile` for a bow: the offhand's arrows, then the main
    /// hand's (`getHeldProjectile`), then the first in the inventory; a
    /// creative player without any has a fresh one.
    fn find_ammo(&self) -> Option<Ammo> {
        let selected = self.entities.selected;
        let held = [OFFHAND, selected].into_iter().find(|&slot| self.slot_stack(slot).is_some_and(|s| self.is_arrow(s)));
        let found = held.or_else(|| (0..self.entities.inventory.slots.len()).find(|&slot| self.slot_stack(slot).is_some_and(|s| self.is_arrow(s))));
        match found {
            Some(slot) => Some(Ammo::Slot(slot)),
            None if self.creative => Some(Ammo::Fresh),
            None => None,
        }
    }

    /// `BowItem.use` with the stack in `slot`: the draw starts when there is
    /// an arrow to loose (always for a creative player). Whether it did.
    pub(super) fn start_using(&mut self, slot: usize) -> bool {
        let Some(stack) = self.slot_stack(slot).cloned() else { return false };
        let Some(kind) = UseKind::of(&stack.id) else { return false };
        let ok = match kind {
            UseKind::Bow => self.creative || self.find_ammo().is_some(),
        };
        if ok {
            self.using = Some(ItemUse { kind, slot, id: stack.id, ticks: 0 });
        }
        ok
    }

    /// One tick of the use: it stops when the button is let go (the item's
    /// `releaseUsing`) or its hand no longer holds it
    /// (`updatingUsingItem`), else counts on (`updateUsingItem`).
    pub(super) fn tick_using(&mut self, held: bool) {
        let Some(using) = self.using.clone() else { return };
        if self.slot_stack(using.slot).is_none_or(|s| s.id != using.id) || (using.slot != OFFHAND && using.slot != self.entities.selected) {
            self.using = None;
            return;
        }
        if !held {
            self.using = None;
            self.release_using(&using);
            return;
        }
        if let Some(using) = self.using.as_mut() {
            using.ticks += 1;
        }
    }

    /// `releaseUsing` for the item let go.
    fn release_using(&mut self, using: &ItemUse) {
        match using.kind {
            UseKind::Bow => self.release_bow(using),
        }
    }

    /// `BowItem.releaseUsing`: drawn to a tenth of its power or more, the
    /// bow looses what it draws at three times the power (critical at full
    /// draw), with `entity.arrow.shoot`, and wears a point per arrow.
    fn release_bow(&mut self, using: &ItemUse) {
        let Some(ammo) = self.find_ammo() else { return };
        let power = bow_power(using.ticks);
        if power < 0.1 {
            return;
        }
        let Some(weapon) = self.slot_stack(using.slot).cloned() else { return };
        let projectiles = self.draw(&weapon, ammo);
        if !projectiles.is_empty() {
            self.loose(using.slot, &weapon, projectiles, power * 3.0, 1.0, power == 1.0, false);
        }
        let pitch = 1.0 / (self.sounds.random() * 0.4 + 1.2) + power * 0.5;
        self.play("minecraft:entity.arrow.shoot", Some(self.player.pos), 1.0, pitch);
    }

    /// `ProjectileWeaponItem.draw`: the weapon's projectile count
    /// (multishot) of arrows, the first from the ammo and the rest free
    /// copies of it (`useAmmo`).
    fn draw(&mut self, weapon: &ItemStack, ammo: Ammo) -> Vec<ItemStack> {
        let effects = self.entities.inventory.recipes.enchantments().clone();
        let me = self.player.facts(&self.world.scene, self.player.velocity);
        let count = effects.projectile_count(weapon, &me, 1, self.player.equipped.random());
        let original = match ammo {
            Ammo::Slot(slot) => self.slot_stack(slot).cloned(),
            Ammo::Fresh => Some(self.entities.stack("minecraft:arrow", 1)),
        };
        let Some(original) = original else { return Vec::new() };
        let mut drawn = Vec::new();
        for i in 0..count {
            let free = i > 0;
            // `useAmmo`: infinity, a creative player and the copies use none.
            let spend = if free || self.creative { 0 } else { effects.ammo_use(weapon, &original, 1, self.player.equipped.random()) };
            if spend == 0 {
                let mut copy = original.clone();
                copy.count = 1;
                let components = copy.components.get_or_insert_with(|| serde_json::json!({}));
                if let Some(map) = components.as_object_mut() {
                    map.insert("minecraft:intangible_projectile".to_owned(), serde_json::json!({}));
                }
                drawn.push(copy);
                continue;
            }
            let Ammo::Slot(slot) = ammo else { continue };
            let Some(stack) = self.entities.inventory.slots[slot].as_mut() else { continue };
            if spend > i32::from(stack.count) {
                continue;
            }
            let mut used = stack.clone();
            used.count = spend as u8;
            stack.count -= spend as u8;
            if stack.count == 0 {
                self.entities.inventory.slots[slot] = None;
            }
            drawn.push(used);
        }
        drawn
    }

    /// `ProjectileWeaponItem.shoot` on the server's side, then the
    /// weapon's wear (`hurtAndBreak`, a point per projectile).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn loose(&mut self, slot: usize, weapon: &ItemStack, projectiles: Vec<ItemStack>, power: f32, uncertainty: f32, crit: bool, crossbow: bool) {
        let count = projectiles.len();
        // `getKnownMovement`: this tick's motion.
        let motion = self.player.pos - self.previous;
        self.entities.shoot(PlayerShot {
            player_id: 0,
            eye: self.player.eye(),
            x_rot: self.player.pitch as f32,
            y_rot: self.player.yaw as f32,
            motion,
            on_ground: self.player.on_ground,
            weapon: weapon.clone(),
            projectiles,
            power,
            uncertainty,
            crit,
            crossbow,
            game_mode: if self.creative { "creative" } else { "survival" },
        });
        if !self.creative {
            for _ in 0..count {
                if self.entities.inventory.wear_tool(slot, 1) {
                    self.item_broke(&weapon.id);
                    break;
                }
            }
        }
    }

    /// The use in `slot`, if that hand's item is in use.
    fn use_in(&self, slot: usize) -> Option<&ItemUse> {
        self.using.as_ref().filter(|using| using.slot == slot)
    }

    /// What the model of the item in `slot` reads of its use
    /// (`minecraft:using_item`, `use_duration`).
    pub(super) fn held_state(&self, slot: usize) -> HeldState {
        match self.use_in(slot) {
            Some(using) => HeldState { using: true, use_ticks: using.ticks as f32, pull: 0.0 },
            None => HeldState::default(),
        }
    }

    /// The use animation of the item in `slot`, if it is in use, with the
    /// ticks held through the frame.
    pub(super) fn use_pose(&self, slot: usize, partial: f32) -> Option<UsePose> {
        let using = self.use_in(slot)?;
        let time_held = using.ticks as f32 + partial - 1.0;
        Some(match using.kind {
            UseKind::Bow => UsePose::Bow(time_held),
        })
    }

    /// The first-person pose of the hand holding `slot` when its item's
    /// use moves it (`submitArmWithItem`).
    pub(super) fn ranged_hand_pose(&self, slot: usize, _swing: f32, equip: f32, left: bool, partial: f32) -> Option<glam::Mat4> {
        self.use_pose(slot, partial).map(|pose| crate::hand::use_pose(pose, equip, left))
    }

    /// `FirstPersonHandsAndItems.evaluateWhichHandsToRender`: drawing a bow
    /// shows only its hand. The main hand, then the offhand.
    pub(super) fn hands_shown(&self) -> (bool, bool) {
        match self.using.as_ref() {
            Some(using) if using.kind == UseKind::Bow => (using.slot != OFFHAND, using.slot == OFFHAND),
            _ => (true, true),
        }
    }

    /// `AbstractClientPlayer.getFieldOfViewModifier`'s use: a drawn bow
    /// narrows the view by up to 15%.
    pub(super) fn use_fov(&self) -> f32 {
        match self.using.as_ref() {
            Some(using) if using.kind == UseKind::Bow => {
                let scale = (using.ticks as f32 / 20.0).min(1.0);
                1.0 - scale * scale * 0.15
            }
            _ => 1.0,
        }
    }
}
