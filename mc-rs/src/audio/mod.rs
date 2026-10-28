//! Audio (M9): port of `SoundManager` / `SoundPool`. Every b1.7.3 sound is embedded (`table.rs`, written by `tools/gen_sounds.py`).
//!
//! Effects are decoded to mono on first use and mixed in software (linear resampling for pitch, distance gain, left/right pan);
//! music and records are decoded packet by packet on a thread and fed through a bounded channel. Output is AAudio through the
//! `ndk` crate on Android; elsewhere the mixer runs but nothing is played (ponytail: no desktop backend; cpal is the upgrade).
//! Written without a toolchain: not compiled, not run. UNVERIFIED: paulscode's linear attenuation is not in the decompiled tree,
//! `1 - d / max` (OpenAL `AL_LINEAR_DISTANCE_CLAMPED`, reference distance 0) is assumed; the `ndk::audio` calls follow ndk 0.9.

mod table;

use crate::world::gen::noise::JavaRandom;
use glam::Vec3;
use lewton::inside_ogg::OggStreamReader;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};

const RATE: f64 = 44100.0;
/// Headroom for the effects: the shipped effects are mastered hot (median peak -2.5 dBFS, 73 of 194 above -1) and the music is not (peaks
/// about -9 dBFS), so two overlapping effects at full gain went past full scale and hard-clipped: that is the harsh, too loud sound.
/// 0.5 is -6 dB on the effects; the Sound option still scales them on top. The record (13, peaks at 0 dBFS) is left as it is.
const VOICE_GAIN: f32 = 0.5;

/// A soft knee instead of a hard clamp for what still goes past full scale (many voices at once): linear for small samples.
fn soft_limit(x: f32) -> i16 {
    (32768.0 * (x / 32768.0).tanh()).clamp(-32768.0, 32767.0) as i16
}

/// OpenAL's usual source limit; a new sound past it replaces the oldest.
const MAX_VOICES: usize = 32;
/// `StepSound` of each `table::BLOCK_STEP` index: (key, volume, pitch). Key "" = `Block.soundPowderFootstep` etc. as in `Block.java`.
/// `place` is `func_1145_d` (step / place), `brk` is `stepSoundDir` (break): glass breaks as "random.glass", sand as "step.gravel".
pub const STEP: [(&str, &str, f32); 9] = [
    ("step.stone", "step.stone", 1.0), ("step.wood", "step.wood", 1.0), ("step.gravel", "step.gravel", 1.0),
    ("step.grass", "step.grass", 1.0), ("step.stone", "step.stone", 1.0), ("step.stone", "step.stone", 1.5),
    ("step.stone", "random.glass", 1.0), ("step.cloth", "step.cloth", 1.0), ("step.sand", "step.gravel", 1.0),
];

/// `Block.stepSound` index of a block id (unlisted blocks: powder, 0).
pub fn step_of(id: u8) -> usize {
    table::BLOCK_STEP.iter().find(|e| e.0 == id).map_or(0, |e| e.1 as usize)
}

struct Clip {
    pcm: Vec<i16>,
    rate: f64,
}

struct Voice {
    clip: Arc<Clip>,
    pos: f64,
    step: f64,
    gl: f32,
    gr: f32,
}

/// A decoded-on-a-thread stereo stream (music, record). `gl` / `gr` are its gains.
struct Stream {
    rx: Receiver<Vec<i16>>,
    buf: Vec<i16>,
    at: usize,
    gl: f32,
    gr: f32,
    done: bool,
}

impl Stream {
    fn open(data: &'static [u8], gl: f32, gr: f32) -> Option<Self> {
        let mut r = OggStreamReader::new(Cursor::new(data)).ok()?;
        if r.ident_hdr.audio_sample_rate != 44100 {
            return None; // every shipped track is 44.1 kHz; no resampler for streams
        }
        let ch = r.ident_hdr.audio_channels as usize;
        let (tx, rx) = sync_channel::<Vec<i16>>(16);
        std::thread::Builder::new().name("audio-stream".into()).spawn(move || {
            while let Ok(Some(p)) = r.read_dec_packet_itl() {
                let v = if ch == 1 { p.iter().flat_map(|&s| [s, s]).collect() } else { p };
                if tx.send(v).is_err() {
                    break; // the receiver was dropped: stopped
                }
            }
        }).ok()?;
        Some(Self { rx, buf: Vec::new(), at: 0, gl, gr, done: false })
    }

    fn next(&mut self) -> (f32, f32) {
        if self.at + 1 >= self.buf.len() {
            match self.rx.try_recv() {
                Ok(b) => { self.buf = b; self.at = 0; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.done = true; return (0.0, 0.0) }
                Err(_) => return (0.0, 0.0), // the decoder is behind: a gap, not a stall
            }
            if self.buf.len() < 2 { return (0.0, 0.0) }
        }
        let f = (self.buf[self.at] as f32, self.buf[self.at + 1] as f32);
        self.at += 2;
        (f.0 * self.gl, f.1 * self.gr)
    }
}

/// What the audio callback mixes. Locked by `Audio` for a moment per play call, `try_lock`ed by the callback.
#[derive(Default)]
pub struct Mix {
    voices: Vec<Voice>,
    music: Option<Stream>,
    record: Option<Stream>,
    music_vol: f32,
}

impl Mix {
    /// Fills `out` (interleaved stereo i16) with the sum of every voice.
    pub fn render(&mut self, out: &mut [i16]) {
        for f in out.chunks_exact_mut(2) {
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for v in &mut self.voices {
                let i = v.pos as usize;
                if let (Some(&a), Some(&b)) = (v.clip.pcm.get(i), v.clip.pcm.get(i + 1).or(v.clip.pcm.get(i))) {
                    let s = a as f32 + (b as f32 - a as f32) * (v.pos - i as f64) as f32;
                    l += s * v.gl;
                    r += s * v.gr;
                }
                v.pos += v.step;
            }
            (l, r) = (l * VOICE_GAIN, r * VOICE_GAIN);
            if let Some(m) = &mut self.music {
                let (a, b) = m.next();
                l += a * self.music_vol;
                r += b * self.music_vol;
            }
            if let Some(m) = &mut self.record {
                let (a, b) = m.next();
                l += a;
                r += b;
            }
            f[0] = soft_limit(l);
            f[1] = soft_limit(r);
        }
        self.voices.retain(|v| (v.pos as usize) < v.clip.pcm.len());
        self.music = self.music.take().filter(|m| !m.done);
        self.record = self.record.take().filter(|m| !m.done);
    }
}

fn decode(data: &[u8]) -> Option<Clip> {
    let mut r = OggStreamReader::new(Cursor::new(data)).ok()?;
    let (ch, rate) = (r.ident_hdr.audio_channels as usize, r.ident_hdr.audio_sample_rate as f64);
    let mut pcm = Vec::new();
    while let Ok(Some(p)) = r.read_dec_packet_itl() {
        pcm.extend(p.chunks(ch).map(|f| (f.iter().map(|&s| s as i32).sum::<i32>() / ch as i32) as i16));
    }
    Some(Clip { pcm, rate })
}

/// Whether `name` is a `SoundPool` key (the tests of whoever queues sounds check their names with it).
#[cfg(test)]
pub fn exists(name: &str) -> bool {
    table::SFX.iter().any(|e| e.0 == name)
}

/// A `World.playSoundEffect` a world system queued (`Ticks::sounds`); the frame hands each one to `Audio::play`.
pub struct Snd {
    pub name: &'static str,
    pub at: Vec3,
    pub vol: f32,
    pub pitch: f32,
}

/// The sounds the world makes around the player on its own: `World.updateBlocksAndPlayCaveSounds` (`cave`, 20 Hz) and the sound half of
/// `World.randomDisplayUpdates` (`display`, 20 Hz: running water, fire, a portal's hum). The particles of the second are not here.
pub struct Ambient {
    /// `World.soundCounter`: ticks until a cave sound may play again.
    counter: i32,
    /// `World.field_9437_g`, the LCG that picks one cell per chunk.
    lcg: i32,
    rng: JavaRandom,
}

impl Ambient {
    pub fn new(seed: i64) -> Self {
        let mut rng = JavaRandom::new(seed ^ 0xCA5E);
        let counter = rng.next_int_bound(12000);
        Self { counter, lcg: rng.next_int(), rng }
    }

    /// One tick of the cave sounds: with the counter at 0, each chunk within 9 of the player picks one cell, and a dark air cell
    /// (light <= `rand(8)`, no sky light) between 2 and 8 blocks from the player plays `ambient.cave.cave`, then 6000 to 18000 ticks of silence.
    /// `probe` is (block id, `getFullBlockLightValue`, sky light) of a cell; an unloaded cell reads as solid.
    pub fn cave(&mut self, ears: Vec3, probe: &dyn Fn(i32, i32, i32) -> (u8, u8, u8)) -> Option<Snd> {
        self.counter = (self.counter - 1).max(0);
        let (pcx, pcz) = ((ears.x / 16.0).floor() as i32, (ears.z / 16.0).floor() as i32);
        for cx in pcx - 9..=pcx + 9 {
            for cz in pcz - 9..=pcz + 9 {
                if self.counter != 0 {
                    return None;
                }
                self.lcg = self.lcg.wrapping_mul(3).wrapping_add(1013904223);
                let r = self.lcg >> 2;
                let (x, z, y) = (cx * 16 + (r & 15), cz * 16 + ((r >> 8) & 15), (r >> 16) & 127);
                let (id, full, sky) = probe(x, y, z);
                if id == 0 && full as i32 <= self.rng.next_int_bound(8) && sky == 0 {
                    let at = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    let d2 = (at - ears).length_squared(); // `getClosestPlayer(.., 8.0)` and `getDistanceSq > 4`
                    if d2 < 64.0 && d2 > 4.0 {
                        self.counter = self.rng.next_int_bound(12000) + 6000;
                        return Some(Snd { name: "ambient.cave.cave", at, vol: 0.7, pitch: 0.8 + self.rng.next_float() * 0.2 });
                    }
                }
            }
        }
        None
    }

    /// One tick of `randomDisplayUpdates`: 1000 cells within 16 of `(x, y, z)` (the sum of two draws: nearer is likelier), and what
    /// each one's `randomDisplayTick` says aloud: flowing water 1 in 64 (levels 1..=7), fire 1 in 24, a portal 1 in 100.
    pub fn display(&mut self, (px, py, pz): (i32, i32, i32), block: &dyn Fn(i32, i32, i32) -> u8, meta: &dyn Fn(i32, i32, i32) -> u8, out: &mut Vec<Snd>) {
        for _ in 0..1000 {
            let x = px + self.rng.next_int_bound(16) - self.rng.next_int_bound(16);
            let y = py + self.rng.next_int_bound(16) - self.rng.next_int_bound(16);
            let z = pz + self.rng.next_int_bound(16) - self.rng.next_int_bound(16);
            let id = block(x, y, z);
            if !matches!(id, 8 | 9 | 51 | 90) {
                continue;
            }
            let at = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
            if matches!(id, 8 | 9) {
                if self.rng.next_int_bound(64) == 0 && (1..8).contains(&meta(x, y, z)) {
                    out.push(Snd { name: "liquid.water", at, vol: self.rng.next_float() * 0.25 + 0.75, pitch: self.rng.next_float() + 0.5 });
                }
            } else if id == 51 {
                if self.rng.next_int_bound(24) == 0 {
                    out.push(Snd { name: "fire.fire", at, vol: 1.0 + self.rng.next_float(), pitch: self.rng.next_float() * 0.7 + 0.3 });
                }
            } else if self.rng.next_int_bound(100) == 0 {
                out.push(Snd { name: "portal.portal", at, vol: 1.0, pitch: self.rng.next_float() * 0.4 + 0.8 });
            }
        }
    }
}

pub struct Audio {
    mix: Arc<Mutex<Mix>>,
    cache: HashMap<usize, Arc<Clip>>,
    rng: JavaRandom,
    /// `GameSettings.soundVolume` / `musicVolume`.
    sound: f32,
    music: f32,
    ears: Vec3,
    right: Vec3,
    /// `SoundManager.ticksBeforeMusic`.
    before_music: i32,
    #[allow(dead_code)]
    out: Option<Keep>,
}

/// The output stream, held only so it keeps playing and closes when `Audio` drops. `App` is built on the GPU init thread and handed to
/// the main thread (`start_init`), so `Audio` has to be `Send`; the AAudio stream is a raw handle and is not.
/// SAFETY: nothing ever calls the stream after `start_output`; the only thing done to it later is dropping it (`AAudioStream_close`),
/// which AAudio allows from any thread.
struct Keep(#[allow(dead_code)] Box<dyn std::any::Any>);
unsafe impl Send for Keep {}

impl Audio {
    pub fn new() -> Self {
        let mix = Arc::new(Mutex::new(Mix::default()));
        let mut rng = JavaRandom::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as i64));
        let before_music = rng.next_int_bound(12000);
        let out = start_output(mix.clone());
        Self { mix, cache: HashMap::new(), rng, sound: 1.0, music: 1.0, ears: Vec3::ZERO, right: Vec3::X, before_music, out }
    }

    /// `onSoundOptionsChanged`: music off stops the track, otherwise the volume follows at once.
    pub fn set_volumes(&mut self, sound: f32, music: f32) {
        (self.sound, self.music) = (sound, music);
        let mut m = self.mix.lock().unwrap();
        m.music_vol = music;
        if music == 0.0 {
            m.music = None;
        }
    }

    /// `func_338_a`: the listener sits at the eye, facing `yaw` degrees (`EntityLiving.rotationYaw`); its right vector is look x up.
    pub fn listener(&mut self, eye: Vec3, yaw_deg: f32) {
        let a = -yaw_deg.to_radians() - std::f32::consts::PI;
        let (fx, fz) = (-a.sin(), -a.cos());
        (self.ears, self.right) = (eye, Vec3::new(-fz, 0.0, fx));
    }

    fn pick(&mut self, name: &str) -> Option<(usize, Arc<Clip>)> {
        let (_, v) = table::SFX.iter().find(|e| e.0 == name)?;
        let d = v[self.rng.next_int_bound(v.len() as i32) as usize];
        let k = d.as_ptr() as usize;
        if let Some(c) = self.cache.get(&k) {
            return Some((k, c.clone()));
        }
        let c = Arc::new(decode(d)?);
        self.cache.insert(k, c.clone());
        Some((k, c))
    }

    fn add(&mut self, clip: Arc<Clip>, pitch: f32, gl: f32, gr: f32) {
        let mut m = self.mix.lock().unwrap();
        if m.voices.len() >= MAX_VOICES {
            m.voices.remove(0);
        }
        m.voices.push(Voice { step: pitch as f64 * clip.rate / RATE, clip, pos: 0.0, gl, gr });
    }

    /// `playSound`: positional, `vol > 1` carries further (`16 * vol` blocks) but never gets louder than 1.
    pub fn play(&mut self, name: &str, at: Vec3, vol: f32, pitch: f32) {
        self.play_gain(name, at, vol, pitch, 1.0);
    }

    /// `play` with the loudness scaled by `gain` (0..1) without shortening its reach: a sound heard through a roof.
    pub fn play_gain(&mut self, name: &str, at: Vec3, vol: f32, pitch: f32, gain: f32) {
        if self.sound == 0.0 || vol <= 0.0 {
            return;
        }
        let Some((_, clip)) = self.pick(name) else { return };
        let (d, dir) = (at - self.ears, (at - self.ears).normalize_or_zero());
        let g = (1.0 - d.length() / (16.0 * vol.max(1.0))).max(0.0) * vol.min(1.0) * self.sound * gain;
        let pan = dir.dot(self.right); // -1 left .. 1 right; constant power
        let (l, r) = (((1.0 - pan) * 0.5).sqrt(), ((1.0 + pan) * 0.5).sqrt());
        if g > 0.0 {
            self.add(clip, pitch, g * l, g * r);
        }
    }

    /// `playSoundAtEntity` for the player's own sounds (hand, steps): at the ears, centred.
    pub fn play_here(&mut self, name: &str, vol: f32, pitch: f32) {
        let e = self.ears;
        self.play(name, e, vol, pitch);
    }

    /// One draw of the sound random, for the pitches `base + r * spread` that callers build themselves.
    pub fn rnd(&mut self) -> f32 {
        self.rng.next_float()
    }

    /// `playSoundAtEntity` with the entity pitch `(r - r) * 0.2 + 1`, times `mul` (`jit` false: exactly `mul`, as a creeper's fuse at 0.5).
    /// The draws come from here, not from the mob's own random, so a sound never changes what a mob does.
    pub fn cry(&mut self, name: &str, at: Vec3, vol: f32, mul: f32, jit: bool) {
        let pitch = if jit { ((self.rng.next_float() - self.rng.next_float()) * 0.2 + 1.0) * mul } else { mul };
        self.play(name, at, vol, pitch);
    }

    /// `RenderGlobal` aux effect 1003 (a door or trapdoor moved): open or close, one in two whichever it did (`Math.random() < 0.5`).
    /// The b1.7.3 asset index has neither `random.door_open` nor `random.door_close`, so the original is silent here too and so is this;
    /// a sound pack that adds them to `table::SFX` is all it needs.
    pub fn door(&mut self, at: Vec3) {
        let name = if self.rng.next_float() < 0.5 { "random.door_open" } else { "random.door_close" };
        let pitch = self.rng.next_float() * 0.1 + 0.9;
        self.play(name, at, 1.0, pitch);
    }

    /// `playSoundFX` (GUI clicks): not positional, volume x 0.25.
    pub fn play_fx(&mut self, name: &str, vol: f32, pitch: f32) {
        if self.sound == 0.0 {
            return;
        }
        let Some((_, clip)) = self.pick(name) else { return };
        let g = vol.min(1.0) * 0.25 * self.sound;
        self.add(clip, pitch, g, g);
    }

    /// `playStreaming`: a record ("13", "cat") at `at`, or `None` to stop it. Stops the background music while it plays.
    pub fn play_record(&mut self, name: Option<&str>, at: Vec3) {
        let mut m = self.mix.lock().unwrap();
        m.record = None;
        let Some(data) = name.and_then(|n| table::RECORDS.iter().find(|r| r.0 == n)).map(|r| r.1) else { return };
        if self.sound == 0.0 {
            return;
        }
        let g = (1.0 - (at - self.ears).length() / 64.0).max(0.0) * 0.5 * self.sound;
        m.music = None;
        m.record = Stream::open(data, g, g);
    }

    /// `playRandomMusicIfReady`: call at 20 Hz. A random track 10 to 20 minutes after the last one (the first after 0 to 10).
    pub fn tick(&mut self) {
        if self.music == 0.0 {
            return;
        }
        let mut m = self.mix.lock().unwrap();
        if m.music.is_some() || m.record.is_some() {
            return;
        }
        if self.before_music > 0 {
            self.before_music -= 1;
            return;
        }
        let t = table::MUSIC[self.rng.next_int_bound(table::MUSIC.len() as i32) as usize];
        self.before_music = self.rng.next_int_bound(12000) + 12000;
        m.music = Stream::open(t, 1.0, 1.0);
    }

    /// `BlockNote.playBlock`: instrument 0 harp, 1 bd, 2 snare, 3 hat, 4 bassattack; pitch `2^((note - 12) / 12)`, volume 3.
    pub fn note(&mut self, at: Vec3, note: u8, inst: u8) {
        let name = ["note.harp", "note.bd", "note.snare", "note.hat", "note.bassattack"][inst.min(4) as usize];
        self.play(name, at, 3.0, 2f32.powf((note as f32 - 12.0) / 12.0));
    }
}

#[cfg(target_os = "android")]
fn start_output(mix: Arc<Mutex<Mix>>) -> Option<Keep> {
    use ndk::audio::{AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioStreamBuilder};
    let s = AudioStreamBuilder::new().ok()?
        .direction(AudioDirection::Output).format(AudioFormat::PCM_I16).channel_count(2).sample_rate(44100)
        .performance_mode(AudioPerformanceMode::None)
        .data_callback(Box::new(move |_, data, frames| {
            let out = unsafe { std::slice::from_raw_parts_mut(data as *mut i16, frames as usize * 2) };
            match mix.try_lock() {
                Ok(mut m) => m.render(out),
                Err(_) => out.fill(0),
            }
            AudioCallbackResult::Continue
        }))
        .open_stream().ok()?;
    s.request_start().ok()?;
    Some(Keep(Box::new(s)))
}

#[cfg(not(target_os = "android"))]
fn start_output(_: Arc<Mutex<Mix>>) -> Option<Keep> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_hums_where_there_is_fire_and_water_and_cave_sounds_wait_their_turn() {
        let mut a = Ambient::new(7);
        let mut out = Vec::new();
        a.display((0, 64, 0), &|_, _, _| 51, &|_, _, _| 0, &mut out);
        assert!(out.len() > 10 && out.iter().all(|s| s.name == "fire.fire" && (1.0..2.0).contains(&s.vol) && (0.3..1.0).contains(&s.pitch)));
        out.clear();
        a.display((0, 64, 0), &|_, _, _| 9, &|_, _, _| 0, &mut out);
        assert!(out.is_empty(), "still water (level 0) is silent");
        a.display((0, 64, 0), &|_, _, _| 8, &|_, _, _| 3, &mut out);
        assert!(!out.is_empty() && out.iter().all(|s| s.name == "liquid.water"));
        // A dark open cave: the first sound comes from a cell 2 to 8 blocks away, then it is quiet for 6000 ticks.
        let (ears, dark) = (Vec3::new(8.0, 40.0, 8.0), |_: i32, _: i32, _: i32| (0u8, 0u8, 0u8));
        a.counter = 0;
        let s = (0..20000).find_map(|_| a.cave(ears, &dark)).expect("some chunk picks a cell near enough");
        let d = (s.at - ears).length();
        assert!(s.name == "ambient.cave.cave" && d > 2.0 && d < 8.0 && s.vol == 0.7 && (0.8..1.0).contains(&s.pitch));
        assert!((0..5999).all(|_| a.cave(ears, &dark).is_none()));
        // Daylight (sky light) or solid rock never speaks.
        a.counter = 0;
        assert!((0..300).all(|_| a.cave(ears, &|_, _, _| (0, 0, 15)).is_none() && a.cave(ears, &|_, _, _| (1, 0, 0)).is_none()));
    }

    #[test]
    fn every_name_the_java_plays_exists_and_decodes() {
        for k in ["random.click", "random.explode", "random.glass", "step.grass", "step.sand", "step.snow", "mob.ghast.fireball", "mob.zombiepig.zpigangry",
                  "ambient.cave.cave", "ambient.weather.thunder", "note.bassattack", "tile.piston.in", "liquid.water", "fire.fire", "portal.portal", "random.wood click", "random.bow", "random.bowhit", "random.pop", "random.splash", "mob.wolf.shake"] {
            let (_, v) = table::SFX.iter().find(|e| e.0 == k).unwrap_or_else(|| panic!("{k}"));
            assert!(v.iter().all(|d| decode(d).is_some_and(|c| !c.pcm.is_empty())), "{k}");
        }
        // The b1.7.3 index has 80 keys in 194 files; NOT b1.7.3: its wolf's six cries (12 files) are replaced by seven voices of six (84 files),
        // and `random.bow` (the rod's cast and the bow's shot; one file) and `random.bowhit` (four) are added from `assets/sound/extra` because the index lacks them.
        assert_eq!((table::SFX.len(), table::MUSIC.len(), table::RECORDS.len()), (80 - 6 + 7 * 6 + 2, 12, 2));
        assert_eq!(table::SFX.iter().map(|e| e.1.len()).sum::<usize>(), 194 - 12 + 7 * 12 + 1 + 4);
    }

    #[test]
    fn step_table_matches_block_java() {
        assert_eq!((step_of(1), step_of(2), step_of(12), step_of(20), step_of(35), step_of(41), step_of(0)), (4, 3, 8, 6, 7, 5, 0));
        assert_eq!(STEP[step_of(20)].1, "random.glass"); // glass breaks as glass, sand as gravel
        assert_eq!(STEP[step_of(12)].1, "step.gravel");
    }

    #[test]
    fn loud_voices_stack_without_hard_clipping() {
        let clip = Arc::new(Clip { pcm: vec![30000; 64], rate: RATE });
        let mut m = Mix::default();
        m.voices = (0..4).map(|_| Voice { clip: clip.clone(), pos: 0.0, step: 1.0, gl: 1.0, gr: 1.0 }).collect();
        let mut out = vec![0i16; 2 * 32];
        m.render(&mut out);
        // 4 x 30000 x 0.5 = 60000 would be clamped to 32767; the soft knee stays below it and above one voice's share.
        assert!(out.iter().all(|&s| (30000..32767).contains(&s)), "{:?}", &out[..4]);
    }

    #[test]
    fn a_voice_is_heard_on_the_near_side_and_ends() {
        let mut a = Audio::new();
        a.listener(Vec3::ZERO, 0.0);
        let right = a.right;
        a.play("random.click", right * 4.0, 1.0, 1.0);
        let mut out = vec![0i16; 2 * 4096];
        a.mix.lock().unwrap().render(&mut out);
        let (l, r): (i64, i64) = out.chunks(2).fold((0, 0), |s, f| (s.0 + f[0].abs() as i64, s.1 + f[1].abs() as i64));
        assert!(r > l && l > 0);
        for _ in 0..40 { a.mix.lock().unwrap().render(&mut out); }
        assert!(a.mix.lock().unwrap().voices.is_empty());
        a.play("random.click", Vec3::new(100.0, 0.0, 0.0), 1.0, 1.0); // past 16 blocks: silent, not queued
        assert!(a.mix.lock().unwrap().voices.is_empty());
    }
}
