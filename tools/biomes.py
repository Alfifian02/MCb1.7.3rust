"""Python reference of the multi-noise biome source (the same arithmetic as mc-rs/src/world/gen/chunk_manager.rs + world/biome.rs).

    python3 tools/biomes.py [scale] [cells] [step]   # biome shares, desert borders, patch size over a window (needs numpy)
    python3 tools/biomes.py check                    # the assertions of the Rust tests `chunk_manager::tests` on this reference

Uses the same java.util.Random seeding and a Perlin octave of the same shape (b1.7.3's gradients differ in a few constants, so the
two agree in distribution, not to the last digit)."""
import numpy as np, sys

class JR:
    M=0x5DEECE66D; K=(1<<48)-1
    def __init__(s,seed): s.s=((seed&((1<<64)-1))^s.M)&s.K
    def nxt(s,b):
        s.s=(s.s*s.M+0xB)&s.K; v=s.s>>(48-b)
        return v-(1<<b) if b==32 and v>=1<<31 else v
    def dbl(s): return ((s.nxt(26)<<27)+s.nxt(27))*(1.0/(1<<53))
    def bound(s,n):
        if n&-n==n: return (n*s.nxt(31))>>31
        while True:
            b=s.nxt(31); v=b%n
            if b-v+n-1>=0: return v

class Perlin:
    def __init__(s,r):
        s.xc,s.yc,s.zc=r.dbl()*256,r.dbl()*256,r.dbl()*256
        p=list(range(256))+[0]*256
        for i in range(256):
            j=r.bound(256-i)+i; p[i],p[j]=p[j],p[i]; p[i+256]=p[i]
        s.p=np.array(p,dtype=np.int64)
    def sample(s,x,y,z):
        x=x+s.xc; y=y+s.yc; z=z+s.zc
        xi=np.floor(x).astype(np.int64); yi=np.floor(y).astype(np.int64); zi=np.floor(z).astype(np.int64)
        x=x-xi; y=y-yi; z=z-zi; xi&=255; yi&=255; zi&=255
        f=lambda t:t*t*t*(t*(t*6-15)+10)
        u,v,w=f(x),f(y),f(z); p=s.p
        A=p[xi]+yi; AA=p[A]+zi; AB=p[A+1]+zi; B=p[xi+1]+yi; BA=p[B]+zi; BB=p[B+1]+zi
        def g(h,x,y,z):
            h=h&15; u=np.where(h<8,x,y); v=np.where(h<4,y,np.where((h==12)|(h==14),x,z))
            return np.where(h&1==0,u,-u)+np.where(h&2==0,v,-v)
        l=lambda t,a,b:a+t*(b-a)
        return l(w,l(v,l(u,g(p[AA],x,y,z),g(p[BA],x-1,y,z)),l(u,g(p[AB],x,y-1,z),g(p[BB],x-1,y-1,z))),
                   l(v,l(u,g(p[AA+1],x,y,z-1),g(p[BA+1],x-1,y,z-1)),l(u,g(p[AB+1],x,y-1,z-1),g(p[BB+1],x-1,y-1,z-1))))

class Normal:
    """Minecraft's NormalNoise: two octave stacks, the second read at 1.0181268882175227 x the coordinates."""
    def __init__(s,seed,first,amps):
        r=JR(seed); n=len(amps)
        mk=lambda:[Perlin(r) for _ in amps]
        s.a,s.b=mk(),mk()
        pers=2.0**(n-1)/(2.0**n-1)
        s.w=[(amp*pers/2**i,2.0**(first+i)) for i,amp in enumerate(amps)]
        nz=[i for i,a in enumerate(amps) if a!=0]
        s.f=(1/6)/(0.1*(1+1/(nz[-1]-nz[0]+1)))
    def get(s,x,z):
        t=0
        for k,(wgt,fr) in enumerate(s.w):
            if wgt==0: continue
            t=t+wgt*(s.a[k].sample(x*fr,0*x,z*fr)+s.b[k].sample(x*fr*1.0181268882175227,0*x,z*fr*1.0181268882175227))
        return t*s.f

SALT=dict(temp=0x1,hum=0x2,cont=0x3,eros=0x4,ridge=0x5,shift=0x6)
class Source:
    def __init__(s,seed,scale=1.0):
        s.scale=scale
        mk=lambda k,f,a:Normal(seed*0x9E3779B97F4A7C15+SALT[k],f,a)
        s.shift=mk('shift',-3,[1,1,1,0])
        s.t=mk('temp',-10,[1.5,0,1,0,0,0]); s.h=mk('hum',-8,[1,1,0,0,0,0])
        s.c=mk('cont',-9,[1,1,2,2,2,1,1,1,1]); s.e=mk('eros',-9,[1,1,0,1,1]); s.w=mk('ridge',-7,[1,2,1,0,0,0])
    def params(s,x,z):
        qx,qz=x*0.25*s.scale,z*0.25*s.scale
        sx=s.shift.get(qx,qz)*4; sz=s.shift.get(qz,qx)*4
        qx,qz=qx+sx,qz+sz
        # the spawn search only looks 32 blocks around the origin: land is lifted out to ~300 blocks (chunk_manager.rs `cell`)
        d=np.hypot(qx/s.scale,qz/s.scale)*4
        return s.t.get(qx,qz),s.h.get(qx,qz),s.c.get(qx,qz)+1.3*np.maximum(1-d/400,0),s.e.get(qx,qz),s.w.get(qx,qz)

TUNDRA,TAIGA,PLAINS,FOREST,SEASONAL,SHRUB,SAVANNA,RAIN,DESERT,SWAMP,OCEAN,BEACH,MOUNTAINS=range(13)
NAMES=['Tundra','Taiga','Plains','Forest','Seasonal','Shrub','Savanna','Rain','Desert','Swamp','Ocean','Beach','Mountains']
def lvl(v,edges): return int(np.searchsorted(edges,v,side='right'))
TE=[-0.45,-0.15,0.2,0.55]; HE=[-0.35,-0.1,0.1,0.3]; EE=[-0.78,-0.375,-0.2225,0.05,0.45,0.55]
MID=[[TUNDRA,TUNDRA,TUNDRA,TAIGA,TAIGA],
     [PLAINS,PLAINS,FOREST,TAIGA,TAIGA],
     [PLAINS,PLAINS,FOREST,FOREST,SEASONAL],
     [SAVANNA,SAVANNA,SHRUB,RAIN,RAIN],
     [DESERT]*5]
def biome(t,h,c,e,w):
    T,H,E=lvl(t,TE),lvl(h,HE),lvl(e,EE)
    pv=1-abs(3*abs(w)-2)
    if c<-0.19: return OCEAN
    if c<-0.11: return BEACH if T<4 else MID[T][H]
    if E==6 and pv<-0.2 and 1<=T<=2: return SWAMP
    if c>0.03 and E<=2 and pv>0.2: return MOUNTAINS
    return MID[T][H]

def biome_grid(t,h,c,e,w):
    T=np.searchsorted(TE,t,side='right'); H=np.searchsorted(HE,h,side='right'); E=np.searchsorted(EE,e,side='right')
    pv=1-abs(3*abs(w)-2)
    B=np.array(MID,dtype=np.int64)[T,H]
    B=np.where(c<-0.11,np.where(T<4,BEACH,B),B)
    B=np.where((c>=-0.11)&(c>0.03)&(E<=2)&(pv>0.2),MOUNTAINS,B)
    B=np.where((c>=-0.11)&(E==6)&(pv<-0.2)&(T>=1)&(T<=2),SWAMP,B)
    return np.where(c<-0.19,OCEAN,B)

LAND=[(-1.2,-40.0),(-0.455,-30.0),(-0.19,-10.0),(-0.11,-3.0),(0.03,0.0),(0.3,4.0),(1.0,10.0)]
def terrain(c,e,w):
    """(surface offset in blocks over b1.7.3's 68, hill steepness): chunk_manager.rs `terrain`."""
    sm=lambda x:(lambda x:x*x*(3-2*x))(np.clip(x,0,1))
    inland=sm((c+0.11)/0.41); pv=1-abs(3*abs(w)-2)
    ero=np.clip((0.05-e)/0.8,0,1)*inland; peak=np.clip((pv-0.2)/0.5,0,1)
    off=np.interp(c,[p[0] for p in LAND],[p[1] for p in LAND])+inland*4*pv+28*ero*(0.5+0.5*peak)
    return off,0.25+1.35*ero

def through(v,pts):
    return float(np.interp(v,[p[0] for p in pts],[p[1] for p in pts]))
TEMP01=[(-1,0),(-0.45,0.5),(-0.15,0.65),(0.2,0.8),(0.55,0.95),(1,1)]

def check():
    src=Source(0xCAFEBABE,1.0)
    xs=(np.arange(128)-64)*32.0
    X,Z=np.meshgrid(xs,xs,indexing='ij')
    t,h,c,e,w=src.params(X,Z)
    B=biome_grid(t,h,c,e,w)
    cool={PLAINS,FOREST,TAIGA,TUNDRA,SEASONAL,SWAMP}
    bad=0
    for a,b in ((B[:-1],B[1:]),(B[:,:-1],B[:,1:])):
        bad+=sum(1 for x,y in zip(a.ravel(),b.ravel()) if (x==DESERT and y in cool) or (y==DESERT and x in cool))
    kinds=len(set(B.ravel().tolist()))
    print('desert next to a non-warm biome:',bad,'| desert cells:',int((B==DESERT).sum()),'| biome kinds:',kinds)
    assert bad==0 and (B==DESERT).any() and kinds>=8
    t01=np.vectorize(lambda v:through(v,TEMP01))(t)
    assert ((B!=TUNDRA)|(t01<0.5)).all() and ((B!=DESERT)|(t01>0.9)).all()
    off,_=terrain(c,e,w); top=68+off
    assert (top[B==OCEAN]<64-6).all() and top[B==MOUNTAINS].mean()>top[~np.isin(B,[OCEAN,BEACH,MOUNTAINS])].mean()+6
    for seed in range(40):                      # land around the origin whatever the seed (Rust test `origin_is_land`)
        p=Source(seed,1.0).params(np.array([8.0,24.0,8.0,24.0]),np.array([8.0,8.0,24.0,24.0]))
        assert (biome_grid(*p)!=OCEAN).all() and (biome_grid(*p)!=BEACH).all(), seed
    print('check ok')

if __name__=='__main__' and sys.argv[1:]==['check']:
    check(); sys.exit()
if __name__=='__main__':
    scale=float(sys.argv[1]) if len(sys.argv)>1 else 1.0
    n=int(sys.argv[2]) if len(sys.argv)>2 else 512; step=float(sys.argv[3]) if len(sys.argv)>3 else 8
    src=Source(0xCAFEBABE,scale)
    xs=(np.arange(n)-n/2)*step
    X,Z=np.meshgrid(xs,xs,indexing='ij')
    t,h,c,e,w=src.params(X,Z)
    T=np.searchsorted(TE,t,side='right')
    B=biome_grid(t,h,c,e,w)
    print(f'scale {scale} window {n*step:.0f} blocks, step {step}')
    for nm,a in (('temp',t),('hum',h),('cont',c),('eros',e),('ridge',w)):
        print(f'  {nm}: mean {a.mean():+.2f} std {a.std():.2f} min {a.min():+.2f} max {a.max():+.2f}')
    off,_=terrain(c,e,w); top=68+off
    for k,nm in ((OCEAN,'ocean'),(BEACH,'beach'),(MOUNTAINS,'mountains')):
        if (B==k).any(): print(f'  surface of {nm}: mean y {top[B==k].mean():.0f} (min {top[B==k].min():.0f}, max {top[B==k].max():.0f})')
    ld=~np.isin(B,[OCEAN,BEACH,MOUNTAINS]); print(f'  surface of other land: mean y {top[ld].mean():.0f}')
    print('  T share',np.bincount(T.ravel(),minlength=5)/T.size)
    sh=np.bincount(B.ravel(),minlength=13)/B.size
    print('  biomes',{NAMES[i]:round(float(sh[i]),3) for i in range(13)})
    # a desert cell next to anything that is not desert/savanna/swamp-free neighbour
    bad=0;tot=0
    for a,b in ((B[:-1],B[1:]),(B[:,:-1],B[:,1:])):
        m=(a!=b); tot+=m.sum()
        d=((a==DESERT)&np.isin(b,[PLAINS,FOREST,TAIGA,TUNDRA,SEASONAL,SWAMP]))|((b==DESERT)&np.isin(a,[PLAINS,FOREST,TAIGA,TUNDRA,SEASONAL,SWAMP]))
        bad+=d.sum()
    print(f'  desert borders with a cool/temperate biome: {bad} of {tot} border pairs')
    # mean patch size: average run length along x
    runs=(B[:-1]!=B[1:]).sum()
    print(f'  average run along x: {B.size/(runs+n):.1f} cells = {B.size/(runs+n)*step:.0f} blocks')
    np.save('/home/claude/work/B.npy',B)
