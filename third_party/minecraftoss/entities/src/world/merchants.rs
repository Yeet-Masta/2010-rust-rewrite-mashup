//! Trading with villagers (26.3 `Villager.mobInteract`, `startTrading`,
//! `updateSpecialPrices`, `AbstractVillager.notifyTrade` and
//! `notifyTradeUpdated`, `Villager.rewardTradeXp` and
//! `increaseMerchantCareer`): a player's use opens the villager's offers
//! (a baby, or a villager with none, shakes its head) with prices set by
//! the player's reputation and Hero of the Village (and left so after, as
//! 26.3 no longer resets them when trading stops), each trade uses the offer, pays the villager experience (levelling it up
//! at once, with its next level's offers and regeneration) and drops an
//! experience orb, and the villager answers the payment slots with yes or
//! no. The screen itself is `crate::merchant`, which its owner runs: it
//! shows the villager's offers ([`EntityWorld::merchant_offers`]) and tells
//! it what the trader did ([`EntityWorld::merchant_notify`]).
use super::*;
use crate::merchant::{MerchantEvent, Offers};
use crate::trading::MerchantOffer;

/// `VillagerData.NEXT_LEVEL_XP_THRESHOLDS`.
const LEVEL_XP: [i32; 5] = [0, 10, 70, 150, 250];

/// `VillagerData.canLevelUp`.
fn can_level_up(level: i32) -> bool {
    (1..5).contains(&level)
}

/// `VillagerData.getMaxXpPerLevel`: the experience its next level needs.
pub fn max_xp_for_level(level: i32) -> i32 {
    if can_level_up(level) { LEVEL_XP[level as usize] } else { 0 }
}

/// `VillagerData.getMinXpPerLevel`.
pub fn min_xp_for_level(level: i32) -> i32 {
    if can_level_up(level) { LEVEL_XP[level as usize - 1] } else { 0 }
}

/// What a player's use of a villager did (`InteractionResult`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VillagerUse {
    /// Not taken (`PASS`): another use may follow.
    Pass,
    /// Taken (`SUCCESS`): the trading screen opened, or a baby shook its head.
    Success,
    /// Taken with nothing to show (`CONSUME`): no offers.
    Consume,
}

impl EntityWorld {
    /// An item's maximum stack size, from the trade data's item catalog.
    pub fn item_max_stack(&self, item: &str) -> i32 {
        self.trades.as_ref().map_or(64, |book| book.max_stack(item))
    }

    /// A player's Hero of the Village level (its amplifier), or none.
    pub fn set_player_hero(&mut self, player: u64, amplifier: Option<i32>) {
        match amplifier {
            Some(a) => {
                self.player_heroes.insert(player, a);
            }
            None => {
                self.player_heroes.remove(&player);
            }
        }
    }

    /// `makeSound` for a villager: its voice's pitch drawn from its random.
    fn villager_voice(&mut self, id: u64, event: &'static str) {
        let Some(entity) = self.villager_mut(id) else { return };
        let pitch = voice_pitch(&mut entity.random, entity.villager.age.baby());
        let at = entity.position();
        entity.voices.push((Voice::Event(event, 1.0, pitch), at));
    }

    /// `Villager.setUnhappy`: it shakes its head for 40 ticks and says no.
    fn villager_unhappy(&mut self, id: u64) {
        if let Some(entity) = self.villager_mut(id) {
            entity.unhappy = 40;
        }
        self.villager_voice(id, "entity.villager.no");
    }

    /// `Villager.mobInteract` by `player` with `held` in the hand used.
    pub fn villager_interact(&mut self, id: u64, player: u64, main_hand: bool, held: Option<&str>) -> VillagerUse {
        let Some(entity) = self.villagers.iter().find(|e| e.id == id) else { return VillagerUse::Pass };
        if held == Some("minecraft:villager_spawn_egg") || entity.villager.health <= 0.0 || entity.trading_player.is_some() || entity.sleeping.is_some() {
            return VillagerUse::Pass;
        }
        if entity.villager.age.baby() {
            self.villager_unhappy(id);
            return VillagerUse::Success;
        }
        let no_offers = self.villager_offers(id).is_none_or(<[MerchantOffer]>::is_empty);
        if main_hand && no_offers {
            self.villager_unhappy(id);
        }
        if no_offers {
            return VillagerUse::Consume;
        }
        // `startTrading`.
        self.villager_update_special_prices(id, player);
        if let Some(entity) = self.villager_mut(id) {
            entity.trading_player = Some(player);
        }
        VillagerUse::Success
    }

    /// `Villager.updateSpecialPrices`: each offer cheaper by the player's
    /// reputation times its price multiplier, and by Hero of the Village's
    /// share of its first cost (at least one).
    pub(super) fn villager_update_special_prices(&mut self, id: u64, player: u64) {
        let uuid = self.uuid_of(PLAYER_TARGET + player);
        let hero = self.player_heroes.get(&player).copied();
        let Some(entity) = self.villagers.iter_mut().find(|e| e.id == id) else { return };
        let reputation = entity.gossips.reputation(uuid);
        let Some(offers) = entity.offers.as_mut() else { return };
        offers.iter_mut().for_each(|o| o.special_price = 0);
        let hero = hero.map_or(0.0, |a| f64::from(0.3_f32 + 0.0625_f32 * a as f32));
        for offer in offers.iter_mut() {
            if reputation != 0 {
                let off = reputation as f32 * offer.price_multiplier;
                offer.special_price -= if off < (off as i32) as f32 { off as i32 - 1 } else { off as i32 };
            }
            if hero > 0.0 {
                let reduction = (hero * f64::from(offer.buy.count)).floor() as i32;
                offer.special_price -= reduction.max(1);
            }
        }
    }

    /// `AbstractVillager.notifyTradeUpdated`: yes or no, if it has not
    /// spoken in the last second.
    pub(super) fn villager_trade_updated(&mut self, id: u64, valid: bool) {
        let Some(entity) = self.villager_mut(id) else { return };
        if entity.ambient_sound_time > -80 + 20 {
            entity.ambient_sound_time = -80;
            self.villager_voice(id, if valid { "entity.villager.yes" } else { "entity.villager.no" });
        }
    }

    /// `AbstractVillager.notifyTrade` and `Villager.rewardTradeXp`: the offer
    /// used, the ambient clock reset, experience for the villager (and on
    /// reaching its level's threshold its next level at once, with that
    /// level's offers, prices renewed for its trader and ten seconds of
    /// regeneration), and an orb of 3 to 6 (8 more on a new level) unless
    /// the offer gives none.
    pub(super) fn villager_notify_trade(&mut self, id: u64, index: usize) {
        let book = self.trades.clone();
        let Some(entity) = self.villagers.iter_mut().find(|e| e.id == id) else { return };
        let Some(offer) = entity.offers.as_mut().and_then(|o| o.get_mut(index)) else { return };
        offer.uses += 1;
        let (xp, reward) = (offer.xp, offer.reward_exp);
        entity.ambient_sound_time = -80;
        let mut pop = 3 + entity.random.next_int(4) as i32;
        entity.villager.xp += xp;
        entity.last_traded_player = entity.trading_player;
        let level = entity.villager.level;
        if can_level_up(level) && entity.villager.xp >= max_xp_for_level(level) {
            // `increaseMerchantCareer`: the next level and its offers.
            entity.villager.level = level + 1;
            if let Some(book) = &book {
                let v = &entity.villager;
                let added = book.villager_offers(&v.kind, v.profession.id(), v.level, &mut self.trade_sequences);
                entity.offers.get_or_insert_with(Vec::new).extend(added);
            }
            let trader = entity.trading_player;
            entity.effects.add(crate::effects::EffectInstance::new(crate::effects::MobEffect::Regeneration, 200, 0));
            if let Some(player) = trader {
                self.villager_update_special_prices(id, player);
            }
            pop += 5;
        }
        if reward {
            if let Some(entity) = self.villagers.iter().find(|e| e.id == id) {
                let p = entity.villager.body.position;
                self.trade_experience.push((DVec3::new(p.x, p.y + 0.5, p.z), pop));
            }
        }
    }

    /// `AbstractVillager.stopTrading`: its trader gone. In 26.3 the special
    /// prices stay (and are saved) until the next `updateSpecialPrices`.
    pub(super) fn villager_stop_trading(&mut self, id: u64) {
        if let Some(entity) = self.villager_mut(id) {
            entity.trading_player = None;
        }
    }

    /// Experience orbs trades dropped since last taken: where, and worth
    /// how much.
    pub fn take_trade_experience(&mut self) -> Vec<(DVec3, i32)> {
        std::mem::take(&mut self.trade_experience)
    }

    /// The villager trading with `player` (`getTradingPlayer`), if any.
    pub fn trading_villager(&self, player: u64) -> Option<u64> {
        self.villagers.iter().find(|e| e.trading_player == Some(player)).map(|e| e.id)
    }

    /// What a villager shows its trader (`ClientboundMerchantOffersPacket`,
    /// as `openTradingScreen` and `updateSpecialPrices` send it): its
    /// offers (made if need be), level and experience; a villager shows
    /// its progress and restocks.
    pub fn merchant_offers(&mut self, id: u64) -> Option<Offers> {
        let offers = self.villager_offers(id)?.to_vec();
        let entity = self.villagers.iter().find(|e| e.id == id)?;
        let (level, xp) = (entity.villager.level, entity.villager.xp);
        let mut max_stacks: Vec<(String, i32)> = Vec::new();
        for offer in &offers {
            for item in std::iter::once(&offer.buy.id).chain(offer.buy_b.as_ref().map(|b| &b.id)).chain(std::iter::once(&offer.sell.id)) {
                let max = self.item_max_stack(item);
                if max != 64 && !max_stacks.iter().any(|(id, _)| id == item) {
                    max_stacks.push((item.clone(), max));
                }
            }
        }
        Some(Offers { offers, max_stacks, level, xp, show_progress: true, can_restock: true })
    }

    /// What a trading screen told its villager, in order: yes or no to its
    /// payments (`notifyTradeUpdated`) and its trades (`notifyTrade`).
    pub fn merchant_notify(&mut self, id: u64, events: &[MerchantEvent]) {
        for event in events {
            match *event {
                MerchantEvent::Updated(valid) => self.villager_trade_updated(id, valid),
                MerchantEvent::Trade(index) => self.villager_notify_trade(id, index),
            }
        }
    }

    /// `AbstractVillager.stillValid` for `player`'s screen: the villager
    /// still trades with them, lives, and is within their entity reach (3,
    /// or 5 in creative) and 4 more of `eyes`.
    pub fn merchant_still_valid(&self, id: u64, player: u64, eyes: DVec3, creative: bool) -> bool {
        let Some(e) = self.villagers.iter().find(|e| e.id == id) else { return false };
        if e.trading_player != Some(player) || e.villager.health <= 0.0 {
            return false;
        }
        let body = &e.villager.body;
        let half = f64::from(body.width) / 2.0;
        let (min, max) = (body.position - DVec3::new(half, 0.0, half), body.position + DVec3::new(half, f64::from(body.height), half));
        let outside = |v: f64, lo: f64, hi: f64| (lo - v).max(v - hi).max(0.0);
        let (dx, dy, dz) = (outside(eyes.x, min.x, max.x), outside(eyes.y, min.y, max.y), outside(eyes.z, min.z, max.z));
        let range = if creative { 5.0 } else { 3.0 } + 4.0;
        dx * dx + dy * dy + dz * dz < range * range
    }

    /// The trading screen closed (`MerchantMenu.removed`'s
    /// `setTradingPlayer(null)`), which stops a villager that was trading.
    pub fn merchant_stop(&mut self, id: u64) {
        if self.villagers.iter().any(|e| e.id == id && e.trading_player.is_some()) {
            self.villager_stop_trading(id);
        }
    }
}
