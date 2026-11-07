#!/usr/bin/env python3
"""Cascades (hybrid-beta 1.0.7) terrain: bundle the datapack for the Rust crate (`tectonic`, the same interpreter Tectonic uses).

  cascades.py bundle <hybrid-beta jar | extracted dir> [out]   -> tectonic/cascades.json (default out)
  cascades.py map [bundle] [out.pgm] [step] [seed]             -> height map of the 384-high world (eyeballing it)
  cascades.py biomes <jar | dir> [out.rs]                      -> `mc-rs/src/world/gen/cascades_table.rs`, the multi-noise biome table

The datapack is written in the 26.x density-function dialect (`left` / `right`, `input`, `gradient`, `cache`, `slice`,
`interval_select`...). `norm` rewrites it into the dialect of `tools/tectonic.py` / `tectonic/src/lib.rs` (`argument1` / `argument2`,
`argument`, `y_clamped_gradient`, `cache_once`, `range_choice` chains): the arithmetic is the same, only the spelling changes.

The pack only carries what Cascades changes; the vanilla functions it points at (`continents`, `erosion`, `ridges`, `depth`, the cave
noodle...) are written out below from memory of the 1.21 `NoiseRouterData` and are UNVERIFIED, like `VANILLA_NOISES` in `tectonic.py`.
The pack's `aquifers` block (barrier, floodedness, level spread, lava, exclusion, the preliminary surface) is bundled under `aq` and evaluated by
`tectonic/src/aquifer.rs`.
NOT PORTED, stubbed (every stub is a constant in the bundle's `stubs`): `minecraft:overworld/caves/spaghetti_2d` (it needs a
`weird_scaled_sampler`; the 3D spaghetti, the cheese, the layer caves, the entrances and the noodle caves are real),
`minecraft:overworld/caves/pillars`, the beardifier (no structures in the density), the blend nodes (no old chunks to blend with).
"""
import json, os, sys, zipfile, glob

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tectonic as T

ROOT = "cascades:final_density"
# Name of the router entries the world reads besides the final density: biome climate and the surface depth.
ROUTES = {
    "temperature": "cascades:temperature", "vegetation": "cascades:vegetation", "continents": "minecraft:overworld/continents",
    "erosion": "minecraft:overworld/erosion", "ridges": "minecraft:overworld/ridges", "depth": "minecraft:overworld/depth",
}
STUBS = {"minecraft:overworld/caves/spaghetti_2d": 1000.0, "minecraft:overworld/caves/pillars": -1000000.0}
BINARY = {"add", "mul", "min", "max"}
UNARY = {"abs", "square", "cube", "half_negative", "quarter_negative", "squeeze", "flat_cache", "cache_2d", "cache_once",
         "cache_all_in_cell", "interpolated", "blend_density"}

# UNVERIFIED (from memory of vanilla 1.21 `Noises`): (firstOctave, amplitudes) of the noises the pack points at but does not carry.
VANILLA_NOISES = {
    "minecraft:cave_layer": (-8, [1.0]),
    "minecraft:cave_cheese": (-8, [0.5, 1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0]),
    "minecraft:cave_entrance": (-7, [0.4, 0.5, 1.0]),
    "minecraft:spaghetti_3d_1": (-7, [1.0]),
    "minecraft:spaghetti_3d_2": (-7, [1.0]),
    "minecraft:spaghetti_3d_rarity": (-11, [1.0]),
    "minecraft:spaghetti_3d_thickness": (-8, [1.0]),
    "minecraft:spaghetti_roughness": (-5, [1.0]),
    "minecraft:spaghetti_roughness_modulator": (-8, [1.0]),
    "minecraft:noodle": (-8, [1.0]),
    "minecraft:noodle_thickness": (-8, [1.0]),
    "minecraft:noodle_ridge_a": (-7, [1.0]),
    "minecraft:noodle_ridge_b": (-7, [1.0]),
    # Aquifers (`Noises`): barrier, floodedness, level spread, lava.
    "minecraft:aquifer_barrier": (-3, [1.0]),
    "minecraft:aquifer_fluid_level_floodedness": (-7, [1.0]),
    "minecraft:aquifer_fluid_level_spread": (-5, [1.0]),
    "minecraft:aquifer_lava": (-1, [1.0]),
    # Ore veins (`Veminer`): the toggle (ore_veininess), the two ridged noises, the gap noise.
    "minecraft:ore_veininess": (-8, [1.0]),
    "minecraft:ore_vein_a": (-7, [1.0]),
    "minecraft:ore_vein_b": (-7, [1.0]),
    "minecraft:ore_gap": (-5, [1.0]),
}


def _noise(name, xz, y):
    return {"type": "minecraft:noise", "noise": name, "xz_scale": xz, "y_scale": y}


def _shifted(name):
    return {"type": "minecraft:flat_cache", "argument": {"type": "minecraft:shifted_noise", "noise": name, "shift_x": "minecraft:shift_x",
                                                         "shift_y": 0.0, "shift_z": "minecraft:shift_z", "xz_scale": 0.25, "y_scale": 0.0}}


def _add(a, b):
    return {"type": "minecraft:add", "argument1": a, "argument2": b}


def _mul(a, b):
    return {"type": "minecraft:mul", "argument1": a, "argument2": b}


def _grad(y0, y1, v0, v1):
    return {"type": "minecraft:y_clamped_gradient", "from_y": y0, "to_y": y1, "from_value": v0, "to_value": v1}


def _limited(f, lo, hi, fallback):
    """`yLimitedInterpolatable`: f inside lo <= y <= hi, `fallback` outside, interpolated."""
    return {"type": "minecraft:interpolated", "argument": {"type": "minecraft:range_choice", "input": "minecraft:y", "min_inclusive": lo,
                                                           "max_exclusive": hi + 1, "when_in_range": f, "when_out_of_range": fallback}}


def vanilla():
    """The vanilla overworld functions the pack refers to (already in the interpreter's dialect). UNVERIFIED, see the module doc."""
    ridges = _shifted("minecraft:ridge")
    noodle_ridge = lambda n: _limited(_noise(n, 2.6666666666666665, 2.6666666666666665), -60, 320, 0.0)
    return {
        "minecraft:overworld/continents": _shifted("minecraft:continentalness"),
        "minecraft:overworld/erosion": _shifted("minecraft:erosion"),
        "minecraft:overworld/ridges": ridges,
        # ridges_folded = -3 * (|(|ridges| - 2/3)| - 1/3)
        "minecraft:overworld/ridges_folded": _mul(-3.0, _add(-0.3333333333333333, {"type": "minecraft:abs", "argument": _add(-0.6666666666666666, {"type": "minecraft:abs", "argument": "minecraft:overworld/ridges"})})),
        # depth = y gradient (1.5 at the bottom, -1.5 at the top of the -64..320 world) + offset
        "minecraft:overworld/depth": _add(_grad(-64, 320, 1.5, -1.5), "minecraft:overworld/offset"),
        # roughness = cache_once((noise(spaghetti_roughness_modulator) mapped to 0 .. -0.1) * (|noise(spaghetti_roughness)| - 0.4))
        "minecraft:overworld/caves/spaghetti_roughness_function": {"type": "minecraft:cache_once", "argument": _mul(
            _mul(-0.05, _add(1.0, _noise("minecraft:spaghetti_roughness_modulator", 1.0, 1.0))),
            _add({"type": "minecraft:abs", "argument": _noise("minecraft:spaghetti_roughness", 1.0, 1.0)}, -0.4))},
        # noodle: where the y-limited noise is below 0, thickness + 1.5 * max(|ridge a|, |ridge b|); elsewhere 64 (solid)
        "minecraft:overworld/caves/noodle": {
            "type": "minecraft:range_choice", "input": _limited(_noise("minecraft:noodle", 1.0, 1.0), -60, 320, -1.0),
            "min_inclusive": -1000000.0, "max_exclusive": 0.0, "when_in_range": 64.0,
            "when_out_of_range": _add(
                _limited(_add(-0.075, _mul(-0.025, _add(1.0, _noise("minecraft:noodle_thickness", 1.0, 1.0)))), -60, 320, 0.0),
                _mul(1.5, {"type": "minecraft:max", "argument1": {"type": "minecraft:abs", "argument": noodle_ridge("minecraft:noodle_ridge_a")},
                           "argument2": {"type": "minecraft:abs", "argument": noodle_ridge("minecraft:noodle_ridge_b")}}))},
    }


def norm(n):
    """The 26.x spelling of a density function -> the interpreter's."""
    if isinstance(n, (int, float, str)) or n is None:
        return n
    if isinstance(n, list):
        return [norm(x) for x in n]
    t = n.get("type")
    if t is None:  # a spline body or a point: keep the shape, normalise what is inside
        return {k: norm(v) for k, v in n.items()}
    t = t.removeprefix("minecraft:")
    full = "minecraft:" + t
    if t in BINARY:
        return {"type": full, "argument1": norm(n["left"]), "argument2": norm(n["right"])}
    if t == "sub":
        return _add(norm(n["left"]), _mul(-1.0, norm(n["right"])))
    if t == "cache":
        return {"type": "minecraft:cache_once", "argument": norm(n["input"])}
    if t == "reciprocal":
        return {"type": "tectonic:invert", "argument": norm(n["input"])}
    if t in UNARY:
        return {"type": full, "argument": norm(n["input"])}
    if t == "clamp":
        return {"type": full, "input": norm(n["input"]), "min": n["min"], "max": n["max"]}
    if t == "range_choice":
        return {"type": full, "input": norm(n["input"]), "min_inclusive": n["min_inclusive"], "max_exclusive": n["max_exclusive"],
                "when_in_range": norm(n["when_in_range"]), "when_out_of_range": norm(n["when_out_of_range"])}
    if t == "gradient":
        assert n["axis"] == "y", n
        return _grad(n["from_coordinate"], n["to_coordinate"], n["from_value"], n["to_value"])
    if t == "slice":  # evaluate at y = 0: every input of a slice in the pack is 2D, so it is the input
        assert n["axis"] == "y" and n["coordinate"] == 0, n
        return norm(n["input"])
    if t == "noise":
        if "shift_x" in n:
            return {"type": "minecraft:shifted_noise", "noise": n["noise"], "shift_x": norm(n["shift_x"]), "shift_y": norm(n.get("shift_y", 0.0)),
                    "shift_z": norm(n["shift_z"]), "xz_scale": n["xz_scale"], "y_scale": n["y_scale"]}
        return {"type": full, "noise": n["noise"], "xz_scale": n["xz_scale"], "y_scale": n["y_scale"]}
    if t == "shifted_noise":
        return {"type": full, **{k: norm(v) if k.startswith("shift") else v for k, v in n.items() if k != "type"}}
    if t == "interval_select":  # f_i where thresholds[i-1] <= input < thresholds[i]: a chain of range_choice
        th, fs, inp = n["thresholds"], [norm(f) for f in n["functions"]], norm(n["input"])
        assert len(fs) == len(th) + 1
        res = fs[-1]
        for i in range(len(th) - 1, -1, -1):
            res = {"type": "minecraft:range_choice", "input": inp, "min_inclusive": th[i - 1] if i else -1000000.0, "max_exclusive": th[i],
                   "when_in_range": fs[i], "when_out_of_range": res}
        return res
    if t == "spline":
        return {"type": full, "spline": norm_spline(n["spline"])}
    if t in ("blend_alpha", "blend_offset"):
        return {"type": full}
    if t == "beardifier":
        return 0.0
    raise SystemExit(f"cascades.py: density function type {t} is not handled")


def norm_spline(s):
    if isinstance(s, (int, float)):
        return s
    return {"coordinate": norm(s["coordinate"]),
            "points": [{"location": p["location"], "derivative": p["derivative"], "value": norm_spline(p["value"])} for p in s["points"]]}


class Pack:
    """The jar (or an extracted directory): `get(kind, id)` -> parsed JSON or None."""

    def __init__(self, path):
        self.z = zipfile.ZipFile(path) if os.path.isfile(path) else None
        self.dir = path

    def get(self, kind, ident):
        ns, p = ident.split(":", 1) if ":" in ident else ("minecraft", ident)
        rel = f"data/{ns}/worldgen/{kind}/{p}.json"
        if self.z:
            return json.loads(self.z.read(rel)) if rel in self.z.namelist() else None
        f = os.path.join(self.dir, rel)
        return json.load(open(f)) if os.path.exists(f) else None


def bundle(path, out):
    pack, van = Pack(path), vanilla()
    router = pack.get("noise_settings", "minecraft:overworld")["noise_router"]
    fns, noises = {}, {}
    fns[ROOT] = norm(router["final_density"])
    fns["cascades:temperature"] = norm(router["temperature"])
    fns["cascades:vegetation"] = norm(router["vegetation"])

    def walk(n):
        if isinstance(n, str):
            if n in fns or n in STUBS or n in T.BUILTINS:
                return
            if n in van:
                fns[n] = van[n]
            else:
                j = pack.get("density_function", n)
                assert j is not None, "missing density function " + n
                fns[n] = norm(j)
            walk(fns[n])
        elif isinstance(n, list):
            for x in n:
                walk(x)
        elif isinstance(n, dict):
            if isinstance(n.get("noise"), str):
                noises.setdefault(n["noise"], None)
            for k, v in n.items():
                if k not in ("type", "noise"):
                    walk(v)
    # The pack's `aquifers` block (`noise_settings/overworld.json`). `find_top_surface` (the preliminary surface) is evaluated by the crate,
    # so it gets the density and the upper bound as two functions; then the barrier / lava noises, floodedness, level spread, exclusion.
    aq = pack.get("noise_settings", "minecraft:overworld")["aquifers"]
    fns["cascades:aq_surface_density"] = norm(aq["surface_level"]["density"])
    fns["cascades:aq_surface_upper"] = norm(aq["surface_level"]["upper_bound"])
    for k in ("barrier", "lava", "fluid_level_floodedness", "fluid_level_spread", "exclusion"):
        fns["cascades:aq_" + k] = norm(aq[k])
    # Ore veins: vanilla's router (not in the pack): toggle = noise(ore_veininess, 1.5, 1.5), ridged = -0.08 + max(|a|, |b|) with a, b = noise(ore_vein_a / b, 4, 4),
    # gap = noise(ore_gap, 1, 1). The y limits (-60 .. 50) and the veins themselves are in `aquifer.rs`.
    fns["cascades:vein_toggle"] = _noise("minecraft:ore_veininess", 1.5, 1.5)
    fns["cascades:vein_a"] = _noise("minecraft:ore_vein_a", 4.0, 4.0)
    fns["cascades:vein_b"] = _noise("minecraft:ore_vein_b", 4.0, 4.0)
    fns["cascades:vein_gap"] = _noise("minecraft:ore_gap", 1.0, 1.0)
    aq_fns = {k: "cascades:aq_" + k for k in ("surface_density", "surface_upper", "barrier", "lava", "fluid_level_floodedness", "fluid_level_spread", "exclusion")}
    aq_fns.update({"vein_toggle": "cascades:vein_toggle", "vein_a": "cascades:vein_a", "vein_b": "cascades:vein_b", "vein_gap": "cascades:vein_gap"})
    for r in (ROOT, "cascades:temperature", "cascades:vegetation", *aq_fns.values()):
        walk(fns[r])
    for r in ROUTES.values():
        walk(r)
    noises.setdefault("minecraft:offset", None)
    for k in noises:
        j = pack.get("noise", k)  # the pack's own (26.x spelling: base_octave / amplitude_modifiers) wins over the from-memory table
        if j is not None:
            o, a = j["base_octave"], j["amplitude_modifiers"]
            assert j["octave_count"] == len(a), k
        else:
            assert k in VANILLA_NOISES or k in T.VANILLA_NOISES, "missing noise " + k
            o, a = VANILLA_NOISES.get(k) or T.VANILLA_NOISES[k]
        noises[k] = {"o": o, "a": a, "h": T.md5_pair(k)}
    octs = {str(i): T.md5_pair(f"octave_{i}") for i in range(-16, 1)}
    # `aq`: the functions above by role, `find_top_surface`'s lower bound and cell height, the name hash of the aquifer's positional random.
    aqj = {"fns": aq_fns, "lower": aq["surface_level"]["lower_bound"], "cell": aq["surface_level"]["cell_height"], "hash": T.md5_pair("minecraft:aquifer"), "ore_hash": T.md5_pair("minecraft:ore")}
    json.dump({"root": ROOT, "routes": ROUTES, "stubs": STUBS, "f": fns, "n": noises, "oct": octs, "terrain": T.md5_pair("minecraft:terrain"), "aq": aqj},
              open(out, "w"), separators=(",", ":"))
    print(f"{out}: {len(fns)} functions, {len(noises)} noises, {os.path.getsize(out)} bytes")


# Cascades' biome -> a biome of the 19 mc-rs has (`world/biome.rs`). The Blooming world type shows Beta's 13 and Blooming's 6; the new
# biomes of the pack (`hybrid_beta:*`) are not added: each takes the nearest existing one. Anything not listed is an error.
FOLD = {
    "hybrid_beta:seasonal_forest": "SeasonalForest", "hybrid_beta:rainforest": "Rainforest", "hybrid_beta:temperate_rainforest": "WetWoodland",
    "hybrid_beta:autumnal_forest": "AutumnalForest",
    "minecraft:plains": "Plains", "minecraft:sunflower_plains": "Plains", "minecraft:meadow": "Plains", "minecraft:river": "Plains",
    "minecraft:savanna": "Savanna", "minecraft:savanna_plateau": "Savanna",
    "minecraft:forest": "Forest", "minecraft:flower_forest": "Forest", "minecraft:pale_garden": "Forest",
    "minecraft:dappled_forest": "OakWoodland", "minecraft:dark_forest": "AutumnalForest",
    "minecraft:badlands": "Desert", "minecraft:eroded_badlands": "Desert", "minecraft:desert": "Desert", "minecraft:wooded_badlands": "Chaparral",
    "minecraft:jungle": "Rainforest", "minecraft:bamboo_jungle": "Rainforest",
    "minecraft:swamp": "Swamp", "minecraft:mangrove_swamp": "Marsh",
    "minecraft:cherry_grove": "SnowyBirchGrove",
    "minecraft:taiga": "Taiga", "minecraft:old_growth_pine_taiga": "Taiga", "minecraft:old_growth_spruce_taiga": "Taiga", "minecraft:snowy_taiga": "Taiga",
    "minecraft:grove": "Taiga",
    "minecraft:snowy_plains": "Tundra", "minecraft:ice_spikes": "Tundra", "minecraft:snowy_slopes": "Tundra", "minecraft:frozen_river": "Tundra",
    "minecraft:beach": "Beach", "minecraft:snowy_beach": "Beach", "minecraft:stony_shore": "Beach",
    "minecraft:stony_peaks": "Mountains", "minecraft:jagged_peaks": "Mountains", "minecraft:frozen_peaks": "Mountains",
    "minecraft:ocean": "Ocean", "minecraft:deep_ocean": "Ocean", "minecraft:cold_ocean": "Ocean", "minecraft:deep_cold_ocean": "Ocean",
    "minecraft:frozen_ocean": "Ocean", "minecraft:deep_frozen_ocean": "Ocean", "minecraft:lukewarm_ocean": "Ocean",
    "minecraft:deep_lukewarm_ocean": "Ocean", "minecraft:warm_ocean": "Ocean", "minecraft:mushroom_fields": "Ocean",
}


def biomes(path, out):
    """The surface boxes of the pack's multi-noise biome source (depth range containing 0), folded, as a Rust table."""
    d = json.loads(zipfile.ZipFile(path).read("data/minecraft/dimension/overworld.json") if os.path.isfile(path)
                   else open(os.path.join(path, "data/minecraft/dimension/overworld.json")).read())
    iv = lambda v: (v, v) if not isinstance(v, list) else (v[0], v[1])
    rows, seen = [], set()
    for b in d["generator"]["biome_source"]["biomes"]:
        p = b["parameters"]
        lo, hi = iv(p["depth"])
        if not lo <= 0 <= hi:
            continue
        assert b["biome"] in FOLD, "unfolded biome " + b["biome"]
        seen.add(b["biome"])
        r = [iv(p[k]) for k in ("temperature", "humidity", "continentalness", "erosion", "weirdness")]
        rows.append((FOLD[b["biome"]], r, p["offset"]))
    f = lambda v: repr(float(v))
    lines = ["// Generated by `tools/cascades.py biomes` from the Cascades (hybrid-beta 1.0.7) overworld dimension: the surface entries of its multi-noise",
             "// biome table (depth range containing 0), in the pack's order, each biome folded to one of the 19 (`FOLD` in the tool). Do not edit.",
             "use crate::world::biome::Biome;", "",
             "/// (biome, [temperature, humidity, continentalness, erosion, weirdness] as (lo, hi), offset)",
             f"pub static BOXES: [(Biome, [(f32, f32); 5], f32); {len(rows)}] = ["]
    for name, r, off in rows:
        lines.append(f"    (Biome::{name}, [" + ", ".join(f"({f(a)}, {f(b)})" for a, b in r) + f"], {f(off)}),")
    lines.append("];")
    open(out, "w").write("\n".join(lines) + "\n")
    print(f"{out}: {len(rows)} boxes, {len(seen)} biomes")


def cascades_config():
    """`tectonic.default_config` for the 384-high world whose sea is at y = 64 (the datapack's is 63 in -64 .. 320): y_world = y_pack + 1... the
    index frame of a chunk is 0 .. 383, so y_pack = y_index - 1 and the pack's bottom (-64) is below the world: min_y = -1."""
    return T.default_config(min_y=-1.0, max_y=383.0, vertical_scale=1.0)


def heightmap(bundle_path, n=24, step=80, seed=1, out=None):
    """Surface of one column per `step` blocks from the cell-corner densities (as `tectonic.py map` does), 384 high, sea at 64."""
    t = T.Terrain(bundle_path, seed, cascades_config())
    sh, hs = 1, []  # y_pack = y_index - 1
    t.reset()
    for iz in range(n):
        row = []
        for ix in range(n):
            x, z = (ix - n // 2) * step, (iz - n // 2) * step
            t.interp_mode = lambda nd, X, Y, Z: t.ev(nd["argument"], X, Y, Z)
            col = [t.ev(t.b["root"], x, gy * 8 - sh, z) for gy in range(49)]
            h = -1
            for gy in range(48, -1, -1):
                if col[gy] > 0:
                    h = gy * 8 + (8 * col[gy] / (col[gy] - col[gy + 1]) if gy < 48 else 0)
                    break
            row.append(h)
        hs.append(row)
        if iz % 4 == 0:
            t.reset()
    flat = sorted(h for r in hs for h in r)
    land = [h for h in flat if h >= 64]
    print(f"columns {len(flat)}: min {flat[0]:.0f} max {flat[-1]:.0f} below sea {100 * sum(h < 64 for h in flat) / len(flat):.0f}% "
          f"land mean {sum(land) / max(1, len(land)):.1f}; p50 {flat[len(flat) // 2]:.0f} p90 {flat[int(len(flat) * .9)]:.0f} p99 {flat[int(len(flat) * .99)]:.0f}")
    if out:
        with open(out, "w") as f:
            f.write(f"P2\n{n} {n}\n255\n")
            for r in hs:
                f.write(" ".join(str(max(0, min(255, int(h * 255 / 384)))) for h in r) + "\n")
    return hs


def main(a):
    if len(a) >= 2 and a[1] == "map":
        heightmap(a[2] if len(a) > 2 else "tectonic/cascades.json", out=a[3] if len(a) > 3 else None,
                  step=int(a[4]) if len(a) > 4 else 80, seed=int(a[5]) if len(a) > 5 else 1)
    elif len(a) >= 3 and a[1] == "biomes":
        biomes(a[2], a[3] if len(a) > 3 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "mc-rs", "src", "world", "gen", "cascades_table.rs"))
    elif len(a) >= 3 and a[1] == "bundle":
        bundle(a[2], a[3] if len(a) > 3 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "tectonic", "cascades.json"))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
