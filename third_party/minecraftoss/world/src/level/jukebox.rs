//! Jukeboxes on the server level (26.3 `JukeboxBlock`, `JukeboxBlockEntity`,
//! `JukeboxSongPlayer`, `JukeboxPlayable`):
//! - A disc put in an empty jukebox plays its song (level event 1010 with
//!   the song's number), marked `has_record`. Each second it plays, it
//!   tells the mobs about it (`JUKEBOX_PLAY`). When its length has run (and
//!   a second more) it stops (level event 1011, `JUKEBOX_STOP_PLAY`).
//! - Used again, or broken, it pops the disc out on top.
//! - A playing jukebox powers redstone at 15, and comparators read its
//!   disc's `comparator_output`.
use super::container::Stack;
use super::Level;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockPos, BlockStateId};
use std::collections::BTreeMap;

/// A `jukebox_song` entry.
#[derive(Clone, Debug)]
pub struct JukeboxSong {
    pub id: String,
    pub sound_event: String,
    pub length_ticks: i64,
    pub comparator_output: i32,
    /// Its name, for "Now Playing".
    pub description: serde_json::Value,
}

/// The songs, in registry order (sorted by identifier): a song's place is
/// the number level event 1010 carries.
pub fn load_songs(registries: &minecraftoss_core::Registries) -> Vec<JukeboxSong> {
    let Ok(ids) = registries.datapack.list("jukebox_song") else { return Vec::new() };
    ids.into_iter()
        .filter_map(|id| {
            let json = registries.datapack.read_json("jukebox_song", &id).ok()?;
            Some(JukeboxSong {
                id: id.to_string(),
                sound_event: json.get("sound_event")?.as_str()?.to_owned(),
                // `lengthInTicks`: `Mth.ceil(lengthInSeconds * 20)`.
                length_ticks: (json.get("length_in_seconds")?.as_f64()? * 20.0).ceil() as i64,
                comparator_output: json.get("comparator_output").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32,
                description: json.get("description").cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// A jukebox's song as it plays (`JukeboxSongPlayer`).
#[derive(Clone, Copy, Debug)]
pub(super) struct SongPlayer {
    song: usize,
    ticks: i64,
}

/// What a jukebox told the mobs: where, and whether it plays or stopped
/// (`JUKEBOX_PLAY`, `JUKEBOX_STOP_PLAY`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JukeboxEvent {
    pub pos: BlockPos,
    pub playing: bool,
}

impl Level<'_> {
    /// `JukeboxSong.fromStack`: its `jukebox_playable` component, else a
    /// music disc's own song (`music_disc_<song>`).
    fn song_of(&self, stack: &Stack) -> Option<usize> {
        let named = stack.components.as_ref().and_then(|c| c.get("minecraft:jukebox_playable")).and_then(Tag::as_str).map(str::to_owned);
        let id = named.or_else(|| stack.id.strip_prefix("minecraft:music_disc_").map(|s| format!("minecraft:{s}")))?;
        self.songs.iter().position(|s| s.id == id)
    }

    fn record_item(&self, pos: BlockPos) -> Option<Stack> {
        let tag = self.block_entity(pos)?.get("RecordItem")?;
        let id = tag.get("id")?.as_str()?.to_owned();
        let count = tag.get("count").and_then(Tag::as_i64).unwrap_or(1) as i32;
        let components = tag.get("components").cloned();
        Some(Stack { id, count, components })
    }

    fn set_record_item(&mut self, pos: BlockPos, stack: Option<&Stack>) {
        let Some(Tag::Compound(map)) = self.block_entity_mut(pos) else { return };
        match stack {
            Some(stack) => {
                let mut item = BTreeMap::from([("id".to_owned(), Tag::String(stack.id.clone())), ("count".to_owned(), Tag::Int(stack.count))]);
                if let Some(c) = &stack.components {
                    item.insert("components".to_owned(), c.clone());
                }
                map.insert("RecordItem".to_owned(), Tag::Compound(item));
            }
            None => {
                map.remove("RecordItem");
                map.remove("ticks_since_song_started");
            }
        }
    }

    /// The song player, read from a loaded jukebox the first time
    /// (`setSongWithoutPlaying` with `ticks_since_song_started`).
    fn song_player(&mut self, pos: BlockPos) -> Option<SongPlayer> {
        if let Some(p) = self.jukeboxes.get(&pos) {
            return *p;
        }
        let ticks = self.block_entity(pos).and_then(|t| t.get("ticks_since_song_started")).and_then(Tag::as_i64);
        let player = match (ticks, self.record_item(pos).and_then(|s| self.song_of(&s))) {
            (Some(ticks), Some(song)) if ticks < self.songs[song].length_ticks + 20 => Some(SongPlayer { song, ticks }),
            _ => None,
        };
        self.jukeboxes.insert(pos, player);
        player
    }

    /// Whether the jukebox at `pos` plays (`getSongPlayer().isPlaying()`).
    pub(super) fn jukebox_playing(&self, pos: BlockPos) -> bool {
        self.jukeboxes.get(&pos).is_some_and(Option::is_some)
    }

    /// `getComparatorOutput`: its disc's song's.
    pub(super) fn jukebox_signal(&self, pos: BlockPos) -> i32 {
        self.record_item(pos).and_then(|s| self.song_of(&s)).map_or(0, |song| self.songs[song].comparator_output)
    }

    /// `onSongChanged`: the neighbours and comparators hear of it.
    fn jukebox_changed(&mut self, pos: BlockPos) {
        let block = self.block_id(self.block(pos));
        self.update_neighbors_at(pos, block);
        self.update_neighbour_for_output_signal(pos, block);
    }

    fn jukebox_stop(&mut self, pos: BlockPos) {
        if let Some(Some(_)) = self.jukeboxes.insert(pos, None) {
            self.jukebox_events.push(JukeboxEvent { pos, playing: false });
            self.level_event(1011, pos, 0);
            self.jukebox_changed(pos);
        }
    }

    /// `JukeboxPlayable.tryInsertIntoJukebox`: a disc into an empty
    /// jukebox, which plays it. Whether it went in.
    pub fn jukebox_insert(&mut self, pos: BlockPos, stack: &Stack) -> bool {
        let state = self.block(pos);
        if !self.is_a(state, "JukeboxBlock") || self.flag(state, "has_record") {
            return false;
        }
        let Some(song) = self.song_of(stack) else { return false };
        let mut one = stack.clone();
        one.count = 1;
        self.set_record_item(pos, Some(&one));
        // `notifyItemChangedInJukebox` (flag 2), then `play`.
        let next = self.with_bool(state, "has_record", true);
        self.set_block(pos, next, super::update::CLIENTS, super::update::LIMIT);
        self.jukeboxes.insert(pos, Some(SongPlayer { song, ticks: 0 }));
        self.level_event(1010, pos, song as i32);
        self.jukebox_changed(pos);
        true
    }

    /// `popOutTheItem`: the disc out on top, a little to one side, and the
    /// song stops. Whether there was one.
    pub fn jukebox_pop(&mut self, pos: BlockPos) -> bool {
        let Some(stack) = self.record_item(pos) else { return false };
        self.set_record_item(pos, None);
        let state = self.block(pos);
        if self.is_a(state, "JukeboxBlock") {
            let next = self.with_bool(state, "has_record", false);
            self.set_block(pos, next, super::update::CLIENTS, super::update::LIMIT);
        }
        self.jukebox_stop(pos);
        // `atLowerCornerWithOffset(0.5, 1.01, 0.5).offsetRandomXZ(0.7)`.
        let dx = f64::from(self.random.next_f32() - 0.5) * 0.7;
        let dz = f64::from(self.random.next_f32() - 0.5) * 0.7;
        let at = [f64::from(pos.x) + 0.5 + dx, f64::from(pos.y) + 1.01, f64::from(pos.z) + 0.5 + dz];
        self.spawn_item_with(at, stack, [0.0, 0.0, 0.0], 10, 0);
        true
    }

    /// `JukeboxBlockEntity.tick`.
    pub(super) fn jukebox_tick(&mut self, pos: BlockPos) {
        let Some(mut player) = self.song_player(pos) else { return };
        if player.ticks >= self.songs[player.song].length_ticks + 20 {
            self.jukebox_stop(pos);
            return;
        }
        if player.ticks % 20 == 0 {
            self.jukebox_events.push(JukeboxEvent { pos, playing: true });
            if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
                map.insert("ticks_since_song_started".to_owned(), Tag::Long(player.ticks));
            }
        }
        player.ticks += 1;
        self.jukeboxes.insert(pos, Some(player));
    }

    /// `preRemoveSideEffects` and `setRemoved`: the disc pops out and the
    /// song stops.
    pub(super) fn jukebox_removed(&mut self, pos: BlockPos, old: BlockStateId) {
        if !self.is_a(old, "JukeboxBlock") {
            return;
        }
        if let Some(stack) = self.record_item(pos) {
            self.set_record_item(pos, None);
            let dx = f64::from(self.random.next_f32() - 0.5) * 0.7;
            let dz = f64::from(self.random.next_f32() - 0.5) * 0.7;
            let at = [f64::from(pos.x) + 0.5 + dx, f64::from(pos.y) + 1.01, f64::from(pos.z) + 0.5 + dz];
            self.spawn_item_with(at, stack, [0.0, 0.0, 0.0], 10, 0);
        }
        if let Some(Some(_)) = self.jukeboxes.remove(&pos) {
            self.jukebox_events.push(JukeboxEvent { pos, playing: false });
        }
        self.level_event(1011, pos, 0);
    }
}
