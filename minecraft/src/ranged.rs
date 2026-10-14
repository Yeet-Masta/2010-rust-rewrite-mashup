//! Items held in use, and what a bow does with them (26.3
//! `LivingEntity.startUsingItem`/`updatingUsingItem`/`releaseUsingItem`,
//! `BowItem.use`/`releaseUsing`/`getPowerForTime`, `Player.getProjectile`,
//! `ProjectileWeaponItem.getHeldProjectile`/`draw`/`useAmmo`,
//! `CrossbowItem.use`/`onUseTick`/`performShooting`,
//! `TridentItem.use`/`releaseUsing`, `Player.startAutoSpinAttack`,
//! `LivingEntity.checkAutoSpinAttack`, `LivingEntity.getItemBlockingWith`
//! and `applyItemBlocking` with a shield's `BlocksAttacks`). A use runs
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
    Crossbow,
    Trident,
    /// `BLOCK`: a shield raised.
    Shield,
}

impl UseKind {
    fn of(id: &str) -> Option<Self> {
        match id {
            "minecraft:bow" => Some(Self::Bow),
            "minecraft:crossbow" => Some(Self::Crossbow),
            "minecraft:trident" => Some(Self::Trident),
            "minecraft:shield" => Some(Self::Shield),
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
    /// `CrossbowItem.startSoundPlayed` and `midLoadSoundPlayed`.
    start_sound: bool,
    mid_sound: bool,
}

/// A riptide's spin (`startAutoSpinAttack`): the ticks left, the damage
/// it strikes with, and the trident in its hand's slot.
#[derive(Clone, Debug)]
pub struct SpinAttack {
    ticks: i32,
    damage: f32,
    item: ItemStack,
    slot: usize,
}

/// Where a projectile comes from (`Player.getProjectile`): an inventory
/// slot, or a fresh arrow for a creative player without one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ammo {
    Slot(usize),
    Fresh,
}

/// A crossbow's loaded projectiles (`minecraft:charged_projectiles`).
fn charged(stack: &ItemStack) -> Vec<ItemStack> {
    let list = stack.components.as_ref().and_then(|c| c["minecraft:charged_projectiles"].as_array());
    list.into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item["id"].as_str()?;
            let count = item["count"].as_u64().unwrap_or(1).clamp(1, 64) as u8;
            let mut stack = ItemStack::new(id, count);
            stack.components = item.get("components").cloned();
            Some(stack)
        })
        .collect()
}

/// `ChargedProjectiles`: a crossbow loaded with `projectiles`, or emptied.
fn set_charged(stack: &mut ItemStack, projectiles: &[ItemStack]) {
    let components = stack.components.get_or_insert_with(|| serde_json::json!({}));
    let Some(map) = components.as_object_mut() else { return };
    if projectiles.is_empty() {
        map.remove("minecraft:charged_projectiles");
    } else {
        let list = projectiles
            .iter()
            .map(|p| {
                let mut item = serde_json::json!({"id": p.id, "count": p.count});
                if let Some(components) = p.components.clone() {
                    item["components"] = components;
                }
                item
            })
            .collect();
        map.insert("minecraft:charged_projectiles".to_owned(), serde_json::Value::Array(list));
    }
    if map.is_empty() {
        stack.components = None;
    }
}

/// `CrossbowItem.getShotPitch`: the first shot at 1, the others higher
/// and lower in turn.
fn shot_pitch(random: f32, index: usize) -> f32 {
    if index == 0 {
        return 1.0;
    }
    let range = if index & 1 == 1 { 0.63 } else { 0.43 };
    1.0 / (random * 0.5 + 1.8) + range
}

/// A sound in an enchantment's `crossbow_charging_sounds`: an event's ID
/// or a sound event.
fn sound_id(value: &serde_json::Value) -> Option<String> {
    value.as_str().or_else(|| value["sound_id"].as_str()).map(str::to_owned)
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
        // `startUseItem`: an item cooling down does nothing.
        if self.on_cooldown(&stack.id) {
            return false;
        }
        let ok = match kind {
            UseKind::Bow => self.creative || self.find_ammo().is_some(),
            UseKind::Crossbow => {
                // A loaded crossbow shoots at once (`performShooting`).
                let projectiles = charged(&stack);
                if !projectiles.is_empty() {
                    self.shoot_crossbow(slot, projectiles);
                    return true;
                }
                self.find_ammo().is_some()
            }
            // `TridentItem.use`: not one about to break, and riptide only in
            // water or rain.
            UseKind::Trident => !self.next_damage_breaks(&stack) && (self.riptide(&stack) <= 0.0 || self.wet()),
            // `Item.use` with `BLOCKS_ATTACKS`: raised until let go.
            UseKind::Shield => true,
        };
        if ok {
            self.using = Some(ItemUse { kind, slot, id: stack.id, ticks: 0, start_sound: false, mid_sound: false });
        }
        ok
    }

    /// `ItemStack.nextDamageWillBreak`.
    fn next_damage_breaks(&self, stack: &ItemStack) -> bool {
        self.entities.inventory.recipes.durability(stack).is_some_and(|(damage, max)| damage + 1 >= max)
    }

    /// `EnchantmentHelper.getTridentSpinAttackStrength`.
    fn riptide(&mut self, trident: &ItemStack) -> f32 {
        let effects = self.entities.inventory.recipes.enchantments().clone();
        effects.trident_spin_attack_strength(trident, self.player.equipped.random())
    }

    /// `isInWaterOrRain`.
    fn wet(&self) -> bool {
        self.player.in_water || self.player.in_rain
    }

    /// `TridentItem.releaseUsing`: held half a second or more, the trident
    /// is thrown (2.5, spread 1) with its sound, worn a point first, or
    /// with riptide in water or rain carries the player off along the look
    /// at its strength, spinning for a second (lifted 1.2 off the ground).
    fn release_trident(&mut self, using: &ItemUse) {
        if using.ticks < 10 {
            return;
        }
        let Some(trident) = self.slot_stack(using.slot).cloned() else { return };
        let strength = self.riptide(&trident);
        if (strength > 0.0 && !self.wet()) || self.next_damage_breaks(&trident) {
            return;
        }
        // `pickHighestLevel(TRIDENT_SOUND)`: riptide's by its level.
        let effects = self.entities.inventory.recipes.enchantments();
        let sound = effects
            .highest_level(&trident, "minecraft:trident_sound")
            .and_then(|(list, level)| {
                let list = list.as_array()?;
                list.get((level.max(1) as usize - 1).min(list.len().checked_sub(1)?))?.as_str().map(str::to_owned)
            })
            .unwrap_or_else(|| "minecraft:item.trident.throw".to_owned());
        // `hurtWithoutBreaking`.
        if !self.creative {
            self.entities.inventory.wear_tool(using.slot, 1);
        }
        if strength <= 0.0 {
            // `consumeAndReturn`: a creative player keeps it.
            let Some(mut thrown) = self.slot_stack(using.slot).cloned() else { return };
            thrown.count = 1;
            if !self.creative
                && let Some(stack) = self.entities.inventory.slots[using.slot].as_mut()
            {
                stack.count -= 1;
                if stack.count == 0 {
                    self.entities.inventory.slots[using.slot] = None;
                }
            }
            self.entities.throw_trident(PlayerShot {
                player_id: 0,
                eye: self.player.eye(),
                x_rot: self.player.pitch as f32,
                y_rot: self.player.yaw as f32,
                motion: self.player.pos - self.previous,
                on_ground: self.player.on_ground,
                weapon: thrown.clone(),
                projectiles: vec![thrown],
                power: 2.5,
                uncertainty: 1.0,
                crit: false,
                crossbow: false,
                game_mode: if self.creative { "creative" } else { "survival" },
            });
            self.play(&sound, Some(self.player.eye()), 1.0, 1.0);
            return;
        }
        use minecraftoss_player::mth::{cos, sin};
        let rad = std::f32::consts::PI / 180.0;
        let (y_rot, x_rot) = (f64::from(self.player.yaw as f32 * rad), f64::from(self.player.pitch as f32 * rad));
        let (mut xd, mut yd, mut zd) = (-sin(y_rot) * cos(x_rot), -sin(x_rot), cos(y_rot) * cos(x_rot));
        let dist = (xd * xd + yd * yd + zd * zd).sqrt();
        xd *= strength / dist;
        yd *= strength / dist;
        zd *= strength / dist;
        self.player.velocity += glam::DVec3::new(f64::from(xd), f64::from(yd), f64::from(zd));
        self.riptide_spin = Some(SpinAttack { ticks: 20, damage: 8.0, item: trident, slot: using.slot });
        self.player.auto_spin = true;
        if self.player.on_ground {
            self.player.move_clipped(&self.world.scene, glam::DVec3::new(0.0, f64::from(1.199_999_9_f32), 0.0));
        }
        self.play(&sound, Some(self.player.pos), 1.0, 1.0);
    }

    /// `LivingEntity.aiStep`'s spin: a tick less, then
    /// `checkAutoSpinAttack` over the box swept since `before`: the first
    /// mob in it is struck (`doAutoAttackOnTouch`, the trident's 8 in
    /// place of the attack damage) and the spin ends with the player
    /// thrown back a fifth; running into a wall ends it too.
    pub(super) fn tick_spin(&mut self, before: glam::DVec3) {
        let Some(mut spin) = self.riptide_spin.take() else { return };
        spin.ticks -= 1;
        let now = self.player.pos;
        let half = glam::DVec3::new(0.3, 0.0, 0.3);
        // The spin's pose is 0.6 high.
        let height = glam::DVec3::new(0.0, 0.6, 0.0);
        let min = before.min(now) - half;
        let max = before.max(now) + half + height;
        if let Some(hit) = self.entities.mob_in_box(min, max) {
            let attack = self.melee_attack(Some(&spin.item), Some(f64::from(spin.damage)));
            self.entities.attack_mob(hit, attack);
            self.player.attack_strength_ticker = 0;
            self.player.survival.food.add_exhaustion(0.1);
            if !self.creative && self.slot_stack(spin.slot).is_some_and(|s| s.id == spin.item.id) && self.entities.inventory.wear_tool(spin.slot, 1) {
                self.item_broke(&spin.item.id);
            }
            spin.ticks = 0;
            self.player.velocity *= -0.2;
        } else if self.player.horizontal_collision {
            spin.ticks = 0;
        }
        if spin.ticks > 0 {
            self.riptide_spin = Some(spin);
        }
        self.player.auto_spin = self.riptide_spin.is_some();
    }

    /// `getItemBlockingWith`: the slot of a shield raised for its block
    /// delay (`block_delay_seconds`, a quarter second).
    pub(super) fn blocking_slot(&self) -> Option<usize> {
        self.using.as_ref().filter(|using| using.kind == UseKind::Shield && using.ticks >= 5).map(|using| using.slot)
    }

    /// `applyItemBlocking` on a mob's hit: a raised shield facing where
    /// the hit comes from takes all of it (its one damage reduction: 90
    /// degrees, everything), unless the hit passes shields
    /// (`#bypasses_shield`: lightning) or is a piercing arrow. Then
    /// `hurtBlockingItem` wears it by 1 and the damage once that is 3 or
    /// more, `onBlocked` sounds `item.shield.block`, and an axe's blow
    /// puts the shield down for its seconds (`BlocksAttacks.disable`).
    /// Whether it took the hit, which then does nothing else.
    pub(super) fn shield_blocks(&mut self, hit: &minecraftoss_entities::world::PlayerHit) -> bool {
        use minecraftoss_entities::world::PlayerHitKind;
        let Some(slot) = self.blocking_slot() else { return false };
        if hit.damage <= 0.0 {
            return false;
        }
        let (source, disables) = match hit.kind {
            PlayerHitKind::Melee { attacker, weapon, .. } => (attacker, weapon.disables_blocking),
            PlayerHitKind::Arrow { pierces: true, .. } | PlayerHitKind::Lightning => return false,
            PlayerHitKind::Arrow { position, .. } => (position, 0.0),
            PlayerHitKind::Explosion { center, .. } => (center, 0.0),
        };
        if !minecraftoss_entities::world::shield_faces(self.player.yaw as f32, self.player.pos, source) {
            return false;
        }
        let at = self.player.pos;
        if !self.creative && hit.damage >= 3.0 {
            let wear = (1.0 + hit.damage).floor() as u32;
            if self.entities.inventory.wear_tool(slot, wear) {
                self.item_broke("minecraft:shield");
            }
        }
        let pitch = 0.8 + self.sounds.random() * 0.4;
        self.play("minecraft:item.shield.block", Some(at), 1.0, pitch);
        if disables > 0.0 {
            self.add_cooldown("minecraft:shield", (disables * 20.0) as u64);
            self.using = None;
            let pitch = 0.8 + self.sounds.random() * 0.4;
            self.play("minecraft:item.shield.break", Some(at), 0.8, pitch);
        }
        true
    }

    /// `CrossbowItem.getChargeDuration`: a second and a quarter, less quick
    /// charge's, in ticks.
    fn charge_duration(&mut self, crossbow: &ItemStack) -> i32 {
        let effects = self.entities.inventory.recipes.enchantments().clone();
        (effects.crossbow_charge_time(crossbow, 1.25, self.player.equipped.random()) * 20.0).floor() as i32
    }

    /// `CrossbowItem.getChargingSounds`: the highest-levelled enchantment's
    /// (quick charge's), else the loading start, middle and end.
    fn charging_sounds(&self, crossbow: &ItemStack) -> [Option<String>; 3] {
        let effects = self.entities.inventory.recipes.enchantments();
        let picked = effects.highest_level(crossbow, "minecraft:crossbow_charging_sounds").and_then(|(list, level)| {
            let list = list.as_array()?;
            list.get((level.max(1) as usize - 1).min(list.len().checked_sub(1)?))
        });
        match picked {
            Some(sounds) => ["start", "mid", "end"].map(|part| sounds.get(part).and_then(sound_id)),
            None => ["start", "middle", "end"].map(|part| Some(format!("minecraft:item.crossbow.loading_{part}"))),
        }
    }

    /// `CrossbowItem.onUseTick`: the loading sounds at a fifth and at half
    /// the charge, and once fully charged the projectiles drawn into it
    /// (`tryLoadProjectiles`) with the end sound.
    fn tick_crossbow(&mut self) {
        let Some(mut using) = self.using.clone() else { return };
        let Some(crossbow) = self.slot_stack(using.slot).cloned() else { return };
        let percent = using.ticks as f32 / self.charge_duration(&crossbow) as f32;
        let [start, mid, end] = self.charging_sounds(&crossbow);
        let at = self.player.pos;
        if percent < 0.2 {
            using.start_sound = false;
            using.mid_sound = false;
        }
        if percent >= 0.2 && !using.start_sound {
            using.start_sound = true;
            if let Some(sound) = start {
                self.play(&sound, Some(at), 0.5, 1.0);
            }
        }
        if percent >= 0.5 && !using.mid_sound {
            using.mid_sound = true;
            if let Some(sound) = mid {
                self.play(&sound, Some(at), 0.5, 1.0);
            }
        }
        if percent >= 1.0
            && charged(&crossbow).is_empty()
            && let Some(ammo) = self.find_ammo()
        {
            let drawn = self.draw(&crossbow, ammo);
            if !drawn.is_empty() {
                if let Some(stack) = self.entities.inventory.slots[using.slot].as_mut() {
                    set_charged(stack, &drawn);
                }
                if let Some(sound) = end {
                    let pitch = 1.0 / (self.sounds.random() * 0.5 + 1.0) + 0.2;
                    self.play(&sound, Some(at), 1.0, pitch);
                }
            }
        }
        self.using = Some(using);
    }

    /// `CrossbowItem.performShooting`: the crossbow is emptied and looses
    /// what it held, critical, at 3.15 (`ARROW_POWER`), each with its
    /// `item.crossbow.shoot` (`shootProjectile`).
    fn shoot_crossbow(&mut self, slot: usize, projectiles: Vec<ItemStack>) {
        let Some(stack) = self.entities.inventory.slots[slot].as_mut() else { return };
        set_charged(stack, &[]);
        let weapon = stack.clone();
        for index in 0..projectiles.len() {
            let pitch = shot_pitch(self.sounds.random(), index);
            self.play("minecraft:item.crossbow.shoot", Some(self.player.pos), 1.0, pitch);
        }
        self.loose(slot, &weapon, projectiles, 3.15, 1.0, true, true);
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
        if using.kind == UseKind::Crossbow {
            self.tick_crossbow();
        }
        if let Some(using) = self.using.as_mut() {
            using.ticks += 1;
        }
    }

    /// `releaseUsing` for the item let go.
    fn release_using(&mut self, using: &ItemUse) {
        match using.kind {
            UseKind::Bow => self.release_bow(using),
            // `CrossbowItem.releaseUsing` only ends the use.
            UseKind::Crossbow => {}
            UseKind::Trident => self.release_trident(using),
            UseKind::Shield => {}
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
    /// A crossbow's `crossbow/pull` is its use over its charge, 0 once
    /// loaded.
    pub(super) fn held_state(&mut self, slot: usize) -> HeldState {
        let Some(using) = self.use_in(slot).cloned() else { return HeldState::default() };
        let mut pull = 0.0;
        if using.kind == UseKind::Crossbow
            && let Some(crossbow) = self.slot_stack(slot).cloned()
            && charged(&crossbow).is_empty()
        {
            pull = using.ticks as f32 / self.charge_duration(&crossbow) as f32;
        }
        HeldState { using: true, use_ticks: using.ticks as f32, pull }
    }

    /// The use animation of the item in `slot`, if it is in use, with the
    /// ticks held through the frame.
    pub(super) fn use_pose(&mut self, slot: usize, partial: f32) -> Option<UsePose> {
        let using = self.use_in(slot)?.clone();
        let time_held = using.ticks as f32 + partial - 1.0;
        Some(match using.kind {
            UseKind::Bow => UsePose::Bow(time_held),
            UseKind::Crossbow => {
                let crossbow = self.slot_stack(slot).cloned()?;
                UsePose::Crossbow(time_held, self.charge_duration(&crossbow) as f32)
            }
            UseKind::Trident => UsePose::Trident(time_held),
            UseKind::Shield => UsePose::Block,
        })
    }

    /// The first-person pose of the hand holding `slot` when its item's
    /// use moves it (`submitArmWithItem`).
    /// A loaded crossbow in the main hand is held ready.
    pub(super) fn ranged_hand_pose(&mut self, slot: usize, swing: f32, equip: f32, left: bool, partial: f32) -> Option<glam::Mat4> {
        if let Some(pose) = self.use_pose(slot, partial) {
            return Some(crate::hand::use_pose(pose, equip, left));
        }
        // `isAutoSpinAttack`: both hands held out before the player.
        if self.riptide_spin.is_some() {
            return Some(crate::hand::spin_pose(equip, left));
        }
        let loaded = slot != OFFHAND && self.slot_stack(slot).is_some_and(|s| s.id == "minecraft:crossbow" && !charged(s).is_empty());
        loaded.then(|| crate::hand::charged_crossbow_pose(swing, equip, left))
    }

    /// `FirstPersonHandsAndItems.evaluateWhichHandsToRender`: with a bow or
    /// crossbow in either hand, drawing one shows only its hand, and a
    /// loaded crossbow in the main hand hides the offhand. The main hand,
    /// then the offhand.
    pub(super) fn hands_shown(&self) -> (bool, bool) {
        let loaded = |slot: usize| self.slot_stack(slot).is_some_and(|s| s.id == "minecraft:crossbow" && !charged(s).is_empty());
        let bow_like = |slot: usize| self.slot_stack(slot).is_some_and(|s| matches!(s.id.as_str(), "minecraft:bow" | "minecraft:crossbow"));
        let selected = self.entities.selected;
        if !bow_like(selected) && !bow_like(OFFHAND) {
            return (true, true);
        }
        // Eating is the main hand's.
        match (self.using.as_ref(), self.eating.as_ref()) {
            (Some(using), _) if matches!(using.kind, UseKind::Bow | UseKind::Crossbow) => (using.slot != OFFHAND, using.slot == OFFHAND),
            (Some(using), _) => (true, !(using.slot != OFFHAND && loaded(OFFHAND))),
            (None, Some(eating)) => (true, !(eating.slot != OFFHAND && loaded(OFFHAND))),
            (None, None) => (true, !loaded(selected)),
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
