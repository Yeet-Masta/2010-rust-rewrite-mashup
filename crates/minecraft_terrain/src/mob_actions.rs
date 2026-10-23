//! A player's hit on a mob and use of a held item on one, as the server
//! runs them (`Player.attack`'s basic hit, `Mob.interact` and the species'
//! `mobInteract`). The caller presents the outcome: sounds, drops and
//! inventory changes. Both the authored scene's local entity world and the
//! integrated server's run through here.

use glam::DVec3;
use minecraftoss_entities::animal::AnimalEvent;
use minecraftoss_entities::cow::{CowEvent, InteractionResult};
use minecraftoss_entities::loot::{EntityLootBook, ShearingLootBook};
use minecraftoss_entities::mooshroom::flower_effects;
use minecraftoss_entities::sheep::ShearsResult;
use minecraftoss_entities::world::{EntityWorld, MobHit, PlayerAttack};
use minecraftoss_player::inventory::{Inventory, ItemStack};

/// A sound a mob action plays, with its sound category option.
#[derive(Clone, Debug, PartialEq)]
pub struct MobSound {
    pub event: String,
    pub position: DVec3,
    pub volume: f32,
    pub pitch: f32,
    pub category: &'static str,
}

/// What a hit or use did.
#[derive(Clone, Debug, Default)]
pub struct MobOutcome {
    /// The action was taken (the click goes no further).
    pub handled: bool,
    pub sounds: Vec<MobSound>,
    /// Items the mob dropped, where they appear.
    pub drops: Vec<(ItemStack, DVec3)>,
    /// Experience killed mobs left, where they died.
    pub experience: Vec<(DVec3, i32)>,
    /// The acting player's inventory changed.
    pub inventory_changed: bool,
    /// The hit's target and what it did, for the client's particles.
    pub attack: Option<(u64, minecraftoss_entities::world::AttackResult)>,
    /// `broadcastEntityEvent`s the action made: entity, event (18 hearts).
    pub events: Vec<(u64, u8)>,
}

impl MobOutcome {
    fn sound(&mut self, event: impl Into<String>, position: DVec3, volume: f32, category: &'static str) {
        self.sounds.push(MobSound { event: event.into(), position, volume, pitch: 1.0, category });
    }
}

/// The player acting on a mob.
pub struct Actor<'a> {
    pub inventory: &'a mut Inventory,
    pub selected: usize,
    /// Creative mode: held items are not used up or worn.
    pub infinite: bool,
    pub entity_loot: Option<&'a mut EntityLootBook>,
    pub shearing_loot: Option<&'a mut ShearingLootBook>,
}


/// What the mobs that died since the last call leave where each died: their
/// drops ([`EntityLootBook::death_drops`]; without loot tables nothing
/// drops) and their experience.
pub fn death_remains(world: &mut EntityWorld, loot: Option<&mut EntityLootBook>) -> (Vec<(ItemStack, DVec3)>, Vec<(DVec3, i32)>) {
    let deaths = world.take_deaths();
    let experience = deaths.iter().filter(|d| d.experience > 0).map(|d| (d.position, d.experience)).collect();
    (loot.map(|book| book.death_drops(deaths)).unwrap_or_default(), experience)
}

/// Whether a hit can land on the mob `id` (a creeper that blew up is gone).
fn hittable(world: &EntityWorld, id: u64) -> bool {
    world.mob_ids().any(|mob| mob == id) && !world.creepers().iter().any(|e| e.id == id && e.creeper.exploded)
}

/// A player's hit on a mob (`Player.attack`): the entity world applies it
/// ([`EntityWorld::player_attack`]); this presents it, with the player's
/// attack sounds, each hurt mob's sound and the loot of those it killed.
pub fn attack(world: &mut EntityWorld, hit: MobHit, actor: &mut Actor, attack: &PlayerAttack) -> MobOutcome {
    let id = hit.id();
    if !hittable(world, id) {
        return MobOutcome::default();
    }
    let result = world.player_attack(attack, id);
    let mut outcome = MobOutcome { handled: true, ..MobOutcome::default() };
    let at = attack.position;
    if result.knockback {
        outcome.sound("entity.player.attack.knockback", at, 1.0, "players_volume");
    }
    if !result.hurt {
        outcome.sound("entity.player.attack.nodamage", at, 1.0, "players_volume");
        outcome.attack = Some((id, result));
        return outcome;
    }
    (outcome.drops, outcome.experience) = death_remains(world, actor.entity_loot.as_deref_mut());
    if result.sweep {
        outcome.sound("entity.player.attack.sweep", at, 1.0, "players_volume");
    }
    if result.critical {
        outcome.sound("entity.player.attack.crit", at, 1.0, "players_volume");
    } else if !result.sweep {
        let event = if result.full_strength { "entity.player.attack.strong" } else { "entity.player.attack.weak" };
        outcome.sound(event, at, 1.0, "players_volume");
    }
    // `MaceItem.hurtEnemy`'s smash, where the player is.
    if let Some(event) = result.smash {
        outcome.sound(event, at, 1.0, "players_volume");
    }
    outcome.sounds.extend(world.take_sounds().into_iter().map(|s| MobSound { event: s.event, position: s.position, volume: s.volume, pitch: s.pitch, category: s.category }));
    outcome.attack = Some((id, result));
    outcome
}

/// The golden dandelion's age lock sound.
fn age_lock_sound(outcome: &mut MobOutcome, locked: Option<bool>, position: DVec3) {
    if let Some(locked) = locked {
        let event = if locked { "item.golden_dandelion.use" } else { "item.golden_dandelion.unuse" };
        outcome.sound(event, position, 1.0, "players_volume");
    }
}

fn animal_lock(events: &[AnimalEvent]) -> Option<bool> {
    events.iter().find_map(|event| match event {
        AnimalEvent::AgeLock => Some(true),
        AnimalEvent::AgeUnlock => Some(false),
        _ => None,
    })
}

fn cow_lock(events: &[CowEvent]) -> Option<bool> {
    events.iter().find_map(|event| match event {
        CowEvent::AgeLock => Some(true),
        CowEvent::AgeUnlock => Some(false),
        _ => None,
    })
}

/// Using the held item on a mob. A pass (nothing happens) leaves the
/// outcome unhandled, so the click may use the item or a block instead.
pub fn interact(world: &mut EntityWorld, hit: MobHit, actor: &mut Actor) -> MobOutcome {
    let selected = actor.selected;
    let infinite = actor.infinite;
    let recipes = actor.inventory.recipes.clone();
    let held = actor.inventory.slots.get(selected).and_then(Option::as_ref).map(|stack| stack.id.clone());
    let mut outcome = MobOutcome::default();
    // `Entity.interact`'s leads come before the mob's own use.
    let sneaking = world.player_sneaking(0);
    if let Some(used) = world.leash_interact(hit.id(), 0, held.as_deref(), sneaking, infinite) {
        use minecraftoss_entities::world::LeashUse;
        match used {
            LeashUse::Tied { spends: true } if !infinite => minecraftoss_entities::animal::consume_one(actor.inventory, selected),
            LeashUse::Sheared if !infinite => {
                actor.inventory.wear_tool(selected, 1);
            }
            _ => {}
        }
        outcome.inventory_changed = true;
        outcome.handled = true;
        outcome.sounds.extend(world.take_sounds().into_iter().map(|s| MobSound { event: s.event, position: s.position, volume: s.volume, pitch: s.pitch, category: s.category }));
        return outcome;
    }
    // `CopperGolem.mobInteract`: its load to an empty hand, wax, or a
    // scrape of wax or weathering.
    if let MobHit::Mob(id) = hit {
        use minecraftoss_entities::world::CopperUse;
        let player_at = world.player_position(0).unwrap_or_default();
        match world.copper_golem_interact(id, held.as_deref(), player_at) {
            Some(CopperUse::Spent) => {
                if !infinite {
                    minecraftoss_entities::animal::consume_one(actor.inventory, selected);
                }
                outcome.inventory_changed = true;
                outcome.handled = true;
                return outcome;
            }
            Some(CopperUse::Worn) => {
                if !infinite {
                    actor.inventory.wear_tool(selected, 1);
                }
                outcome.inventory_changed = true;
                outcome.handled = true;
                return outcome;
            }
            Some(CopperUse::Took) => {
                outcome.handled = true;
                return outcome;
            }
            None => {}
        }
    }
    // `Allay.mobInteract`: one of the held item given, or its item handed
    // back to an empty hand.
    if let MobHit::Mob(id) = hit {
        use minecraftoss_entities::world::AllayUse;
        match world.allay_interact(id, 0, held.as_deref()) {
            Some(AllayUse::Given | AllayUse::Duplicated) => {
                if !infinite {
                    minecraftoss_entities::animal::consume_one(actor.inventory, selected);
                }
                outcome.inventory_changed = true;
                outcome.handled = true;
                return outcome;
            }
            Some(AllayUse::Taken(item)) => {
                if let Some(left) = actor.inventory.add_item(ItemStack::new(item, 1), selected) {
                    if let Some(at) = world.mobs().iter().find(|e| e.id == id).map(|e| e.body.position) {
                        outcome.drops.push((left, at));
                    }
                }
                outcome.inventory_changed = true;
                outcome.handled = true;
                return outcome;
            }
            None => {}
        }
    }
    // `Panda.mobInteract`: up off its back, or sat down to eat the bamboo
    // it is given.
    if let MobHit::Mob(id) = hit {
        if let Some(spent) = world.panda_interact(id, held.as_deref()) {
            if spent && !infinite {
                minecraftoss_entities::animal::consume_one(actor.inventory, selected);
            }
            outcome.inventory_changed = true;
            outcome.handled = true;
            return outcome;
        }
    }
    match hit {
        MobHit::Cow(id) => {
            let Some(entity) = world.cow_mut(id) else { return outcome };
            let position = entity.cow.body.position;
            // Horses and donkeys have their own click (no milk).
            if let Some((result, love)) = entity.interact_horse(actor.inventory, selected, infinite, |item| recipes.item_in_tag("minecraft:horse_food", item)) {
                if result == InteractionResult::Pass {
                    return outcome;
                }
                if love {
                    outcome.events.push((id, 18));
                }
                // `doPlayerRide` (a sneaking player opens a tame one's
                // inventory instead, which is not simulated).
                let tame = entity.horse.as_ref().is_some_and(|h| h.tame);
                if std::mem::take(&mut entity.mount_request) && !(sneaking && tame) {
                    world.mount_player(MobHit::Cow(id));
                }
                outcome.inventory_changed = true;
                outcome.handled = true;
                return outcome;
            }
            let (result, events) = entity.cow.interact(actor.inventory, selected, infinite, |item| recipes.item_in_tag("minecraft:cow_food", item), false);
            if result == InteractionResult::Pass {
                return outcome;
            }
            if events.contains(&CowEvent::Hearts) {
                outcome.events.push((id, 18));
            }
            if events.contains(&CowEvent::Milk) {
                outcome.sound("entity.cow.milk", position, 1.0, "friendly_volume");
            }
        }
        MobHit::Sheep(id) if held.as_deref() == Some("minecraft:shears") => {
            let Some(loot) = actor.shearing_loot.as_deref_mut() else { return outcome };
            let Some(entity) = world.sheep_mut(id) else { return outcome };
            let position = entity.body.position;
            let Some(result) = entity.sheep.use_shears(loot) else { return outcome };
            if let ShearsResult::Sheared { drops, tool_damage } = result {
                outcome.sound("entity.sheep.shear", position, 1.0, "friendly_volume");
                for stack in drops {
                    for _ in 0..stack.count {
                        outcome.drops.push((ItemStack { count: 1, ..stack.clone() }, position + DVec3::Y));
                    }
                }
                if !infinite {
                    actor.inventory.wear_tool(selected, tool_damage);
                }
            }
        }
        MobHit::Sheep(id) => {
            let Some(entity) = world.sheep_mut(id) else { return outcome };
            let (result, events) = entity.sheep.interact_food(actor.inventory, selected, infinite, |item| recipes.item_in_tag("minecraft:sheep_food", item), false);
            entity.sync_dimensions();
            let position = entity.body.position;
            if result == InteractionResult::Pass {
                return outcome;
            }
            if events.contains(&AnimalEvent::Hearts) {
                outcome.events.push((id, 18));
            }
            age_lock_sound(&mut outcome, animal_lock(&events), position);
        }
        MobHit::Pig(id) => {
            let Some(entity) = world.pig_mut(id) else { return outcome };
            // `Pig.mobInteract`: a saddled pig takes on a player not
            // feeding it nor sneaking.
            let food = held.as_deref().is_some_and(|item| recipes.item_in_tag("minecraft:pig_food", item));
            if entity.pig.saddled && !food && !sneaking {
                world.mount_player(MobHit::Pig(id));
                outcome.handled = true;
                return outcome;
            }
            let Some(entity) = world.pig_mut(id) else { return outcome };
            let was_saddled = entity.pig.saddled;
            let (result, events) = if held.as_deref() == Some("minecraft:saddle") {
                (entity.pig.interact_saddle(actor.inventory, selected, infinite), Vec::new())
            } else {
                entity.pig.interact_food(actor.inventory, selected, infinite, |item| recipes.item_in_tag("minecraft:pig_food", item), false)
            };
            entity.pig.sync_dimensions();
            let position = entity.pig.body.position;
            if result == InteractionResult::Pass {
                return outcome;
            }
            if !was_saddled && entity.pig.saddled {
                outcome.sound("entity.pig.saddle", position, 1.0, "friendly_volume");
            }
            if events.contains(&AnimalEvent::Hearts) {
                outcome.events.push((id, 18));
            }
            age_lock_sound(&mut outcome, animal_lock(&events), position);
        }
        MobHit::Chicken(id) => {
            let Some(entity) = world.chicken_mut(id) else { return outcome };
            let (result, events) = entity.chicken.interact_food(actor.inventory, selected, infinite, |item| recipes.item_in_tag("minecraft:chicken_food", item), false);
            entity.chicken.sync_dimensions();
            let position = entity.chicken.body.position;
            if result == InteractionResult::Pass {
                return outcome;
            }
            if events.contains(&AnimalEvent::Hearts) {
                outcome.events.push((id, 18));
            }
            age_lock_sound(&mut outcome, animal_lock(&events), position);
        }
        MobHit::Mooshroom(id) if held.as_deref() == Some("minecraft:shears") => {
            let Some(position) = world.mooshroom_mut(id).map(|entity| entity.cow.body.position) else { return outcome };
            let Some(shearing) = world.shear_mooshroom(id) else { return outcome };
            outcome.sound("entity.mooshroom.shear", position, 1.0, "players_volume");
            for _ in 0..shearing.drop_count {
                outcome.drops.push((ItemStack::new(shearing.drop_item, 1), position + DVec3::Y));
            }
            if !infinite {
                actor.inventory.wear_tool(selected, shearing.tool_damage);
            }
        }
        MobHit::Mooshroom(id) => {
            let Some(entity) = world.mooshroom_mut(id) else { return outcome };
            let Some(state) = entity.mooshroom.as_mut() else { return outcome };
            let was_charged = state.stew_effects.is_some();
            let (result, events) = state.interact(&mut entity.cow, actor.inventory, selected, infinite, |item| recipes.item_in_tag("minecraft:cow_food", item), flower_effects);
            let charged = !was_charged && state.stew_effects.is_some();
            entity.cow.sync_dimensions();
            let position = entity.cow.body.position;
            if result == InteractionResult::Pass {
                return outcome;
            }
            if held.as_deref() == Some("minecraft:bowl") {
                let event = if was_charged { "entity.mooshroom.suspicious_milk" } else { "entity.mooshroom.milk" };
                outcome.sound(event, position, 1.0, "friendly_volume");
            } else if charged {
                outcome.sound("entity.mooshroom.eat", position, 2.0, "friendly_volume");
            }
            if events.contains(&CowEvent::Hearts) {
                outcome.events.push((id, 18));
            }
            age_lock_sound(&mut outcome, cow_lock(&events), position);
        }
        MobHit::Villager(id) => {
            // `Villager.mobInteract`: trading opens, or it shakes its head.
            if world.villager_interact(id, 0, true, held.as_deref()) == minecraftoss_entities::world::VillagerUse::Pass {
                return outcome;
            }
            outcome.handled = true;
            return outcome;
        }
        MobHit::Wolf(id) => {
            // `Wolf.mobInteract`: taming with bones, feeding, collar dyes,
            // love, and its owner's orders to sit or stand.
            if world.wolf_interact(id, 0, actor.inventory, selected, infinite) == InteractionResult::Pass {
                return outcome;
            }
        }
        MobHit::IronGolem(id) if held.as_deref() == Some("minecraft:iron_ingot") => {
            let Some(entity) = world.iron_golem_mut(id) else { return outcome };
            if !entity.repair_with_ingot() {
                return outcome;
            }
            if !infinite {
                minecraftoss_entities::animal::consume_one(actor.inventory, selected);
            }
        }
        // The generic kinds' `Animal.mobInteract`: feeding and breeding.
        MobHit::Mob(id) if held.as_deref() == Some("minecraft:water_bucket") && world.mobs().iter().any(|e| e.id == id && e.health > 0.0 && crate::mob_bucket::bucket_for(e.kind).is_some()) => {
            // `Bucketable.bucketMobPickup`: into its bucket, with what
            // `saveToBucketTag` keeps, and gone (`discard`).
            let Some(mob) = world.discard_mob(id) else { return outcome };
            let Some((item, sound)) = crate::mob_bucket::bucket_for(mob.kind) else { return outcome };
            outcome.sound(sound, mob.body.position, 1.0, "neutral_volume");
            let bucket = ItemStack { max: 1, components: Some(crate::mob_bucket::bucket_components(&mob)), ..ItemStack::new(item, 1) };
            // `ItemUtils.createFilledResult` without the creative limit.
            if !infinite {
                minecraftoss_entities::animal::consume_one(actor.inventory, selected);
            }
            if actor.inventory.slots[selected].is_none() {
                actor.inventory.slots[selected] = Some(bucket);
            } else if let Some(rest) = actor.inventory.add_item(bucket, selected) {
                outcome.drops.push((rest, mob.body.position));
            }
        }
        // `Goat.mobInteract`: a bucket filled with milk.
        MobHit::Mob(id) if held.as_deref() == Some("minecraft:bucket") && world.milk_goat(id) => {
            let milk = ItemStack::new("minecraft:milk_bucket", 1);
            let at = world.mobs().iter().find(|e| e.id == id).map_or(glam::DVec3::ZERO, |e| e.body.position);
            if !infinite {
                minecraftoss_entities::animal::consume_one(actor.inventory, selected);
            }
            if actor.inventory.slots[selected].is_none() {
                actor.inventory.slots[selected] = Some(milk);
            } else if let Some(rest) = actor.inventory.add_item(milk, selected) {
                outcome.drops.push((rest, at));
            }
            outcome.inventory_changed = true;
            outcome.handled = true;
            outcome.sounds.extend(world.take_sounds().into_iter().map(|s| MobSound { event: s.event, position: s.position, volume: s.volume, pitch: s.pitch, category: s.category }));
        }
        // `Armadillo.mobInteract`: a brush takes off a scute, and wears 16.
        MobHit::Mob(id) if held.as_deref() == Some("minecraft:brush") && world.brush_armadillo(id) => {
            if !infinite {
                actor.inventory.wear_tool(selected, 16);
            }
            outcome.inventory_changed = true;
            outcome.handled = true;
            return outcome;
        }
        // `WanderingTrader.mobInteract`: its trades open.
        MobHit::Mob(id) if world.trader_interact(id, 0, held.as_deref()) != minecraftoss_entities::world::VillagerUse::Pass => {
            outcome.handled = true;
            return outcome;
        }
        MobHit::Mob(id) if held.as_deref() == Some("minecraft:gold_ingot") && world.piglin_take_gold(id, "minecraft:gold_ingot") => {
            // `PiglinAi.mobInteract`: one ingot goes to be admired.
            if !infinite {
                minecraftoss_entities::animal::consume_one(actor.inventory, selected);
            }
        }
        MobHit::Mob(id) => {
            // Taming and orders first (`TamableAnimal`s, the ocelot's trust).
            if let Some(events) = world.tame_mob(id, 0, actor.inventory, selected, infinite) {
                outcome.inventory_changed = true;
                outcome.events.extend(events);
                outcome.handled = true;
                outcome.sounds.extend(world.take_sounds().into_iter().map(|s| MobSound { event: s.event, position: s.position, volume: s.volume, pitch: s.pitch, category: s.category }));
                return outcome;
            }
            let Some((used, events)) = world.feed_mob(id, actor.inventory, selected, infinite) else { return outcome };
            if !used {
                return outcome;
            }
            outcome.inventory_changed = true;
            if events.contains(&AnimalEvent::Hearts) {
                outcome.events.push((id, 18));
            }
            let position = world.mobs().iter().find(|e| e.id == id).map_or(DVec3::ZERO, |e| e.body.position);
            age_lock_sound(&mut outcome, animal_lock(&events), position);
        }
        _ => return outcome,
    }
    outcome.handled = true;
    outcome.inventory_changed = true;
    outcome
}
