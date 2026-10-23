//! Minecraft's sound events, resolved as the game resolves them: the pack's
//! `sounds.json` names each event's weighted variants (files, or other
//! events) with their own volume and pitch. Files are decoded once and played
//! with vanilla's linear falloff (sixteen blocks, further for louder sounds),
//! panned by where they are around the listener.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{DVec3, Vec3};
use minecraft_terrain::pack::{PackStack, ResourceId};

struct Entry {
    name: String,
    volume: f32,
    pitch: f32,
    weight: u32,
    /// `name` is another event rather than a file.
    event: bool,
}

/// A decoded sound file, mixed down to one channel.
struct Clip {
    rate: u32,
    samples: Arc<[f32]>,
}

pub struct Sounds {
    events: HashMap<String, Vec<Entry>>,
    clips: HashMap<String, Option<Clip>>,
    rng: u64,
    unresolved: HashSet<String>,
    output: Option<rodio::MixerDeviceSink>,
    /// The listener's ear and its right, in blocks.
    ear: DVec3,
    right: Vec3,
    pub volume: f32,
    /// The jukeboxes' songs as they play, and where.
    records: HashMap<(i32, i32, i32), (rodio::Player, DVec3)>,
}

impl Sounds {
    pub fn load(packs: &PackStack) -> Self {
        let mut events = HashMap::new();
        let json = ResourceId::parse("minecraft:sounds")
            .ok()
            .and_then(|id| packs.json(&id, "sounds.json").ok().flatten());
        if let Some(entries) = json.as_ref().and_then(serde_json::Value::as_object) {
            for (event, value) in entries {
                let list = value["sounds"]
                    .as_array()
                    .map(|sounds| {
                        sounds
                            .iter()
                            .filter_map(|sound| match sound {
                                serde_json::Value::String(name) => Some(Entry {
                                    name: name.clone(),
                                    volume: 1.0,
                                    pitch: 1.0,
                                    weight: 1,
                                    event: false,
                                }),
                                serde_json::Value::Object(o) => Some(Entry {
                                    name: o.get("name")?.as_str()?.to_owned(),
                                    volume: o
                                        .get("volume")
                                        .and_then(serde_json::Value::as_f64)
                                        .unwrap_or(1.0)
                                        as f32,
                                    pitch: o
                                        .get("pitch")
                                        .and_then(serde_json::Value::as_f64)
                                        .unwrap_or(1.0)
                                        as f32,
                                    weight: o
                                        .get("weight")
                                        .and_then(serde_json::Value::as_u64)
                                        .unwrap_or(1)
                                        as u32,
                                    event: o.get("type").and_then(serde_json::Value::as_str)
                                        == Some("event"),
                                }),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(Vec::new);
                events.insert(event.clone(), list);
            }
        }
        let output = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                Some(sink)
            }
            Err(error) => {
                log!("No sound: {error}");
                None
            }
        };
        Self {
            events,
            clips: HashMap::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
            unresolved: HashSet::new(),
            output,
            ear: DVec3::ZERO,
            right: Vec3::X,
            volume: 1.0,
            records: HashMap::new(),
        }
    }

    pub fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        ((self.rng >> 40) as u32 as f32) / ((1u32 << 24) as f32)
    }

    pub fn set_listener(&mut self, ear: DVec3, right: Vec3) {
        self.ear = ear;
        self.right = right;
        self.update_records();
    }

    /// `LevelRenderer.playJukeboxSong`: the song from the jukebox at `pos`
    /// (`forJukeboxSong`: volume 4, so heard to 64 blocks, falling off
    /// linearly), until it stops or ends.
    pub fn play_record(&mut self, packs: &PackStack, pos: (i32, i32, i32), event: &str) {
        self.stop_record(pos);
        let Some((samples, rate)) = self.resolve_clip(packs, event, 0) else {
            return;
        };
        let Some(output) = &self.output else {
            return;
        };
        let mut stereo = Vec::with_capacity(samples.len() * 2);
        for &sample in samples.iter() {
            stereo.push(sample);
            stereo.push(sample);
        }
        let (Some(channels), Some(rate)) = (std::num::NonZero::new(2u16), std::num::NonZero::new(rate)) else {
            return;
        };
        let player = rodio::Player::connect_new(output.mixer());
        player.append(rodio::buffer::SamplesBuffer::new(channels, rate, stereo));
        let at = DVec3::new(f64::from(pos.0) + 0.5, f64::from(pos.1) + 0.5, f64::from(pos.2) + 0.5);
        self.records.insert(pos, (player, at));
        self.update_records();
    }

    /// `stopJukeboxSong`.
    pub fn stop_record(&mut self, pos: (i32, i32, i32)) {
        if let Some((player, _)) = self.records.remove(&pos) {
            player.stop();
        }
    }

    /// Each song's loudness as the listener moves; finished ones go.
    fn update_records(&mut self) {
        let (ear, master) = (self.ear, self.volume);
        self.records.retain(|_, (player, _)| !player.empty());
        for (player, at) in self.records.values() {
            let falloff = (1.0 - at.distance(ear) / 64.0).clamp(0.0, 1.0) as f32;
            player.set_volume(falloff * master);
        }
    }

    /// A sound event's file (one of its variants by weight, through the
    /// events it names), decoded.
    fn resolve_clip(&mut self, packs: &PackStack, event: &str, depth: u8) -> Option<(Arc<[f32]>, u32)> {
        let key = event.strip_prefix("minecraft:").unwrap_or(event);
        let total: u32 = self.events.get(key).map_or(0, |e| e.iter().map(|x| x.weight).sum());
        if total == 0 || depth > 4 {
            return None;
        }
        let mut pick = (self.random() * total as f32) as u32;
        let entry = self.events.get(key)?.iter().find(|entry| {
            if pick < entry.weight {
                true
            } else {
                pick -= entry.weight;
                false
            }
        })?;
        let (name, is_event) = (entry.name.clone(), entry.event);
        if is_event {
            return self.resolve_clip(packs, &name, depth + 1);
        }
        let clip = self.clips.entry(name.clone()).or_insert_with(|| {
            let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", &name));
            let id = ResourceId::parse(&format!("{namespace}:{path}")).ok()?;
            let bytes = packs.get(&id, &format!("sounds/{path}.ogg")).ok().flatten()?;
            decode(bytes)
        });
        clip.as_ref().map(|c| (c.samples.clone(), c.rate))
    }

    /// Plays `event` at a point in blocks (or at the listener), picking one
    /// of its variants by weight.
    pub fn play(
        &mut self,
        packs: &PackStack,
        event: &str,
        position: Option<DVec3>,
        volume: f32,
        pitch: f32,
    ) {
        self.play_depth(packs, event, position, volume, pitch, 0);
    }

    fn play_depth(
        &mut self,
        packs: &PackStack,
        event: &str,
        position: Option<DVec3>,
        volume: f32,
        pitch: f32,
        depth: u8,
    ) {
        if self.output.is_none() {
            return;
        }
        let key = event.strip_prefix("minecraft:").unwrap_or(event);
        let total: u32 = self
            .events
            .get(key)
            .map_or(0, |e| e.iter().map(|x| x.weight).sum());
        if total == 0 || depth > 4 {
            if self.unresolved.insert(key.to_owned()) {
                log!("Sound event `{event}` has no sounds");
            }
            return;
        }
        let mut pick = (self.random() * total as f32) as u32;
        let Some(entries) = self.events.get(key) else {
            return;
        };
        let Some(entry) = entries.iter().find(|entry| {
            if pick < entry.weight {
                true
            } else {
                pick -= entry.weight;
                false
            }
        }) else {
            return;
        };
        let (name, entry_volume, entry_pitch, is_event) =
            (entry.name.clone(), entry.volume, entry.pitch, entry.event);
        if is_event {
            self.play_depth(
                packs,
                &name,
                position,
                volume * entry_volume,
                pitch * entry_pitch,
                depth + 1,
            );
            return;
        }
        let clip = self.clips.entry(name.clone()).or_insert_with(|| {
            let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", &name));
            let id = ResourceId::parse(&format!("{namespace}:{path}")).ok()?;
            let bytes = packs
                .get(&id, &format!("sounds/{path}.ogg"))
                .ok()
                .flatten()?;
            decode(bytes)
        });
        let Some(clip) = clip.as_ref() else {
            if self.unresolved.insert(name.clone()) {
                log!("Sound file `{name}` is missing or did not decode");
            }
            return;
        };
        let volume = volume * entry_volume;
        // `SoundEngine`: linear falloff to sixteen blocks, times the volume
        // when it is above one.
        let (gain, pan) = match position {
            Some(at) => {
                let offset = at - self.ear;
                let reach = 16.0 * f64::from(volume.max(1.0));
                let falloff = (1.0 - offset.length() / reach).clamp(0.0, 1.0) as f32;
                let side = offset.as_vec3().normalize_or_zero().dot(self.right);
                // Sounds right beside the ear pan less than far ones.
                let closeness = (offset.length() as f32 / 2.0).min(1.0);
                (volume.min(1.0) * falloff, side * closeness)
            }
            None => (volume.min(1.0), 0.0),
        };
        let gain = gain * self.volume;
        if gain <= 0.001 {
            return;
        }
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let (left, right) = (angle.cos() * gain, angle.sin() * gain);
        let mut stereo = Vec::with_capacity(clip.samples.len() * 2);
        for &sample in clip.samples.iter() {
            stereo.push(sample * left);
            stereo.push(sample * right);
        }
        let (Some(channels), Some(rate)) = (
            std::num::NonZero::new(2u16),
            std::num::NonZero::new(clip.rate),
        ) else {
            return;
        };
        let source = rodio::buffer::SamplesBuffer::new(channels, rate, stereo);
        if let Some(output) = &self.output {
            output.mixer().add(rodio::Source::speed(
                source,
                (pitch * entry_pitch).clamp(0.5, 2.0),
            ));
        }
    }
}

fn decode(bytes: Vec<u8>) -> Option<Clip> {
    let decoder = rodio::Decoder::new(std::io::Cursor::new(bytes)).ok()?;
    let channels = usize::from(rodio::Source::channels(&decoder).get());
    let rate = rodio::Source::sample_rate(&decoder).get();
    let interleaved: Vec<f32> = decoder.collect();
    let samples: Vec<f32> = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Some(Clip {
        rate,
        samples: samples.into(),
    })
}
