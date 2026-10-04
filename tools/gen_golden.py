#!/usr/bin/env python3
"""Membuat mc-core/src/worldgen/golden.rs dari keluaran tools/GenDump.java (golden dari Java asli).
  for s in 12345 -987654321 0; do echo "### seed $s"; java -cp deobfuscated.jar tools/GenDump.java $s | grep -v counts | tr '\\n' ' ' | sed 's/chunk /\\nchunk /g'; done > golden.txt
  python3 tools/gen_golden.py golden.txt mc-core/src/worldgen/golden.rs
"""
import re, sys
seed = None
rows = []
for line in open(sys.argv[1]):
    m = re.match(r'### seed (-?\d+)', line)
    if m:
        seed = int(m.group(1)); continue
    m = re.match(r'chunk (-?\d+) (-?\d+)\s+stages (\w+) (\w+) (\w+) (\w+)\s+blocks (\w+)\s+height (\w+)\s+sky (\w+)\s+biome (\w+)', line)
    if m:
        g = m.groups()
        rows.append((seed, int(g[0]), int(g[1])) + tuple(g[2:]))
o = ['//! DIHASILKAN oleh tools/gen_golden.py dari ChunkProviderGenerate asli b1.7.3 (tools/GenDump.java).',
     '//! Hash FNV-1a 64 atas byte blok, heightmap, data skylight, dan string biome (huruf pertama + panjang nama).',
     '',
     '/// (seed, cx, cz, suhu, kelembapan, terrain mentah, setelah permukaan, blok akhir, heightmap, skylight, biome)',
     'pub type GoldenRow = (i64, i32, i32, u64, u64, u64, u64, u64, u64, u64, u64);',
     '',
     'pub static GOLDEN: [GoldenRow; %d] = [' % len(rows)]
for r in rows:
    o.append('    (%d, %d, %d, %s),' % (r[0], r[1], r[2], ', '.join('0x' + x for x in r[3:])))
o += ['];', '',
      '/// FNV-1a 64-bit, sama dengan GenDump.java.',
      'pub fn fnv1a64(data: &[u8]) -> u64 {',
      '    let mut h: u64 = 0xcbf29ce484222325;',
      '    for b in data {',
      '        h ^= *b as u64;',
      '        h = h.wrapping_mul(0x100000001b3);',
      '    }',
      '    h',
      '}', '',
      '/// FNV-1a 64-bit atas bit double (big-endian), sama dengan hexD di GenDump.java.',
      'pub fn fnv1a64_f64(data: &[f64]) -> u64 {',
      '    let mut h: u64 = 0xcbf29ce484222325;',
      '    for v in data {',
      '        for b in v.to_bits().to_be_bytes() {',
      '            h ^= b as u64;',
      '            h = h.wrapping_mul(0x100000001b3);',
      '        }',
      '    }',
      '    h',
      '}', '']
open(sys.argv[2], 'w').write('\n'.join(o))
print(len(rows), 'baris golden')
