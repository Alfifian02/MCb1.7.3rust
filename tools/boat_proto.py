"""Python twin of world::boat::Boat::drive (horizontal part only) to pick the test thresholds."""
import math
TURN, MAXA, MAXB, KEEL = 3.5, 0.30, 0.12, 0.82
class B:
    def __init__(s): s.x=s.z=32.0; s.vx=s.vz=0.0; s.yaw=0.0; s.th=s.st=0.0
    def h(s): return math.radians(s.yaw-90)
    def tick(s,f,side):
        s.th+=(f-s.th)*0.25; s.st+=(side-s.st)*0.25
        sh,ch=math.sin(s.h()),math.cos(s.h())
        al=s.vx*sh-s.vz*ch
        s.yaw+=s.st*TURN*(0.4+0.6*min(abs(al)/0.15,1))
        sh,ch=math.sin(s.h()),math.cos(s.h())
        al,ac=s.vx*sh-s.vz*ch, s.vx*ch+s.vz*sh
        t=s.th*(MAXA if s.th>=0 else MAXB)
        al+=(t-al)*(0.06 if abs(t)>abs(al) else 0.035)
        ac*=KEEL
        s.vx=sh*al+ch*ac; s.vz=-ch*al+sh*ac
        s.vx=max(-.4,min(.4,s.vx)); s.vz=max(-.4,min(.4,s.vz))
        s.x+=s.vx; s.z+=s.vz
def run(f,side,n,start_heading=0.0):
    b=B(); b.yaw=math.degrees(start_heading)+90
    for _ in range(n): b.tick(f,side)
    return b
b=run(1,0,80); print("fwd 80t: dz",b.z-32,"speed",math.hypot(b.vx,b.vz),"heading deg",math.degrees(b.h()))
b=run(1,0,20); print("fwd 20t: dz",b.z-32,"speed",math.hypot(b.vx,b.vz))
b=run(-1,0,80); print("back 80t: dz",b.z-32,"speed",math.hypot(b.vx,b.vz))
b=run(0,1,40); print("turn only 40t: heading",math.degrees(b.h()),"moved",math.hypot(b.x-32,b.z-32))
d=0.7071
b=run(d,d,120); print("fwd+right 120t: heading",math.degrees(b.h()),"dx",b.x-32,"dz",b.z-32,"speed",math.hypot(b.vx,b.vz))
ang=math.degrees(math.atan2(b.vx,-b.vz)); print("  vel dir",ang,"vs heading",math.degrees(b.h())%360)
b=run(1,0,60);
for _ in range(60): b.tick(0,0)
print("coast 60t after 60 fwd: speed",math.hypot(b.vx,b.vz))
# smoothness: max per-tick change of heading rate and speed
b=B(); b.yaw=90; prev=None; mx=0
for i in range(100):
    b.tick(d,d); sp=math.hypot(b.vx,b.vz)
    if prev is not None: mx=max(mx,abs(sp-prev))
    prev=sp
print("max speed step per tick",mx)
