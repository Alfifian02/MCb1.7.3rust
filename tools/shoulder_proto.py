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
print("ok")
