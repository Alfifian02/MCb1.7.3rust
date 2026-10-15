#!/usr/bin/env python3
"""Run from the repo root: python3 apply_zoom.py
Adds the Zoomify button + paused-menu frame cap to mc-rs. Each edit is found by its text (not line numbers);
nothing is written unless every anchor is found exactly once. Safe to re-run (already-applied edits are skipped)."""
import sys

ED = {}  # path -> [(anchor, replacement, marker)]
def e(path, anchor, new, marker):
    ED.setdefault(path, []).append((anchor, new, marker))

T = "mc-rs/src/input/touch_ui.rs"
e(T, "    /// Press landed on the inventory button. Opens",
  "    /// Press landed on the zoom button. Zoomed while held (`TouchUi::zooming`).\n    Zoom,\n    /// Press landed on the inventory button. Opens", "    Zoom,\n")
e(T, "    pub inventory: Rect,\n",
  "    pub inventory: Rect,\n    /// Hold to zoom (Zoomify), left of `inventory`.\n    pub zoom: Rect,\n", "pub zoom: Rect")
e(T, "        let inventory = (w - ps * 2.0 - pad * 1.5, pad, ps, ps);\n",
  "        let inventory = (w - ps * 2.0 - pad * 1.5, pad, ps, ps);\n        let zoom = (w - ps * 3.0 - pad * 2.0, pad, ps, ps);\n", "let zoom = (")
e(T, "pause, inventory, resume,", "pause, inventory, zoom, resume,", "inventory, zoom, resume,")
e(T, "        if in_rect(self.jump, x, y) {",
  "        if in_rect(self.zoom, x, y) {\n            return Some(PointerRole::Zoom);\n        }\n        if in_rect(self.jump, x, y) {", "PointerRole::Zoom);")
e(T, "    /// The inventory button was pressed and released since the last call.\n    pub fn take_inventory",
  "    /// A finger is on the zoom button.\n    pub fn zooming(&self) -> bool {\n        self.pointers.values().any(|p| p.role == PointerRole::Zoom)\n    }\n\n    /// The inventory button was pressed and released since the last call.\n    pub fn take_inventory", "pub fn zooming")
e(T, "PointerRole::Jump => {}", "PointerRole::Jump | PointerRole::Zoom => {}", "Jump | PointerRole::Zoom")
e(T, "    #[test]\n    fn open_screen_turns_presses_into_taps() {", '''    /// The zoom button is held, not tapped: `zooming` is true while the finger is down, only in play, and it clears on release.
    #[test]
    fn zoom_button_is_a_hold() {
        let mut ui = TouchUi::new(W, H);
        let (x, y) = mid(ui.layout.zoom);
        assert!(ui.layout.zoom.0 + ui.layout.zoom.2 < ui.layout.inventory.0, "zoom sits left of inventory");
        assert_eq!(ui.layout.hit_test(x, y, true, false), None);
        assert_eq!(ui.layout.hit_test(x, y, false, true), Some(PointerRole::Tap));
        assert_eq!(ui.layout.hit_test(x, y, false, false), Some(PointerRole::Zoom));
        ui.on_press(1, x, y, PointerRole::Zoom);
        assert!(ui.zooming());
        ui.on_release(1);
        assert!(!ui.zooming());
    }

    #[test]
    fn open_screen_turns_presses_into_taps() {''', "fn zoom_button_is_a_hold")

L = "mc-rs/src/lib.rs"
e(L, "        self.last_frame = now;\n", """        self.last_frame = now;
        // Zoomify: hold the zoom button for 4x (the vertical tangent / 4), eased; the look drag slows with the field of view.
        let base = 70_f32.to_radians();
        let target = if self.touch.zooming() { 2.0 * ((base * 0.5).tan() / ZOOM).atan() } else { base };
        self.camera.fov_y += (target - self.camera.fov_y) * (1.0 - (-14.0 * dt).exp());
""", "self.touch.zooming()")
e(L, "            self.camera.add_yaw(look_dx * LOOK_SENS);\n            self.camera.add_pitch(look_dy * LOOK_SENS);",
  "            let k = LOOK_SENS * (self.camera.fov_y * 0.5).tan() / (70_f32.to_radians() * 0.5).tan();\n            self.camera.add_yaw(look_dx * k);\n            self.camera.add_pitch(look_dy * k);", "let k = LOOK_SENS *")
e(L, "/// World units per second when a d-pad direction is held.",
  "/// Zoom button magnification (Zoomify's default).\nconst ZOOM: f32 = 4.0;\n/// World units per second when a d-pad direction is held.", "const ZOOM: f32")
e(L, "        // Hotbar (`GuiIngame`): the 182 x 22 strip", """        // Zoom button, left of the inventory button: a Z (Zoomify), lit while held.
        let (zx, zy, zw, zh) = layout.zoom;
        HudPipeline::push_button(v, zx, zy, zw, zh, if pressed(PointerRole::Zoom) || !play { 2 } else { 1 });
        HudPipeline::push_text_centered(v, zx + zw * 0.5, zy + (zh - 8.0 * zh / 20.0) * 0.5, zh / 20.0, "Z", [1.0; 4]);

        // Hotbar (`GuiIngame`): the 182 x 22 strip""", "layout.zoom;")
e(L, "        if let Some(a) = app_state.as_mut() {\n            a.render();",
  "        if let Some(a) = app_state.as_mut() {\n            a.render();\n            // Dynamic FPS: the paused world is frozen, so the menu runs at ~10 frames a second; a touch still wakes the poll.\n            if a.touch.paused {\n                std::thread::sleep(Duration::from_millis(90));\n            }", "Dynamic FPS: the paused")

out, bad = {}, []
for path, edits in ED.items():
    try:
        s = open(path).read()
    except OSError as ex:
        bad.append(f"{path}: {ex}"); continue
    for anchor, new, marker in edits:
        if marker in s:
            continue
        n = s.count(anchor)
        if n != 1:
            bad.append(f"{path}: anchor found {n}x (need 1): {anchor.strip()[:70]!r}")
        else:
            s = s.replace(anchor, new, 1)
    out[path] = s
if bad:
    print("NOTHING WRITTEN. Problems:\n  " + "\n  ".join(bad)); sys.exit(1)
for path, s in out.items():
    open(path, "w").write(s)

# docs: append-only, never fails on context
try:
    r = open("ROADMAP.md").read()
    if "zoom_button_is_a_hold" not in r:
        open("ROADMAP.md", "a").write("\n- `done` Additive modpack audit (packwiz manifest of ~60 Fabric mods, no code; not compiled, not run; +1 test `touch_ui::tests::zoom_button_is_a_hold`). Ported: Zoomify (hold the Z button left of the inventory button: 4x, eased, look drag slows with the field of view) and Dynamic FPS (the paused menu sleeps 90 ms a frame in `android_main`). Already here: Sodium (`embeddium/`), C2ME, Lithium, Iris-style shaders. Not applicable: Fabric / Mod Menu / config libs, server + networking mods, skins and capes, resource-pack mods. Left out: Better Grass + Cull Fewer Leaves (shipped off), FastQuit, Fast Noise / Fast Surface (would risk the golden tests), Dynamic Lights, Entity Culling, Controlify (need real work or a device).\n")
except OSError:
    pass
print("ok: zoom button + paused FPS cap applied to", ", ".join(out))
