#!/usr/bin/env python3
"""Membuat mc-core/src/material.rs dan blocks.rs dari dump registri b1.7.3 asli.

Cara pakai (butuh JDK 11+):
  java -cp deobfuscated.jar tools/DumpBlocks.java > dump.txt
  java -cp deobfuscated.jar tools/DumpMat.java   > mat.txt
  python3 tools/gen_blocks.py dump.txt mat.txt mc-core/src
"""
import struct, sys, re

def f32(x):
    return struct.unpack('f', struct.pack('f', float(x)))[0]

def lit(x):
    v = f32(x)
    for p in range(1, 10):
        s = '%.*g' % (p, v)
        if f32(float(s)) == v:
            break
    if 'e' in s or 'E' in s:
        m, e = s.lower().split('e')
        if '.' not in m:
            m += '.0'
        return f'{m}e{int(e)}'
    if '.' not in s:
        s += '.0'
    return s

def b(s):
    return 'true' if s == 'true' else 'false'

MAT = {
 'air':'Air','grassMaterial':'Grass','ground':'Ground','wood':'Wood','rock':'Rock','iron':'Iron',
 'water':'Water','lava':'Lava','leaves':'Leaves','plants':'Plants','sponge':'Sponge','cloth':'Cloth',
 'fire':'Fire','sand':'Sand','circuits':'Circuits','glass':'Glass','tnt':'Tnt','field_4262_q':'Unused4262',
 'ice':'Ice','snow':'Snow','builtSnow':'BuiltSnow','cactus':'Cactus','clay':'Clay','pumpkin':'Pumpkin',
 'portal':'Portal','cakeMaterial':'Cake','field_31068_A':'Web','field_31067_B':'Piston'}

SND = {  # nama field Java -> (varian, nama dasar, pitch, suara_pecah)
 'soundPowderFootstep':('Powder','stone',1.0,'step.stone'),
 'soundWoodFootstep':('Wood','wood',1.0,'step.wood'),
 'soundGravelFootstep':('Gravel','gravel',1.0,'step.gravel'),
 'soundGrassFootstep':('Grass','grass',1.0,'step.grass'),
 'soundStoneFootstep':('Stone','stone',1.0,'step.stone'),
 'soundMetalFootstep':('Metal','stone',1.5,'step.stone'),
 'soundGlassFootstep':('Glass','stone',1.0,'random.glass'),
 'soundClothFootstep':('Cloth','cloth',1.0,'step.cloth'),
 'soundSandFootstep':('Sand','sand',1.0,'step.gravel'),
}

def main(dump, mat, out):
    mats, colors = [], []
    for l in open(mat):
        p = l.strip().split('|')
        if p[0] == 'M': mats.append(p[1:])
        if p[0] == 'C': colors.append(p[1:])
    blocks = [l.strip().split('|') for l in open(dump) if re.match(r'^\d+\|', l)]
    blocks.sort(key=lambda p: int(p[0]))
    ids = [int(p[0]) for p in blocks]
    assert ids == list(range(1, len(ids) + 1)), 'ID tidak berurutan'

    # ---- material.rs
    o = ['//! DIHASILKAN oleh tools/gen_blocks.py dari registri b1.7.3 asli. Jangan edit manual.',
         '',
         '#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]',
         '#[repr(u8)]',
         'pub enum Material {']
    for m in mats: o.append(f'    {MAT[m[0]]},')
    o += ['}', '',
          '#[derive(Clone, Copy, Debug)]',
          'pub struct MaterialProps {',
          '    pub map_color: u8,',
          '    pub is_liquid: bool,',
          '    /// Java: isSolid()',
          '    pub solid: bool,',
          '    pub can_block_grass: bool,',
          '    /// Java: getIsSolid()',
          '    pub is_solid2: bool,',
          '    pub burns: bool,',
          '    pub ground_cover: bool,',
          '    /// Java: getIsTranslucent() (nama asli menyesatkan; ikuti perilaku Java)',
          '    pub translucent: bool,',
          '    pub harvestable: bool,',
          '    /// 0 = bisa didorong piston, 1 = hancur saat didorong, 2 = tak bisa digerakkan',
          '    pub mobility: u8,',
          '}', '',
          'pub static MATERIAL_PROPS: [MaterialProps; %d] = [' % len(mats)]
    for m in mats:
        o.append('    MaterialProps { map_color: %s, is_liquid: %s, solid: %s, can_block_grass: %s, is_solid2: %s, burns: %s, ground_cover: %s, translucent: %s, harvestable: %s, mobility: %s },  // %s' %
                 (m[1], b(m[2]), b(m[3]), b(m[4]), b(m[5]), b(m[6]), b(m[7]), b(m[8]), b(m[9]), m[10], MAT[m[0]]))
    o += ['];', '',
          'impl Material {',
          '    #[inline]',
          '    pub fn props(self) -> &\'static MaterialProps {',
          '        &MATERIAL_PROPS[self as usize]',
          '    }',
          '    #[inline] pub fn is_liquid(self) -> bool { self.props().is_liquid }',
          '    #[inline] pub fn is_solid(self) -> bool { self.props().solid }',
          '    #[inline] pub fn can_block_grass(self) -> bool { self.props().can_block_grass }',
          '    #[inline] pub fn burns(self) -> bool { self.props().burns }',
          '    #[inline] pub fn mobility(self) -> u8 { self.props().mobility }',
          '}', '',
          '/// Warna peta (MapColor): (indeks, RGB)',
          'pub static MAP_COLORS: [(u8, u32); %d] = [' % len(colors)]
    for c in colors: o.append(f'    ({c[1]}, {c[2]}),  // {c[0]}')
    o += ['];', '',
          '#[cfg(test)]', 'mod tests {', '    use super::*;', '',
          '    #[test]', '    fn sifat_dasar() {',
          '        assert!(Material::Water.is_liquid());',
          '        assert!(!Material::Water.is_solid());',
          '        assert!(Material::Wood.burns());',
          '        assert!(!Material::Rock.burns());',
          '        assert_eq!(Material::Portal.mobility(), 2);',
          '        assert_eq!(Material::Piston.mobility(), 2);',
          '    }', '}', '']
    open(f'{out}/material.rs', 'w').write('\n'.join(o))

    # ---- blocks.rs
    kinds = []
    for p in blocks:
        if p[1] not in kinds: kinds.append(p[1])
    o = ['//! DIHASILKAN oleh tools/gen_blocks.py dari registri b1.7.3 asli (96 blok). Jangan edit manual.',
         '//! Perilaku (tick, interaksi) ada di modul terpisah per `BlockKind`; file ini hanya data statis.',
         '',
         'use crate::material::Material;', '',
         '#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]',
         'pub enum StepSound { Powder, Wood, Gravel, Grass, Stone, Metal, Glass, Cloth, Sand }',
         '',
         'impl StepSound {',
         '    /// Nama suara langkah ("step.xxx") dan pitch. Metal = stone dengan pitch 1.5.',
         '    pub fn step(self) -> (&\'static str, f32) {',
         '        match self {']
    for k, v in SND.items():
        o.append(f'            StepSound::{v[0]} => ("step.{v[1]}", {lit(v[2])}),')
    o += ['        }', '    }',
          '    /// Suara saat blok pecah/ditaruh (Java: stepSoundDir()).',
          '    pub fn break_sound(self) -> &\'static str {',
          '        match self {']
    for k, v in SND.items():
        o.append(f'            StepSound::{v[0]} => "{v[3]}",')
    o += ['        }', '    }', '}', '',
          '/// Kelas Java asal tiap blok; dipakai untuk dispatch perilaku.',
          '#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]',
          'pub enum BlockKind {']
    for k in kinds: o.append(f'    {k[5:] or "Plain"},' if k != 'Block' else '    Plain,')
    o += ['}', '',
          '#[derive(Clone, Copy, Debug)]',
          'pub struct BlockDef {',
          '    pub id: u8,',
          '    pub kind: BlockKind,',
          '    /// Kunci terjemahan ("tile.xxx"); None untuk blok internal piston.',
          '    pub name: Option<&\'static str>,',
          '    pub texture: u16,',
          '    pub material: Material,',
          '    /// -1 = tak bisa dihancurkan',
          '    pub hardness: f32,',
          '    /// Sudah dikali 3 seperti Java (setResistance)',
          '    pub resistance: f32,',
          '    pub light_value: u8,',
          '    pub light_opacity: u8,',
          '    /// Nilai awal. Daun (fancy graphics) dan slab ganda berubah saat runtime.',
          '    pub opaque: bool,',
          '    pub render_as_normal: bool,',
          '    /// 0 = kubus, 1 = silang, 2 = obor, 3 = api, 4 = cairan, 5 = kabel redstone, 6 = tanaman, 7 = pintu, 8 = tangga, 9 = rel, 10 = tangga blok, 11 = pagar, 12 = tuas, 13 = kaktus, 14 = ranjang, 15 = repeater, 16/17 = piston, -1 = khusus',
          '    pub render_type: i8,',
          '    pub collidable: bool,',
          '    pub sound: StepSound,',
          '    /// minX, minY, minZ, maxX, maxY, maxZ',
          '    pub bounds: [f32; 6],',
          '    pub tick_on_load: bool,',
          '    pub is_container: bool,',
          '    /// Java: canBlockGrass[] (sudah dibalik dari material)',
          '    pub can_block_grass: bool,',
          '    /// Java: field_28032_t (tidak memberi tahu tetangga saat metadata berubah)',
          '    pub no_neighbor_notify_on_meta: bool,',
          '    pub enable_stats: bool,',
          '    pub slipperiness: f32,',
          '    pub tick_rate: u8,',
          '}', '',
          'const DEFS: [BlockDef; %d] = [' % len(blocks)]
    for p in blocks:
        bid, cls, name, tex, mat_, hard, res, lv, op, opq, rn, rt, col, snd, bnd, tol, cont, cbg, nn, st, slip, tr = p
        kind = 'Plain' if cls == 'Block' else cls[5:]
        bn = [lit(x) for x in bnd.split(',')]
        nm = 'None' if name == 'null' else f'Some("{name}")'
        o.append(f'    BlockDef {{ id: {bid}, kind: BlockKind::{kind}, name: {nm}, texture: {tex}, material: Material::{MAT[mat_]}, hardness: {lit(hard)}, resistance: {lit(res)}, light_value: {lv}, light_opacity: {op}, opaque: {b(opq)}, render_as_normal: {b(rn)}, render_type: {rt}, collidable: {b(col)}, sound: StepSound::{SND[snd][0]}, bounds: [{", ".join(bn)}], tick_on_load: {b(tol)}, is_container: {b(cont)}, can_block_grass: {b(cbg)}, no_neighbor_notify_on_meta: {b(nn)}, enable_stats: {b(st)}, slipperiness: {lit(slip)}, tick_rate: {tr} }},')
    o += ['];', '',
          'pub static BLOCK_DEFS: [BlockDef; %d] = DEFS;' % len(blocks), '',
          '/// Tabel datar untuk jalur panas (pencahayaan, meshing). Indeks = ID blok (0 = udara).',
          'pub static LIGHT_OPACITY: [u8; 256] = build_light_opacity();',
          'pub static LIGHT_VALUE: [u8; 256] = build_light_value();',
          'pub static OPAQUE: [bool; 256] = build_opaque();',
          '',
          'const fn build_light_opacity() -> [u8; 256] {',
          '    let mut t = [0u8; 256];',
          '    let mut i = 0;',
          '    while i < DEFS.len() {',
          '        t[DEFS[i].id as usize] = DEFS[i].light_opacity;',
          '        i += 1;',
          '    }',
          '    t',
          '}', '',
          'const fn build_light_value() -> [u8; 256] {',
          '    let mut t = [0u8; 256];',
          '    let mut i = 0;',
          '    while i < DEFS.len() {',
          '        t[DEFS[i].id as usize] = DEFS[i].light_value;',
          '        i += 1;',
          '    }',
          '    t',
          '}', '',
          'const fn build_opaque() -> [bool; 256] {',
          '    let mut t = [false; 256];',
          '    let mut i = 0;',
          '    while i < DEFS.len() {',
          '        t[DEFS[i].id as usize] = DEFS[i].opaque;',
          '        i += 1;',
          '    }',
          '    t',
          '}', '',
          '/// Cari definisi blok. ID 0 (udara) dan ID > 96 mengembalikan None.',
          '#[inline]',
          'pub fn block(id: u8) -> Option<&\'static BlockDef> {',
          '    BLOCK_DEFS.get((id as usize).wrapping_sub(1))',
          '}', '',
          '#[cfg(test)]', 'mod tests {', '    use super::*;', '',
          '    #[test]', '    fn id_berurutan() {',
          '        for (i, d) in BLOCK_DEFS.iter().enumerate() {',
          '            assert_eq!(d.id as usize, i + 1);',
          '        }',
          '        assert_eq!(BLOCK_DEFS.len(), 96);',
          '    }', '',
          '    #[test]', '    fn nilai_dari_java() {',
          '        let stone = block(1).unwrap();',
          '        assert_eq!(stone.hardness, 1.5);',
          '        assert_eq!(stone.resistance, 30.0);',
          '        assert_eq!(block(7).unwrap().hardness, -1.0);',
          '        assert_eq!(block(50).unwrap().light_value, 14); // obor',
          '        assert_eq!(block(10).unwrap().light_value, 15); // lava',
          '        assert_eq!(LIGHT_OPACITY[8], 3);               // air',
          '        assert_eq!(LIGHT_OPACITY[1], 255);',
          '        assert!(OPAQUE[1] && !OPAQUE[20]);             // kaca tidak opak',
          '        assert!(block(0).is_none() && block(97).is_none());',
          '    }', '}', '']
    open(f'{out}/blocks.rs', 'w').write('\n'.join(o))
    print('blok:', len(blocks), 'jenis kelas:', len(kinds), 'material:', len(mats))

main(*sys.argv[1:4])
