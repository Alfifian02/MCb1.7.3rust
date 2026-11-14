#!/usr/bin/env python3
"""Tuning of the Cascades bundle (`tectonic/cascades.json`) against the artifacts seen in game. Idempotent: the bundle carries a `tune`
marker and a second run changes nothing. `cascades.py bundle` calls `tune()` after it has written the pack's functions.

  cascades_tune.py [bundle.json]      (default tectonic/cascades.json, rewritten in place)

What and why (numbers measured by `tectonic/examples/probe.rs`, 2 seeds x 400 chunks):
* NOISE_3D: `base_3d_noise` is scaled in `sloped_cheese`. With the pack's weight the 3D noise out-voted the terrain's vertical slope
  (factor ~1.3, a quarter of that above the ground), so hills grew sheer 16+ block walls and ledges. 0.55 keeps overhangs, not walls.
* ENTRANCE_BIAS: lowers the cave-entrance threshold (`0.37 + noise`; a cave is where the entrance function is <= 0), so entrances (the
  shafts and mouths that open on the surface) are more frequent: -0.15 gave about twice the surface openings and 1.5 x the ones that lead
  to >= 1500 blocks of cave (`examples/probe5.rs`, 3 seeds x 400 chunks); -0.22 opened 17 % of the land, too much. CHAMBER: where the entrance noise is inside a tunnel, the cheese cave threshold is lowered, so an entrance more often
  opens into a big cheese chamber under it instead of a dead-end tunnel.
"""
import json, os, sys

NOISE_3D = 0.55
ENTRANCE_BIAS = -0.15
CHAMBER = 0.6          # cheese threshold drop at full depth inside an entrance tunnel
CHAMBER_RANGE = 0.3    # entrance values under this count as "inside a tunnel"
MARK = {"noise_3d": NOISE_3D, "entrance_bias": ENTRANCE_BIAS, "chamber": CHAMBER}
ENT = "hybrid_beta:overworld/caves/entrances"


def _bump(node, old, new):
    """Replace the first constant `old` that is the `argument1` of an add whose `argument2` is a noise: the entrance threshold."""
    if isinstance(node, dict):
        if node.get("type") == "minecraft:add" and node.get("argument1") == old and isinstance(node.get("argument2"), dict) \
                and node["argument2"].get("noise") == "minecraft:cave_entrance":
            node["argument1"] = new
            return True
        return any(_bump(v, old, new) for v in node.values())
    if isinstance(node, list):
        return any(_bump(v, old, new) for v in node)
    return False


def tune(d):
    if d.get("tune") == MARK:
        return False
    assert "tune" not in d, "bundle tuned with other numbers: rebuild it with `cascades.py bundle` first"
    f = d["f"]
    sc = f["minecraft:overworld/sloped_cheese"]
    assert sc["argument2"] == "minecraft:overworld/base_3d_noise", sc["argument2"]
    sc["argument2"] = {"type": "minecraft:mul", "argument1": NOISE_3D, "argument2": "minecraft:overworld/base_3d_noise"}
    assert _bump(f[ENT], 0.37, round(0.37 + ENTRANCE_BIAS, 4)), "entrance threshold not found"
    ch = f["hybrid_beta:overworld/caves/cheese"]
    inner = ch["argument2"]["input"]            # add(0.27, noise)
    assert inner["argument1"] == 0.27
    inside = {"type": "minecraft:clamp", "input": {"type": "minecraft:add", "argument1": CHAMBER_RANGE,
              "argument2": {"type": "minecraft:mul", "argument1": -1.0, "argument2": ENT}}, "min": 0.0, "max": CHAMBER_RANGE}
    ch["argument2"]["input"] = {"type": "minecraft:add", "argument1": inner,
                                "argument2": {"type": "minecraft:mul", "argument1": -CHAMBER / CHAMBER_RANGE, "argument2": inside}}
    d["tune"] = MARK
    return True


if __name__ == "__main__":
    p = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "tectonic", "cascades.json")
    d = json.load(open(p))
    print("tuned" if tune(d) else "already tuned")
    json.dump(d, open(p, "w"), separators=(",", ":"))
