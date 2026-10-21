#!/usr/bin/env python3
"""ETC2 RGBA8 (EAC alpha + ETC1-style RGB) encoder for the terrain part of the block atlas: `assets/terrain.rgba` -> `assets/terrain.etc2`
(3 mip levels, the same alpha-weighted 2x2 box filter as `render::atlas::mip`, blocks in row order, 16 bytes a block).

    python3 tools/gen_terrain_etc2.py            # writes the file, prints the size
    python3 tools/gen_terrain_etc2.py --verify   # also decodes it with Mesa (EGL + GLES3, llvmpipe) and prints PSNR per level

The bit layout (ETC1 individual / differential modes, flip, EAC tables, pixel order) was checked against Mesa's decoder: every block of
a test image decodes to exactly the pixels this file predicts. The encoder is plain numpy, ~20 ms a block. Modes NOT encoded: ETC2's
T, H and planar; the RGB error is therefore ETC1-class (28 dB PSNR at mip 0, 30 dB at mip 2, on terrain.png: see the roadmap entry). Not used by the game yet.
"""
import sys, itertools, numpy as np

import itertools

ETC1_T = np.array([[2,8],[5,17],[9,29],[13,42],[18,60],[24,80],[33,106],[47,183]], dtype=np.int32)
# modifier for pixel index (msb,lsb): 00 +t0, 01 +t1, 10 -t0, 11 -t1  -> index = msb*2+lsb
def mods(t): a,b=ETC1_T[t]; return np.array([a,b,-a,-b],dtype=np.int32)
MODS = np.stack([mods(t) for t in range(8)])  # (8,4)

EAC_T = np.array([
 [-3,-6,-9,-15,2,5,8,14],[-3,-7,-10,-13,2,6,9,12],[-2,-5,-8,-13,1,4,7,12],[-2,-4,-6,-13,1,3,5,12],
 [-3,-6,-8,-12,2,5,7,11],[-3,-7,-9,-11,2,6,8,10],[-4,-7,-8,-11,3,6,7,10],[-3,-5,-8,-11,2,4,7,10],
 [-2,-6,-8,-10,1,5,7,9],[-2,-5,-8,-10,1,4,7,9],[-2,-4,-8,-10,1,3,7,9],[-2,-5,-7,-10,1,4,6,9],
 [-3,-4,-7,-10,2,3,6,9],[-1,-2,-3,-10,0,1,2,9],[-4,-6,-8,-9,3,5,7,8],[-3,-5,-7,-9,2,4,6,8]], dtype=np.int32)

def ex5(v): return (v<<3)|(v>>2)
def ex4(v): return (v<<4)|v

# ---- pixel order: pixel (x,y) -> idx = x*4+y (column major)
PIX = [(x,y) for x in range(4) for y in range(4)]  # idx -> (x,y)

def sub_pixels(flip):
    """list of two lists of (x,y) pixels"""
    if flip==0: return [[(x,y) for x in range(2) for y in range(4)], [(x,y) for x in range(2,4) for y in range(4)]]
    return [[(x,y) for x in range(4) for y in range(2)], [(x,y) for x in range(4) for y in range(2,4)]]

def table_errors(px, bases):
    """px (n,3), bases (m,3) -> err (m,), table (m,), idx (m,n): the best table and pixel indices for every base."""
    v = np.clip(bases[:,None,None,:] + MODS[None,:,:,None], 0, 255)          # (m,8,4,3)
    e = ((v[:,:,None,:,:] - px[None,None,:,None,:])**2).sum(-1)               # (m,8,n,4)
    k = e.argmin(3); tot = e.min(3).sum(2)                                    # (m,8,n), (m,8)
    t = tot.argmin(1); r = np.arange(len(bases))
    return tot[r,t], t, k[r,t]

def cands(mean, bits, rad):
    s=(1<<bits)-1
    c0=np.clip(np.round(mean/255.0*s).astype(int),0,s)
    rng=range(-rad,rad+1)
    out=[tuple(c0+np.array(d)) for d in itertools.product(rng,rng,rng)]
    return np.array([c for c in out if min(c)>=0 and max(c)<=s],dtype=np.int32)

def enc_rgb(blk):
    """blk: (4,4,3) [y][x] ints -> 8 bytes, error"""
    best=None
    for flip in (0,1):
        subs=sub_pixels(flip)
        data=[np.array([blk[y][x] for (x,y) in s],dtype=np.int32) for s in subs]
        means=[d.mean(0) for d in data]
        res={}
        for mode,bits,rad,ex in (('i',4,1,ex4),('d',5,2,ex5)):
            per=[]
            for d,mn in zip(data,means):
                c=cands(mn,bits,rad); base=ex(c); e,t,k=table_errors(d,base); per.append((c,e,t,k))
            res[mode]=per
        (ca,ea,ta,ka),(cb,eb,tb,kb)=res['i']
        i,j=ea.argmin(),eb.argmin()
        tot=ea[i]+eb[j]
        if best is None or tot<best[0]: best=(tot,'i',flip,[(ea[i],tuple(ca[i]),ta[i],ka[i]),(eb[j],tuple(cb[j]),tb[j],kb[j])])
        (ca,ea,ta,ka),(cb,eb,tb,kb)=res['d']
        dl=cb[None,:,:]-ca[:,None,:]
        ok=((dl>=-4)&(dl<=3)).all(2)
        tt=np.where(ok,ea[:,None]+eb[None,:],1<<40)
        i,j=np.unravel_index(tt.argmin(),tt.shape)
        if tt[i,j]<(1<<40) and (best is None or tt[i,j]<best[0]):
            best=(tt[i,j],'d',flip,[(ea[i],tuple(ca[i]),ta[i],ka[i]),(eb[j],tuple(cb[j]),tb[j],kb[j])])
    tot,mode,flip,sub=best
    return pack_rgb(mode,flip,sub), tot

def pack_rgb(mode,flip,sub):
    (e1,c1,t1,k1),(e2,c2,t2,k2)=sub
    w=0
    c1=tuple(int(v) for v in c1); c2=tuple(int(v) for v in c2); t1=int(t1); t2=int(t2)
    if mode=='i':
        w|= (c1[0]<<60)|(c2[0]<<56)|(c1[1]<<52)|(c2[1]<<48)|(c1[2]<<44)|(c2[2]<<40)
    else:
        d=[(c2[i]-c1[i])&7 for i in range(3)]
        w|= (c1[0]<<59)|(d[0]<<56)|(c1[1]<<51)|(d[1]<<48)|(c1[2]<<43)|(d[2]<<40)
        w|= 1<<33
    w|=(t1<<37)|(t2<<34)|(flip<<32)
    subs=sub_pixels(flip)
    for s,k in zip(subs,(k1,k2)):
        for (x,y),kk in zip(s,k):
            idx=x*4+y
            kk=int(kk); msb=(kk>>1)&1; lsb=kk&1
            w|=(msb<<(16+idx))|(lsb<<idx)
    return w.to_bytes(8,'big')

def dec_rgb(b):
    w=int.from_bytes(b,'big'); out=np.zeros((4,4,3),dtype=np.int32)
    flip=(w>>32)&1; diff=(w>>33)&1; t1=(w>>37)&7; t2=(w>>34)&7
    if diff:
        r=(w>>59)&31; g=(w>>51)&31; bl=(w>>43)&31
        def sg(v): return v-8 if v>=4 else v
        r2=r+sg((w>>56)&7); g2=g+sg((w>>48)&7); b2=bl+sg((w>>40)&7)
        base=[np.array([ex5(r),ex5(g),ex5(bl)]),np.array([ex5(r2),ex5(g2),ex5(b2)])]
    else:
        base=[np.array([ex4((w>>60)&15),ex4((w>>52)&15),ex4((w>>44)&15)]),np.array([ex4((w>>56)&15),ex4((w>>48)&15),ex4((w>>40)&15)])]
    for si,s in enumerate(sub_pixels(flip)):
        t=(t1,t2)[si]
        for (x,y) in s:
            idx=x*4+y; k=(((w>>(16+idx))&1)<<1)|((w>>idx)&1)
            out[y][x]=np.clip(base[si]+MODS[t][k],0,255)
    return out

# ---- EAC alpha
def eac_pack(base,mult,table,idxs):
    w=(base<<56)|(mult<<52)|(table<<48)
    for i,k in enumerate(idxs): # idx = x*4+y order, first pixel in the top bits
        w|=k<<(45-3*i)
    return w.to_bytes(8,'big')

def eac_dec(b):
    w=int.from_bytes(b,'big'); base=(w>>56)&255; mult=(w>>52)&15; table=(w>>48)&15
    out=np.zeros(16,dtype=np.int32)
    for i in range(16):
        k=(w>>(45-3*i))&7
        out[i]=min(255,max(0,base+EAC_T[table][k]*mult))
    return out  # indexed by x*4+y

def enc_alpha(a):
    """a: 16 values indexed by x*4+y -> 8 bytes, err"""
    a=np.asarray(a,dtype=np.int32)
    if (a==a[0]).all():
        # table 13 has a 0 modifier (index 4)
        return eac_pack(int(a[0]),1,13,[4]*16),0
    if set(np.unique(a))<={0,255}:
        # base 128, mult 15, table 13: -10*15 -> 0, +9*15 -> 255 (both clamp)
        return eac_pack(128,15,13,[3 if v==0 else 7 for v in a]),0
    best=None
    bases=np.arange(0,256)
    for t in range(16):
        for m in range(1,16):
            o=EAC_T[t]*m                                  # (8,)
            v=np.clip(bases[:,None]+o[None,:],0,255)      # (256,8)
            e=((v[:,None,:]-a[None,:,None])**2)           # (256,16,8)
            k=e.argmin(2); er=np.take_along_axis(e,k[:,:,None],2)[:,:,0].sum(1)
            bi=er.argmin()
            if best is None or er[bi]<best[0]: best=(er[bi],int(bases[bi]),m,t,k[bi].tolist())
    return eac_pack(best[1],best[2],best[3],best[4]),best[0]

def encode_image(rgba):
    """rgba: (h,w,4) uint8, h,w multiples of 4 -> bytes (block rows top to bottom), also returns the summed rgb error"""
    h,w,_=rgba.shape; out=bytearray(); err=0
    for by in range(0,h,4):
        for bx in range(0,w,4):
            blk=rgba[by:by+4,bx:bx+4].astype(np.int32)
            a=[blk[y][x][3] for x in range(4) for y in range(4)]
            ab,_=enc_alpha(a)
            rb,e=enc_rgb(blk[:,:,:3]); err+=e
            out+=ab+rb
    return bytes(out),err


def mip(src):
    """`render::atlas::mip`: 2x2 box filter, colour weighted by alpha, alpha averaged (integer maths)."""
    h,w,_=src.shape; s=src.astype(np.int64)
    a=s[:,:,3]; c=s[:,:,:3]*a[:,:,None]
    def q(x): return x[0::2,0::2]+x[1::2,0::2]+x[0::2,1::2]+x[1::2,1::2]
    asum=q(a); d=np.maximum(asum,1)
    out=np.zeros((h//2,w//2,4),dtype=np.uint8)
    for i in range(3): out[:,:,i]=(q(c[:,:,i])//d).astype(np.uint8)
    out[:,:,3]=(asum//4).astype(np.uint8)
    return out

def main():
    import os
    root=os.path.join(os.path.dirname(os.path.abspath(__file__)),'..','mc-rs','assets')
    t=np.fromfile(os.path.join(root,'terrain.rgba'),dtype=np.uint8).reshape(256,256,4)
    levels=[t]
    for _ in range(2): levels.append(mip(levels[-1]))
    blobs=[encode_image(l)[0] for l in levels]
    open(os.path.join(root,'terrain.etc2'),'wb').write(b''.join(blobs))
    print('terrain.etc2:',sum(map(len,blobs)),'bytes (RGBA8 with mips:',sum(l.size for l in levels),')')
    if '--verify' in sys.argv:
        sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
        import mesa_dec as M
        dec=M.decode(blobs,256,256,0x9278)
        for i,(d,l) in enumerate(zip(dec,levels)):
            mse=((d[:,:,:3].astype(float)-l[:,:,:3])**2).mean()
            print(f'level {i}: rgb PSNR {10*np.log10(255**2/max(mse,1e-9)):.1f} dB, alpha equal {100*(d[:,:,3]==l[:,:,3]).mean():.1f} %')

if __name__=='__main__':
    main()
