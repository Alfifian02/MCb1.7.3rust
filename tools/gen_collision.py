#!/usr/bin/env python3
"""Membuat mc-core/src/collision_data.rs dari dump tools/DumpCollision.java.
  java -cp deobfuscated.jar tools/DumpCollision.java > coll.txt
  python3 tools/gen_collision.py coll.txt mc-core/src
"""
import re, sys

def lit(s):
    v = float(s)
    r = repr(v)
    return r

rows = [l.strip().split('|') for l in open(sys.argv[1]) if re.match(r'^\d+\|', l)]
rows.sort(key=lambda r: int(r[0]))
assert [int(r[0]) for r in rows] == list(range(1, len(rows) + 1))
o = ['//! DIHASILKAN oleh tools/gen_collision.py dari registri b1.7.3 asli. Jangan edit manual.',
     '//! Bentuk tabrakan statis per blok (relatif terhadap sudut blok) dan mask canCollideCheck.',
     '',
     '#[derive(Clone, Copy, Debug, PartialEq)]',
     'pub enum CollisionShape {',
     '    /// getCollisionBoundingBoxFromPool mengembalikan null',
     '    None,',
     '    /// Kotak statis: minX, minY, minZ, maxX, maxY, maxZ',
     '    Box([f64; 6]),',
     '    /// Bergantung pada World/metadata: harus disediakan lewat `CollisionBehaviors`',
     '    Dynamic,',
     '}',
     '',
     'pub static COLLISION_SHAPES: [CollisionShape; %d] = [' % len(rows)]
for r in rows:
    c = r[2]
    if c == 'null': s = 'CollisionShape::None'
    elif c == 'NEEDS_WORLD': s = 'CollisionShape::Dynamic'
    else: s = 'CollisionShape::Box([%s])' % ', '.join(lit(x) for x in c.split(','))
    o.append(f'    {s},  // {r[0]} {r[1]}')
o += ['];', '',
      '/// Bit (meta * 2 + flag) = canCollideCheck(meta, flag). flag = "boleh berhenti di cairan".',
      'pub static COLLIDE_CHECK: [u32; %d] = [' % len(rows)]
for r in rows:
    o.append(f'    {int(r[4]):#010x},  // {r[0]} {r[1]}')
o += ['];', '']
open(sys.argv[2] + '/collision_data.rs', 'w').write('\n'.join(o))
print(len(rows), 'blok')
