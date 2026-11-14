"""Python twin of the shoulder-surf camera maths in `mc-rs/src/render/shoulder.rs` (no Rust toolchain needed).
Checks the sign conventions: the aim point lands on the screen centre after the convergence matrix, and a
critically damped spring never overshoots. Run: python3 tools/shoulder_proto.py"""
import math, random
import numpy as np

def rx(a): c, s = math.cos(a), math.sin(a); return np.array([[1,0,0],[0,c,-s],[0,s,c]])
def ry(a): c, s = math.cos(a), math.sin(a); return np.array([[c,0,s],[0,1,0],[-s,0,c]])
def rz(a): c, s = math.cos(a), math.sin(a); return np.array([[c,-s,0],[s,c,0],[0,0,1]])

def basis(yaw, pitch):
    sy, cy = math.sin(yaw), math.cos(yaw); sp, cp = math.sin(pitch), math.cos(pitch)
    fwd = np.array([sy*cp, -sp, -cy*cp]); fwd /= np.linalg.norm(fwd)
    right = np.array([cy, 0, sy]); up = np.cross(right, fwd)
    return fwd, right, up

def look_at(pos, fwd):  # glam look_at_rh with up = +Y; rotation part only + translation
    f = fwd / np.linalg.norm(fwd); s = np.cross(f, [0,1,0]); s /= np.linalg.norm(s); u = np.cross(s, f)
    R = np.array([s, u, -f]); return R, -R @ pos

random.seed(1)
worst = 0.0
for _ in range(2000):
    yaw, pitch = random.uniform(-6, 6), random.uniform(-1.4, 1.4)
    fwd, right, up = basis(yaw, pitch)
    eye = np.array([random.uniform(-50, 50) for _ in range(3)])
    side, back, rise = random.uniform(-0.8, 0.8), random.uniform(1.0, 3.5), random.uniform(-0.1, 0.3)
    pos = eye + right*side - fwd*back + np.array([0, rise, 0])
    D = random.uniform(4, 64)
    d = (eye + fwd*D) - pos
    df, dr, du = d @ fwd, d @ right, d @ up
    conv_yaw, conv_pitch = math.atan2(dr, df), math.atan2(du, math.hypot(dr, df))
    bob = rx(-conv_pitch) @ ry(conv_yaw)           # same order as the Rust: Rz * Rx(-pitch) * Ry(yaw)
    R, t = look_at(pos, fwd)
    p = bob @ (R @ (eye + fwd*D) + t)
    worst = max(worst, abs(math.atan2(p[0], -p[2])), abs(math.atan2(p[1], -p[2])))
print("aim point off screen centre, worst angle (rad):", worst)
assert worst < 1e-9, "convergence matrix has a wrong sign"

# roll: positive Rz leans the camera right (the horizon's right end goes up on screen)
p = rz(0.1) @ np.array([1.0, 0.0, -5.0]); assert p[1] > 0, "positive roll should lift the right side of the view"

def crit(x, v, target, w, dt):
    d = x - target; e = math.exp(-w*dt); c2 = v + w*d
    return target + (d + c2*dt)*e, (v - w*c2*dt)*e
x, v, peak = 0.0, 0.0, 0.0
for i in range(600):
    x, v = crit(x, v, 1.0, 5.5, 1/60); peak = max(peak, x)
print("spring: peak", peak, "after 10 s", x)
assert peak <= 1.0 + 1e-9 and abs(x - 1) < 1e-6
# exactness: one big step equals many small ones
a = crit(0.0, 2.0, 1.0, 7.0, 0.5); b = (0.0, 2.0)
for i in range(50): b = crit(b[0], b[1], 1.0, 7.0, 0.01)
assert abs(a[0]-b[0]) < 1e-9 and abs(a[1]-b[1]) < 1e-9

# strafing toward the camera's shoulder: the pivot's x spring lags the player by up to the 0.6 cap, which used to eat the whole 0.62
# shoulder offset and put the player on the crosshair; the clamp (lag toward the head's line cut to 0) keeps it at the standing offset.
def strafe(clamped, v=4.3, dt=1/60, shoulder=0.62):
    eye, px, pv, least = 0.0, shoulder, 0.0, 9.0
    for _ in range(240):
        eye += v*dt
        want = eye + shoulder
        px, pv = crit(px, pv, want, 12.0, dt)
        lat = px - want
        if clamped and lat*shoulder < 0: px = want          # `Rig::update`: lag toward the head's line is dropped
        if abs(px - want) > 0.6: px = want + math.copysign(0.6, px - want)
        least = min(least, px - eye)
    return least
old, new = strafe(False), strafe(True)
print("camera offset from the player's line while strafing: before", round(old, 3), "after", round(new, 3), "(standing 0.62)")
assert old < 0.1 and abs(new - 0.62) < 1e-6

# the glide starts on the vanilla third-person place: first frame of `Rig::update` in open air, yaw 0 (behind = +z), eye at the origin
RADIUS, THIRD = 0.3, 4.3
dt = 1/60; blend, _ = crit(0.0, 0.0, 1.0, 16.0, dt)
dist = 2.6; to = np.array([0.62*blend, 0.2*blend, THIRD + (dist - THIRD)*blend]); ln = np.linalg.norm(to)
pos = to * ((ln - RADIUS)/ln)
print("first frame: blend", round(blend, 4), "camera", pos.round(3))
assert 3.85 < pos[2] < 4.0 and abs(pos[0]) < 0.05
print("ok")
