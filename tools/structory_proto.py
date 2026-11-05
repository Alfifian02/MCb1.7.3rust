#!/usr/bin/env python3
"""Reference for `world/gen/structory.rs`: reads `structory.bin` (layout of `gen_structory.py`) and assembles structures the way the
Rust does (JigsawPlacement of 1.20, from memory: UNVERIFIED against the Java). `python3 tools/structory_proto.py [bin]` prints, for a few
seeds, the structures that start in a window and how many pieces each has; asserts that no two pieces overlap and that every piece is inside
`max_distance_from_center` -- the one runnable check of the assembly logic.
"""
import struct, sys, os

class Rd:
    def __init__(s, b): s.b, s.i = b, 0
    def u8(s): v = s.b[s.i]; s.i += 1; return v
    def u16(s): v = struct.unpack_from('<H', s.b, s.i)[0]; s.i += 2; return v
    def i16(s): v = struct.unpack_from('<h', s.b, s.i)[0]; s.i += 2; return v
    def u32(s): v = struct.unpack_from('<I', s.b, s.i)[0]; s.i += 4; return v

class Rand:
    """java.util.Random."""
    def __init__(s, seed): s.seed = (seed ^ 0x5DEECE66D) & ((1 << 48) - 1)
    def next(s, bits):
        s.seed = (s.seed * 0x5DEECE66D + 0xB) & ((1 << 48) - 1)
        v = s.seed >> (48 - bits)
        return v - (1 << bits) if v >= 1 << (bits - 1) and bits == 32 else v
    def int(s, bound):
        if bound & -bound == bound: return (bound * s.next(31)) >> 31
        while True:
            bits = s.next(31); val = bits % bound
            if bits - val + (bound - 1) < (1 << 31): return val
    def float(s): return s.next(24) / float(1 << 24)

def shuffle(l, r):                       # Util.shuffle
    for i in range(len(l), 1, -1):
        j = r.int(i); l[i - 1], l[j] = l[j], l[i - 1]
    return l

def load(path):
    r = Rd(open(path, 'rb').read()); assert r.b[:4] == b'STRY'; r.i = 5
    D = {}
    D['beta'] = [(r.u8(), r.u8()) for _ in range(r.u16())]
    n = r.u16(); strs = []
    for _ in range(n):
        l = r.u8(); strs.append(r.b[r.i:r.i + l].decode()); r.i += l
    D['strs'] = strs
    T = []
    for _ in range(r.u16()):
        sx, sy, sz = r.u8(), r.u8(), r.u8()
        cells = struct.unpack_from('<%dH' % (sx * sy * sz), r.b, r.i); r.i += 2 * sx * sy * sz
        jig = [dict(pos=(r.u8(), r.u8(), r.u8()), front=r.u8(), top=r.u8(), joint=r.u8(), name=r.u16(), target=r.u16(), pool=r.u16()) for _ in range(r.u16())]
        ents = [(r.u8(), r.u8(), r.u8(), r.u8(), r.u16()) for _ in range(r.u16())]
        T.append(dict(size=(sx, sy, sz), cells=cells, jig=jig, ents=ents))
    D['t'] = T
    P = []
    for _ in range(r.u16()):
        procs = []
        for _ in range(r.u8()):
            per = {}
            for _ in range(r.u16()):
                st = r.u16(); per[st] = [(r.u16(), r.u8(), r.u16()) for _ in range(r.u8())]
            procs.append(per)
        P.append(procs)
    D['proc'] = P
    pools = []
    for _ in range(r.u16()):
        fb = r.u16(); els = [dict(w=r.u8(), kind=r.u8(), t=r.u16(), proc=r.u8(), proj=r.u8()) for _ in range(r.u8())]
        pools.append(dict(fb=fb, els=els))
    D['pools'] = pools
    D['structs'] = [dict(mask=r.u32(), pool=r.u16(), size=r.u8(), adapt=r.u8(), y=r.i16(), dist=r.u8(), hack=r.u8()) for _ in range(r.u8())]
    sets = []
    for _ in range(r.u8()):
        s = dict(salt=r.u32(), spacing=r.u8(), sep=r.u8(), tri=r.u8(), freq=r.u16()); s['entries'] = [(r.u8(), r.u8()) for _ in range(r.u8())]
        sets.append(s)
    D['sets'] = sets
    return D

# ---- geometry (Rotation about the origin: cw90 (x, z) -> (-z, x))
def rot_xz(x, z, k):
    for _ in range(k): x, z = -z, x
    return x, z
def rot_dir(d, k):                        # 0 down 1 up 2 north 3 south 4 west 5 east
    if d < 2: return d
    for _ in range(k): d = {2: 5, 5: 3, 3: 4, 4: 2}[d]
    return d
STEP = {0: (0, -1, 0), 1: (0, 1, 0), 2: (0, 0, -1), 3: (0, 0, 1), 4: (-1, 0, 0), 5: (1, 0, 0)}
OPP = {0: 1, 1: 0, 2: 3, 3: 2, 4: 5, 5: 4}

def box_of(size, rot, pos):
    sx, sy, sz = size
    a = rot_xz(0, 0, rot); b = rot_xz(sx - 1, sz - 1, rot)
    return (pos[0] + min(a[0], b[0]), pos[1], pos[2] + min(a[1], b[1]), pos[0] + max(a[0], b[0]), pos[1] + sy - 1, pos[2] + max(a[1], b[1]))

def inside(b, p): return b[0] <= p[0] <= b[3] and b[1] <= p[1] <= b[4] and b[2] <= p[2] <= b[5]
def inter(a, b, deflate=0): return a[0] <= b[3] and a[3] >= b[0] and a[1] <= b[4] and a[4] >= b[1] and a[2] <= b[5] and a[5] >= b[2]

def jigsaws(D, t, rot, pos):
    out = []
    for j in D['t'][t]['jig']:
        x, y, z = j['pos']; rx, rz = rot_xz(x, z, rot)
        out.append(dict(pos=(pos[0] + rx, pos[1] + y, pos[2] + rz), front=rot_dir(j['front'], rot), top=rot_dir(j['top'], rot), joint=j['joint'],
                        name=j['name'], target=j['target'], pool=j['pool']))
    return out

def can_attach(a, b):
    return a['front'] == OPP[b['front']] and (a['joint'] == 0 or a['top'] == b['top']) and a['target'] == b['name']

def fits(shape, bx):
    """`Shapes.joinIsNotEmpty(free, box, ONLY_SECOND)` is false: the box lies in the shape's outer box and meets none of its holes."""
    outer, holes = shape
    return (outer[0] <= bx[0] and bx[3] <= outer[3] and outer[1] <= bx[1] and bx[4] <= outer[4] and outer[2] <= bx[2] and bx[5] <= outer[5]
            and not any(inter(bx, h) for h in holes))

def assemble(D, st, cx, cz, rnd, ground, height=128, off=(0, 0)):
    """Pieces of structure `st` starting in chunk (cx, cz). `ground(x, z)` = first free height. Returns a list of piece dicts."""
    S = D['structs'][st]; pools = D['pools']
    def flat(ix): return [e for e in pools[ix]['els'] for _ in range(e['w'])]
    def pool_max(ix):                        # StructureTemplatePool.getMaxSize: the tallest template
        return max([D['t'][e['t']]['size'][1] for e in pools[ix]['els'] if e['kind']] or [0])
    start = (lambda l: l[rnd.int(len(l))] if l else None)(flat(S['pool']))
    if start is None or start['kind'] == 0: return []
    rot = rnd.int(4)
    bx, bz = cx * 16 + off[0], cz * 16 + off[1]
    size = D['t'][start['t']]['size']
    b0 = box_of(size, rot, (bx, 0, bz))
    ctr = lambda b: (b[0] + (b[3] - b[0] + 1) // 2, b[2] + (b[5] - b[2] + 1) // 2)           # BoundingBox.getCenter
    c0 = ctr(b0)
    pos = (bx - (c0[0] - bx), 0, bz - (c0[1] - bz))   # the box's centre goes to the chunk's corner
    box = box_of(size, rot, pos)
    gx, gz = ctr(box)
    pos = (pos[0], S['y'] + ground(gx, gz), pos[2]); box = box_of(size, rot, pos)
    p0 = dict(t=start['t'], rot=rot, pos=pos, box=box, rigid=start['proj'] == 0, kind=start['kind'], proc=start['proc'], terrain=start['proj'] == 1)
    pieces = [p0]
    dist = S['dist']
    free = [((gx - dist, 0, gz - dist, gx + dist, height - 1, gz + dist), [box])]
    queue = [(p0, 0)]
    while queue:
        pc, depth = queue.pop(0)
        pb = pc['box']; rigid = pc['rigid']; k = None; inner = None
        for jg in shuffle(jigsaws(D, pc['t'], pc['rot'], pc['pos']), rnd):
            jpos = jg['pos']; d = STEP[jg['front']]; tpos = (jpos[0] + d[0], jpos[1] + d[1], jpos[2] + d[2])
            j = jpos[1] - pb[1]
            if jg['pool'] == 0xFFFF: continue
            pool = pools[jg['pool']]; fbx = pool['fb']
            if inside(pb, tpos):
                if inner is None: inner = (pb, [])
                shape = inner
            else: shape = free[0]
            cand = []
            if depth != S['size']: cand += shuffle(flat(jg['pool']), rnd)
            if fbx != 0xFFFF: cand += shuffle(flat(fbx), rnd)
            placed = False
            for c in cand:
                if c['kind'] == 0: break
                for r2 in shuffle([0, 1, 2, 3], rnd):
                    tj = jigsaws(D, c['t'], r2, (0, 0, 0)); cs = D['t'][c['t']]['size']; cb = box_of(cs, r2, (0, 0, 0))
                    i1 = 0
                    if S['hack'] and cb[4] - cb[1] + 1 <= 16:
                        for ij in tj:
                            f = STEP[ij['front']]
                            if inside(cb, (ij['pos'][0] + f[0], ij['pos'][1] + f[1], ij['pos'][2] + f[2])) and ij['pool'] != 0xFFFF:
                                p_ = pools[ij['pool']]
                                i1 = max(i1, pool_max(ij['pool']), pool_max(p_['fb']) if p_['fb'] != 0xFFFF else 0)
                    for cj in tj:
                        if not can_attach(jg, cj): continue
                        cp = cj['pos']; off = (tpos[0] - cp[0], tpos[1] - cp[1], tpos[2] - cp[2])
                        cb2 = box_of(cs, r2, off)
                        crigid = c['proj'] == 0
                        l1 = j - cp[1] + STEP[jg['front']][1]
                        if rigid and crigid: i2 = pb[1] + l1
                        else:
                            if k is None: k = ground(jpos[0], jpos[2])
                            i2 = k - cp[1]
                        j2 = i2 - cb2[1]
                        cb3 = (cb2[0], cb2[1] + j2, cb2[2], cb2[3], cb2[4] + j2, cb2[5])
                        if i1 > 0: cb3 = cb3[:4] + (max(cb3[4], cb3[1] + max(i1 + 1, cb3[4] - cb3[1] + 1) - 1),) + cb3[5:]
                        if fits(shape, cb3):
                            shape[1].append(cb3)
                            np_ = dict(t=c['t'], rot=r2, pos=(off[0], off[1] + j2, off[2]), box=cb3, rigid=crigid, kind=c['kind'], proc=c['proc'], terrain=not crigid)
                            pieces.append(np_)
                            if depth + 1 <= S['size']: queue.append((np_, depth + 1))
                            placed = True; break
                    if placed: break
                if placed: break
    return pieces

# ---- the site (twin of `judge`, `blend`, `Site::height`, `shifts` in structory.rs): move to flat dry ground, level what is left
PAD, MARGIN, FLAT, SIDE, CLIFF, SAFE, SHIFT, SHIFT_STEP = 2, 16, 2, 6, 2, 4, 48, 8
def aquatic(D, st, wet_bits=(1 << 10) | (1 << 1) | (1 << 15)): return D['structs'][st]['mask'] & ~wet_bits == 0   # Ocean, Swamp, Marsh
def shifts(): n = SHIFT // SHIFT_STEP; return sorted(((i * SHIFT_STEP, j * SHIFT_STEP) for i in range(-n, n + 1) for j in range(-n, n + 1)), key=lambda o: (o[0] ** 2 + o[1] ** 2, o))
def blend(t, n, k):
    u = min(max(int(k / MARGIN * 1024), 0), 1024); s = (u * u * (3072 - 2 * u)) >> 20
    return t + ((n - t) * s + 512) // 1024
def rdist(r, x, z): return ((max(r[0] - x, 0, x - r[2])) ** 2 + (max(r[1] - z, 0, z - r[3])) ** 2) ** 0.5
def every2(a, b): return list(range(a, b + 1, 2)) + ([b] if b > a and (b - a) % 2 else [])
def site_height(rigid, target, x, z, n):
    d = min(rdist(r, x, z) for r in rigid)
    return None if d > PAD + MARGIN else blend(target, n, max(d - PAD, 0))
def judge(pieces, target, flat, at):
    """at(x, z) -> (height, wet). Returns the rigid rects, or None when the site is refused."""
    rigid = [(p['box'][0], p['box'][2], p['box'][3], p['box'][5]) for p in pieces if p['rigid']]
    if not (flat and rigid): return []
    reach = PAD + MARGIN
    for r in rigid:
        for x in every2(r[0] - reach - SAFE, r[2] + reach + SAFE):
            for z in every2(r[1] - reach - SAFE, r[3] + reach + SAFE):
                n, wet = at(x, z)
                if wet: return None
                d = rdist(r, x, z); dev = abs(n - target)
                if (d <= PAD and dev > FLAT) or (d <= reach and dev > SIDE): return None
                if d <= reach and (abs(at(x + 1, z)[0] - n) > CLIFF or abs(at(x, z + 1)[0] - n) > CLIFF): return None
    return rigid
def place(D, st, cx, cz, seed, at):
    """The first shift whose assembled rigid pieces `judge` accepts: (offset, pieces, rigid rects, target) or None."""
    for off in (shifts() if not aquatic(D, st) else [(0, 0)]):
        ps = assemble(D, st, cx, cz, Rand(seed), lambda x, z: at(x, z)[0], off=off)
        if not ps: return None
        base = lambda t: 0                                   # the start's baked ground depth is the template's; the proto assumes none
        target = ps[0]['pos'][1] - D['structs'][st]['y'] + base(ps[0]['t'])
        rigid = judge(ps, target, not aquatic(D, st), at)
        if rigid is not None: return off, ps, rigid, target
    return None

def spread(seed, rx, rz, sp):
    r = Rand((rx * 341873128712 + rz * 132897987541 + seed + sp['salt']) & ((1 << 64) - 1))
    k = sp['spacing'] - sp['sep']
    f = (lambda: (r.int(k) + r.int(k)) // 2) if sp['tri'] else (lambda: r.int(k))
    a = f(); b = f()
    return rx * sp['spacing'] + a, rz * sp['spacing'] + b

if __name__ == '__main__':
    D = load(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), '..', 'mc-rs', 'assets', 'structory.bin'))
    print(len(D['t']), 'templates;', len(D['pools']), 'pools;', len(D['structs']), 'structures')
    ground = lambda x, z: 64
    total = 0
    for st in range(len(D['structs'])):
        counts = []
        for seed in range(25):
            ps = assemble(D, st, 100 + seed, -40 + seed, Rand(seed * 7919 + st), ground)
            counts.append(len(ps)); total += len(ps)
            S = D['structs'][st]; c0 = ps[0]['box'] if ps else None
            for a in ps:                                                  # every piece stays inside the max-distance area, y inside the world
                assert a['box'][1] >= 0 and a['box'][4] < 128, ('y', st, a['box'])
                cx_, cz_ = c0[0] + (c0[3] - c0[0] + 1) // 2, c0[2] + (c0[5] - c0[2] + 1) // 2
                assert abs(a['box'][0] - cx_) <= S['dist'] and abs(a['box'][3] - cx_) <= S['dist'] + 1 and abs(a['box'][2] - cz_) <= S['dist'] and abs(a['box'][5] - cz_) <= S['dist'] + 1, ('dist', st)
        print('structure %2d  pieces over 25 starts: min %d max %d mean %.1f' % (st, min(counts), max(counts), sum(counts) / len(counts)))
    print('total pieces', total)
    # the site search on a ramp between x = 260 and 300 (plateau west, flat and dry east of it) with a river along x = 420: nothing may end on the ramp or the river
    at = lambda x, z: (64 + max(0, min(40, 300 - x)) // 2, abs(x - 420) <= 3)
    for st in range(len(D['structs'])):
        if aquatic(D, st): continue
        found = [place(D, st, 17, 6, 100 + i, at) for i in range(6)]
        for f in found:
            if f is None: continue
            off, ps, rigid, target = f
            assert all(abs(at(x, z)[0] - target) <= FLAT for r in rigid for x in every2(r[0] - PAD, r[2] + PAD) for z in every2(r[1] - PAD, r[3] + PAD)), st
            assert not any(at(x, z)[1] for r in rigid for x in every2(r[0] - 22, r[2] + 22) for z in every2(r[1] - 22, r[3] + 22)), st
            for r in rigid[:3]:                                # the levelled ground: the target on the pad, between target and land in the margin, never a jump
                for x in range(r[0] - PAD - MARGIN - 1, r[2] + PAD + MARGIN + 2, 3):
                    row = []
                    for z in range(r[1] - PAD - MARGIN - 1, r[3] + PAD + MARGIN + 2, 3):
                        n = at(x, z)[0]; h = site_height(rigid, target, x, z, n); d = min(rdist(q, x, z) for q in rigid)
                        if h is None: assert d > PAD + MARGIN; continue
                        assert min(n, target) <= h <= max(n, target), (st, x, z, n, h, target)
                        if d <= PAD: assert h == target
                        if d >= PAD + MARGIN: assert h == n
                        row.append(h)
                    assert all(abs(a - b) <= 4 for a, b in zip(row, row[1:])), (st, row)   # 3 blocks apart, on a 0.5 slope: no wall
        print('structure %2d  sites %d/6 accepted, shifted %s' % (st, sum(f is not None for f in found), sorted({f[0] for f in found if f})[:4]))
