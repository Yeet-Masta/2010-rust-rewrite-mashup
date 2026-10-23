//! Names given to mobs (26.3 `Entity.setCustomName`, `NameTagItem`,
//! `Mob.shouldShowName`).
//!
//! A name tag that was given a name at an anvil names the mob it is used
//! on, which then never despawns. A named mob shows its name when the
//! crosshair is on it.
use super::*;

impl EntityWorld {
    /// The name a mob was given.
    pub fn custom_name(&self, id: u64) -> Option<&str> {
        self.custom_names.get(&id).map(String::as_str)
    }

    /// Gives a mob its name, or takes it away (a saved mob's `CustomName`).
    pub fn set_custom_name(&mut self, id: u64, name: Option<String>) {
        match name.filter(|n| !n.is_empty()) {
            Some(name) => {
                self.custom_names.insert(id, name);
            }
            None => {
                self.custom_names.remove(&id);
            }
        }
    }

    /// `NameTagItem.interactLivingEntity`: the name goes on a living mob,
    /// which is kept from then on (`setPersistenceRequired`). Whether it
    /// took (the tag is spent).
    pub fn name_tag(&mut self, hit: MobHit, name: &str) -> bool {
        if name.is_empty() || !self.living_hits().iter().any(|(h, _)| *h == hit) {
            return false;
        }
        let id = hit.id();
        self.custom_names.insert(id, name.to_owned());
        match hit {
            MobHit::Bat(id) => self.bats.iter_mut().filter(|e| e.id == id).for_each(|e| e.bat.persistence_required = true),
            MobHit::Zombie(id) => self.zombies.iter_mut().filter(|e| e.id == id).for_each(|e| e.zombie.persistence_required = true),
            MobHit::Skeleton(id) => self.skeletons.iter_mut().filter(|e| e.id == id).for_each(|e| e.skeleton.persistence_required = true),
            MobHit::Creeper(id) => self.creepers.iter_mut().filter(|e| e.id == id).for_each(|e| e.creeper.persistence_required = true),
            MobHit::Spider(id) => self.spiders.iter_mut().filter(|e| e.id == id).for_each(|e| e.spider.persistence_required = true),
            MobHit::Slime(id) => self.slimes.iter_mut().filter(|e| e.id == id).for_each(|e| e.slime.persistence_required = true),
            MobHit::Enderman(id) => self.endermen.iter_mut().filter(|e| e.id == id).for_each(|e| e.enderman.persistence_required = true),
            MobHit::Witch(id) => self.witches.iter_mut().filter(|e| e.id == id).for_each(|e| e.witch.persistence_required = true),
            MobHit::IronGolem(id) => self.iron_golems.iter_mut().filter(|e| e.id == id).for_each(|e| e.golem.persistence_required = true),
            MobHit::Wolf(id) => self.wolves.iter_mut().filter(|e| e.id == id).for_each(|e| e.wolf.persistence_required = true),
            MobHit::Villager(id) => self.villagers.iter_mut().filter(|e| e.id == id).for_each(|e| e.villager.persistence_required = true),
            MobHit::Cow(id) | MobHit::Mooshroom(id) => self.cows.iter_mut().filter(|e| e.id == id).for_each(|e| e.cow.persistence_required = true),
            MobHit::Sheep(id) => self.sheep.iter_mut().filter(|e| e.id == id).for_each(|e| e.sheep.persistence_required = true),
            MobHit::Pig(id) => self.pigs.iter_mut().filter(|e| e.id == id).for_each(|e| e.pig.persistence_required = true),
            MobHit::Chicken(id) => self.chickens.iter_mut().filter(|e| e.id == id).for_each(|e| e.chicken.persistence_required = true),
            MobHit::Mob(id) => self.mobs.iter_mut().filter(|e| e.id == id).for_each(|e| e.persistence_required = true),
        }
        true
    }

    /// `Mob.shouldShowName` for the mob the crosshair is on: its name, and
    /// where it is drawn (`getNameTagAttachment`, half a block over its
    /// head).
    pub fn name_plate(&self, eye: DVec3, look: DVec3, reach: f64) -> Option<(String, DVec3)> {
        let (hit, _) = self.mob_on_ray(eye, look, reach)?;
        let name = self.custom_names.get(&hit.id())?;
        let (_, body) = self.living_hits().into_iter().find(|(h, _)| *h == hit)?;
        Some((name.clone(), body.position + DVec3::new(0.0, f64::from(body.height) + 0.5, 0.0)))
    }

    /// Names of mobs no longer there are let go.
    pub(super) fn forget_names(&mut self) {
        if self.custom_names.is_empty() {
            return;
        }
        let alive: std::collections::HashSet<u64> = self.order.iter().map(|k| k.id()).collect();
        self.custom_names.retain(|id, _| alive.contains(id));
    }
}
