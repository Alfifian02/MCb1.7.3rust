//! M7 saving. Deliberately NOT McRegion (the roadmap's first idea): no region files, no NBT, no zlib.
//!
//! - One file per chunk, `c.<cx>.<cz>`: a populated flag, then the block ids and the metadata nibbles, each run-length
//!   coded as (byte, run 1..=255) pairs. Terrain is long vertical runs, so a chunk is a few KB instead of 48 KB.
//!   Only chunks that differ from what the seed generates are written (edited, or touched by a `populate`); the
//!   rest are regenerated. Light and the height map are not stored, they are recomputed on load.
//! - `level`: world time, player, inventory, furnaces, chests and dropped items, little-endian, versioned by its magic.
//! - Every write goes to `<name>.tmp` and is renamed over the old file, so a kill mid-write keeps the old save.
//!
//! Decoding is a trust boundary (a truncated or foreign file): every read is checked and any problem is `None`,
//! which the callers treat as "no save" (chunk: regenerate; level: new game). Never a panic.

use std::{fs, io, path::Path};

use glam::Vec3;

use crate::world::chunk::{Nibbles, VOLUME};
use crate::world::chest::{Chest, Pos};
use crate::world::craft::Furnace;
use crate::world::items::{ItemStack, MAX_ITEMS, SLOTS};

/// Atomic write: a crash leaves either the old file or the new one, never half of it.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // Not `with_extension`: the chunk names end in a number, which it would replace.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

fn rle(src: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < src.len() {
        let b = src[i];
        let n = src[i..].iter().take(255).take_while(|&&x| x == b).count(); // >= 1: src[i] == b
        out.extend_from_slice(&[b, n as u8]);
        i += n;
    }
}

/// Exactly `len` bytes of runs, and what follows them; `None` on a short input, a zero run or an overrun.
fn unrle(src: &[u8], len: usize) -> Option<(Vec<u8>, &[u8])> {
    let mut out = Vec::with_capacity(len);
    let mut at = 0;
    while out.len() < len {
        let (&b, &n) = (src.get(at)?, src.get(at + 1)?);
        if n == 0 || out.len() + n as usize > len {
            return None;
        }
        out.resize(out.len() + n as usize, b);
        at += 2;
    }
    Some((out, &src[at..]))
}

pub fn encode_chunk(blocks: &[u8], data: &Nibbles, populated: bool) -> Vec<u8> {
    let mut out = vec![populated as u8];
    rle(blocks, &mut out);
    rle(data.bytes(), &mut out);
    out
}

/// (blocks, metadata, populated).
pub fn decode_chunk(b: &[u8]) -> Option<(Vec<u8>, Nibbles, bool)> {
    let (&populated, rest) = b.split_first()?;
    let (blocks, rest) = unrle(rest, VOLUME)?;
    let (data, rest) = unrle(rest, VOLUME / 2)?;
    if populated > 1 || !rest.is_empty() {
        return None;
    }
    Some((blocks, Nibbles::from_bytes(data), populated == 1))
}

/// Everything about the player and the loose ends of the world that is not a chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub ticks: f64,
    pub pos: Vec3,
    pub spawn: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub slot: u8,
    /// health, air, fire.
    pub vitals: [i32; 3],
    pub inv: [Option<ItemStack>; SLOTS],
    pub furnaces: Vec<((i32, i32, i32), Furnace)>,
    /// Dropped items: position, stack, age. Motion is not kept: they come back at rest and settle.
    pub drops: Vec<(Vec3, ItemStack, u32)>,
    /// `TileEntityChest`s by block position (the Java's `Items` list, 27 slots each).
    pub chests: Vec<(Pos, Chest)>,
}

const MAGIC: &[u8; 4] = b"MCL1";

fn put<const N: usize>(w: &mut Vec<u8>, a: [u8; N]) {
    w.extend_from_slice(&a);
}

/// An empty slot is id 0 (no item has it).
fn put_stack(w: &mut Vec<u8>, s: Option<ItemStack>) {
    let s = s.unwrap_or(ItemStack { id: 0, count: 0, damage: 0 });
    put(w, s.id.to_le_bytes());
    w.push(s.count);
    put(w, s.damage.to_le_bytes());
}

fn put_vec(w: &mut Vec<u8>, v: Vec3) {
    v.to_array().into_iter().for_each(|c| put(w, c.to_le_bytes()));
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn get<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s: &[u8] = self.0;
        let (a, rest) = s.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*a)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.get::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.get()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.get()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.get()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.get()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.get()?))
    }
    fn vec(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    /// `Some(None)` is an empty slot; a stack of 0 or more than 64 is corrupt.
    fn stack(&mut self) -> Option<Option<ItemStack>> {
        let (id, count, damage) = (self.u16()?, self.u8()?, self.u16()?);
        if id == 0 {
            return Some(None);
        }
        (1..=64).contains(&count).then_some(Some(ItemStack { id, count, damage }))
    }
}

impl Level {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = MAGIC.to_vec();
        put(&mut w, self.ticks.to_le_bytes());
        put_vec(&mut w, self.pos);
        put_vec(&mut w, self.spawn);
        put(&mut w, self.yaw.to_le_bytes());
        put(&mut w, self.pitch.to_le_bytes());
        w.push(self.slot);
        self.vitals.iter().for_each(|v| put(&mut w, v.to_le_bytes()));
        self.inv.iter().for_each(|&s| put_stack(&mut w, s));
        put(&mut w, (self.furnaces.len() as u16).to_le_bytes());
        for ((x, y, z), f) in &self.furnaces {
            [x, y, z].iter().for_each(|c| put(&mut w, c.to_le_bytes()));
            f.slots.iter().for_each(|&s| put_stack(&mut w, s));
            [f.burn, f.item_burn, f.cook].iter().for_each(|c| put(&mut w, c.to_le_bytes()));
        }
        let drops = &self.drops[..self.drops.len().min(MAX_ITEMS)];
        put(&mut w, (drops.len() as u16).to_le_bytes());
        for &(p, s, age) in drops {
            put_vec(&mut w, p);
            put_stack(&mut w, Some(s));
            put(&mut w, age.to_le_bytes());
        }
        // Chests come last so a level file from before them still decodes (it just has none).
        put(&mut w, (self.chests.len() as u16).to_le_bytes());
        for ((x, y, z), c) in &self.chests {
            [x, y, z].iter().for_each(|v| put(&mut w, v.to_le_bytes()));
            c.iter().for_each(|&s| put_stack(&mut w, s));
        }
        w
    }

    pub fn decode(b: &[u8]) -> Option<Level> {
        let mut r = R(b.strip_prefix(MAGIC)?);
        let ticks = r.f64()?;
        let (pos, spawn) = (r.vec()?, r.vec()?);
        let (yaw, pitch, slot) = (r.f32()?, r.f32()?, r.u8()?);
        let vitals = [r.i32()?, r.i32()?, r.i32()?];
        // A NaN position would poison physics and the camera for good, so it is a corrupt save.
        if !(ticks.is_finite() && pos.is_finite() && spawn.is_finite() && yaw.is_finite() && pitch.is_finite()) || slot >= 9 {
            return None;
        }
        let mut inv = [None; SLOTS];
        for s in inv.iter_mut() {
            *s = r.stack()?;
        }
        let furnaces = (0..r.u16()?)
            .map(|_| {
                Some((
                    (r.i32()?, r.i32()?, r.i32()?),
                    Furnace { slots: [r.stack()?, r.stack()?, r.stack()?], burn: r.u16()?, item_burn: r.u16()?, cook: r.u16()? },
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        let drops = (0..r.u16()?.min(MAX_ITEMS as u16))
            .map(|_| Some((r.vec()?, r.stack()??, r.u32()?)))
            .collect::<Option<Vec<_>>>()?;
        let chests = if r.0.is_empty() {
            Vec::new()
        } else {
            (0..r.u16()?)
                .map(|_| {
                    let p = (r.i32()?, r.i32()?, r.i32()?);
                    let mut c: Chest = Default::default();
                    for s in c.iter_mut() {
                        *s = r.stack()?;
                    }
                    Some((p, c))
                })
                .collect::<Option<Vec<_>>>()?
        };
        Some(Level { ticks, pos, spawn, yaw, pitch, slot, vitals, inv, furnaces, drops, chests })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chunk (stone below y = 60, a few stray blocks; all-zero metadata, whose runs are split at 255) round-trips exactly (metadata, flag and all); any cut, extra byte or
    /// bad flag is refused. The level round-trips, a cut or a NaN position is refused.
    #[test]
    fn chunk_and_level_round_trip() {
        let mut blocks = vec![0u8; VOLUME];
        for (i, b) in blocks.iter_mut().enumerate() {
            *b = if i % 128 < 60 { 1 } else if i % 977 == 0 { 3 } else { 0 };
        }
        let mut data = Nibbles::new();
        data.set(3, 40, 5, 14);
        data.set(15, 127, 15, 9);
        let enc = encode_chunk(&blocks, &data, true);
        assert!(enc.len() < VOLUME / 4, "{} bytes", enc.len());
        let (b2, d2, p2) = decode_chunk(&enc).unwrap();
        assert!(b2 == blocks && d2 == data && p2);
        assert!(!decode_chunk(&encode_chunk(&vec![7u8; VOLUME], &Nibbles::new(), false)).unwrap().2);
        assert!(decode_chunk(&enc[..enc.len() - 1]).is_none());
        assert!(decode_chunk(&[&enc[..], &[0]].concat()).is_none());
        assert!(decode_chunk(&[&[2u8][..], &enc[1..]].concat()).is_none());
        assert!(decode_chunk(&[]).is_none());

        let mut inv = [None; SLOTS];
        inv[0] = Some(ItemStack { id: 257, count: 1, damage: 12 });
        inv[35] = Some(ItemStack { id: 4, count: 64, damage: 0 });
        let l = Level {
            ticks: 12345.5,
            pos: Vec3::new(-3.5, 70.0, 9.25),
            spawn: Vec3::new(8.5, 66.0, 8.5),
            yaw: 1.5,
            pitch: -0.25,
            slot: 4,
            vitals: [17, 300, 0],
            inv,
            furnaces: vec![((1, 64, -2), Furnace { slots: [Some(ItemStack { id: 15, count: 3, damage: 0 }), None, None], burn: 10, item_burn: 100, cook: 7 })],
            drops: vec![(Vec3::new(1.0, 2.0, 3.0), ItemStack { id: 17, count: 2, damage: 0 }, 99)],
            chests: vec![((4, 70, -9), {
                let mut c: Chest = Default::default();
                c[0] = Some(ItemStack { id: 265, count: 3, damage: 0 });
                c[26] = Some(ItemStack { id: 351, count: 1, damage: 3 });
                c
            })],
        };
        let enc = l.encode();
        assert_eq!(Level::decode(&enc), Some(l.clone()));
        // A level file written before chests existed ends after the drops: it decodes with no chests.
        let old = Level { chests: Vec::new(), ..l.clone() }.encode();
        assert_eq!(Level::decode(&old[..old.len() - 2]), Some(Level { chests: Vec::new(), ..l }));
        assert!(Level::decode(&enc[..enc.len() - 1]).is_none());
        assert!(Level::decode(b"nope").is_none());
        let mut nan = enc.clone();
        nan[12..16].copy_from_slice(&f32::NAN.to_le_bytes()); // pos.x
        assert!(Level::decode(&nan).is_none());
    }
}
