//! The pieces every menu shares, in b1.7.3's skin (stone buttons, dirt, the yellow of the splash, `GuiSlot`'s scroll bar): a clip for
//! scrolling lists, word wrap, a scroll bar, a bevelled panel and the screen title, and the pad's step between targets (`step`). NOT b1.7.3 (it has no scrolling options screen);
//! the layout of the options list is Sodium's (`VideoSettingsScreen`: a page list, one long option list, a tooltip pane).
//!
//! Units: everything takes GUI units except `clip`, which works on the vertices already in px.

use crate::menu::{grad, rect, rgb, tw};
use crate::render::hud::{HudPipeline, HudVertex};

fn lerp<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// What a scissor box would do: cut the quads pushed since `from` (6 vertices each, axis-aligned, which is what every
/// `HudPipeline::push_*` makes) to `r` = (x, y, w, h) in px. A cut quad keeps its texture and its colour where they are (both are linear in
/// x and y over a quad), so a half-visible glyph or button shows exactly its visible half. Anything else (a rotated quad) must not be pushed
/// between `from` and the call.
pub fn clip(v: &mut Vec<HudVertex>, from: usize, r: [f32; 4]) {
    let tail = v.split_off(from);
    let (cx0, cy0, cx1, cy1) = (r[0], r[1], r[0] + r[2], r[1] + r[3]);
    for q in tail.chunks_exact(6) {
        let (tl, tr, br, bl) = (q[0], q[1], q[2], q[5]);
        let (x0, y0, x1, y1) = (tl.pos[0], tl.pos[1], br.pos[0], br.pos[1]);
        let (nx0, ny0, nx1, ny1) = (x0.max(cx0), y0.max(cy0), x1.min(cx1), y1.min(cy1));
        if nx0 >= nx1 || ny0 >= ny1 {
            continue; // outside (or empty)
        }
        if (nx0, ny0, nx1, ny1) == (x0, y0, x1, y1) {
            v.extend_from_slice(q); // inside
            continue;
        }
        let (a, b) = ((nx0 - x0) / (x1 - x0), (nx1 - x0) / (x1 - x0));
        let (c, d) = ((ny0 - y0) / (y1 - y0), (ny1 - y0) / (y1 - y0));
        let at = |tx: f32, ty: f32, pos: [f32; 2]| HudVertex {
            pos,
            color: lerp(lerp(tl.color, tr.color, tx), lerp(bl.color, br.color, tx), ty),
            uv: lerp(lerp(tl.uv, tr.uv, tx), lerp(bl.uv, br.uv, tx), ty),
        };
        let (n_tl, n_tr, n_br, n_bl) = (at(a, c, [nx0, ny0]), at(b, c, [nx1, ny0]), at(b, d, [nx1, ny1]), at(a, d, [nx0, ny1]));
        v.extend([n_tl, n_tr, n_br, n_tl, n_br, n_bl]);
    }
}

/// `text` broken into lines at most `w` units wide, at the spaces.
/// ponytail: a single word wider than `w` stays whole on its own line (it overflows); every text that goes through here is short words.
pub fn wrap(text: &str, w: f32) -> Vec<String> {
    let (mut lines, mut line) = (Vec::new(), String::new());
    for word in text.split_whitespace() {
        let joined = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if line.is_empty() || tw(&joined) <= w {
            line = joined;
        } else {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// `s`, cut and ended with ".." when it is wider than `w` units.
pub fn fit(s: &str, w: f32) -> String {
    if tw(s) <= w {
        return s.to_string();
    }
    let mut t = s.to_string();
    while !t.is_empty() && tw(&format!("{t}..")) > w {
        t.pop();
    }
    format!("{t}..")
}

/// `GuiSlot`'s thumb: (its offset from the top of the box, its height) for a box `view` tall over `content` units, scrolled `scroll` of
/// `max` (> 0). At least 32 tall and 8 short of the box.
pub fn thumb(view: f32, content: f32, scroll: f32, max: f32) -> (f32, f32) {
    let th = (view * view / content).max(32.0).min(view - 8.0).floor();
    ((scroll * (view - th) / max).clamp(0.0, view - th), th)
}

/// `GuiSlot`'s scroll bar (6 wide: a black track, a grey thumb with a light edge) at `x`, over the box (`top`, `view`) of a list `content`
/// tall that is scrolled `scroll`. Nothing when everything fits.
pub fn scrollbar(v: &mut Vec<HudVertex>, s: f32, x: f32, top: f32, view: f32, content: f32, scroll: f32) {
    let max = content - view;
    if max <= 0.0 {
        return;
    }
    let (off, th) = thumb(view, content, scroll, max);
    rect(v, s, [x, top, 6.0, view], 0x000000);
    rect(v, s, [x, top + off, 6.0, th], 0x808080);
    rect(v, s, [x, top + off, 5.0, th - 1.0], 0xC0C0C0);
}

/// A 1-unit bevel round `r` (light on the top and left, black on the bottom and right): the frame of a list or a panel.
pub fn bevel(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4]) {
    rect(v, s, [r[0], r[1], r[2], 1.0], 0x606060);
    rect(v, s, [r[0], r[1], 1.0, r[3]], 0x606060);
    rect(v, s, [r[0], r[1] + r[3] - 1.0, r[2], 1.0], 0x000000);
    rect(v, s, [r[0] + r[2] - 1.0, r[1], 1.0, r[3]], 0x000000);
}

/// A bevelled box filled with black at `alpha`: what sits behind a stack of buttons or a block of text.
pub fn panel(v: &mut Vec<HudVertex>, s: f32, r: [f32; 4], alpha: f32) {
    HudPipeline::push_quad(v, r[0] * s, r[1] * s, r[2] * s, r[3] * s, [0.0, 0.0, 0.0, alpha]);
    bevel(v, s, r);
}

/// The title of a screen: 2x text between two rules, a gold stud at each end of the text. `band` first lays a dark strip (24 units) behind
/// it, for a screen whose background is the world or the panorama; a list screen's own dirt header needs none. `y` is the top.
pub fn title(v: &mut Vec<HudVertex>, s: f32, gw: f32, y: f32, text: &str, band: bool) {
    let w = tw(text) * 2.0;
    if band {
        grad(v, s, [0.0, y, gw, 24.0], [0.0, 0.0, 0.0, 0.7], [0.0, 0.0, 0.0, 0.35]);
        rect(v, s, [0.0, y + 24.0, gw, 1.0], 0x606060);
    }
    let x = ((gw - w) / 2.0).floor();
    HudPipeline::push_text(v, x * s, (y + 4.0) * s, 2.0 * s, text, rgb(0xFFFFFF));
    for sx in [x - 9.0, x + w + 6.0] {
        rect(v, s, [sx, y + 10.0, 3.0, 3.0], 0xFFAA00);
    }
    rect(v, s, [8.0, y + 11.0, (x - 17.0 - 8.0).max(0.0), 1.0], 0x606060);
    rect(v, s, [x + w + 17.0, y + 11.0, (gw - 8.0 - (x + w + 17.0)).max(0.0), 1.0], 0x606060);
}

/// The target the cursor `at` rests on.
pub fn under(targets: &[[f32; 4]], at: (f32, f32)) -> Option<[f32; 4]> {
    targets.iter().copied().find(|r| at.0 >= r[0] && at.0 < r[0] + r[2] && at.1 >= r[1] && at.1 < r[1] + r[3])
}

/// The pad's step: the rect to jump to from the one under `at` when the d-pad says `d` (x, y; y down). Off every rect, the nearest one.
/// Among the rects that lie that way, within 63 degrees of it, the one with the least `along + 2 * across` (centre to centre) wins:
/// touching slots step one by one. `None` when nothing lies that way.
pub fn step(targets: &[[f32; 4]], at: (f32, f32), d: (i32, i32)) -> Option<[f32; 4]> {
    let mid = |r: &[f32; 4]| (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
    let Some(cur) = under(targets, at) else {
        let far = |r: &[f32; 4]| (mid(r).0 - at.0).hypot(mid(r).1 - at.1);
        return targets.iter().copied().min_by(|a, b| far(a).total_cmp(&far(b)));
    };
    let from = mid(&cur);
    let score = |r: &[f32; 4]| {
        let (dx, dy) = (mid(r).0 - from.0, mid(r).1 - from.1);
        let (along, across) = if d.0 != 0 { (dx * d.0 as f32, dy.abs()) } else { (dy * d.1 as f32, dx.abs()) };
        (along > 1.0 && across <= 2.0 * along).then_some(along + 2.0 * across)
    };
    targets.iter().copied().filter(|r| *r != cur).filter_map(|r| score(&r).map(|sc| (sc, r))).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, r)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::hud::NO_UV;

    fn quad(x: f32, y: f32, w: f32, h: f32, uv: Option<[f32; 4]>) -> Vec<HudVertex> {
        let [u0, v0, u1, v1] = uv.unwrap_or([-1.0, -1.0, -1.0, -1.0]);
        let p = |px: f32, py: f32, u: f32, vv: f32| HudVertex { pos: [px, py], color: [1.0, 1.0, 1.0, 1.0], uv: if uv.is_some() { [u, vv] } else { NO_UV } };
        let (tl, tr, br, bl) = (p(x, y, u0, v0), p(x + w, y, u1, v0), p(x + w, y + h, u1, v1), p(x, y + h, u0, v1));
        vec![tl, tr, br, tl, br, bl]
    }

    /// Inside stays, outside goes, a quad over the edge is cut and its texture is cut with it (linearly).
    #[test]
    fn clip_cuts_quads_and_their_texture() {
        let mut v = quad(0.0, 0.0, 1.0, 1.0, None); // before `from`: untouched
        let from = v.len();
        v.extend(quad(10.0, 10.0, 10.0, 10.0, Some([0.0, 0.0, 1.0, 1.0]))); // inside
        v.extend(quad(100.0, 10.0, 10.0, 10.0, Some([0.0, 0.0, 1.0, 1.0]))); // outside
        v.extend(quad(-10.0, 0.0, 40.0, 10.0, Some([0.0, 0.0, 1.0, 1.0]))); // over the left edge and the right one
        clip(&mut v, from, [0.0, 0.0, 30.0, 30.0]);
        assert_eq!(v.len(), 6 + 6 + 6, "the outside quad is gone");
        let cut = &v[12..];
        let xs: Vec<f32> = cut.iter().map(|p| p.pos[0]).collect();
        assert!(xs.iter().all(|&x| (0.0..=30.0).contains(&x)));
        // the quad ran from -10 to 30 (u 0..1): the visible part is x 0..30, u 0.25..1.0
        assert!((cut[0].uv[0] - 0.25).abs() < 1e-6 && (cut[1].uv[0] - 1.0).abs() < 1e-6, "{:?}", (cut[0].uv, cut[1].uv));
        assert_eq!((cut[0].uv[1], cut[2].uv[1]), (0.0, 1.0), "the other axis is whole");
    }

    /// A flat colour keeps its `NO_UV` marker exactly (the pipeline tells flat from textured by it).
    #[test]
    fn clip_keeps_flat_quads_flat() {
        let mut v = quad(-5.0, -5.0, 20.0, 20.0, None);
        clip(&mut v, 0, [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(v.len(), 6);
        assert!(v.iter().all(|p| p.uv == NO_UV));
    }

    #[test]
    fn wrap_and_fit_stay_inside_their_width() {
        let t = "Draws far terrain as simplified blocks out to 768 blocks.";
        let lines = wrap(t, 100.0);
        assert!(lines.len() > 1 && lines.iter().all(|l| tw(l) <= 100.0), "{lines:?}");
        assert_eq!(lines.join(" "), t, "nothing is lost");
        assert!(wrap("", 50.0).is_empty());
        let f = fit("A very long world name indeed", 60.0);
        assert!(tw(&f) <= 60.0 && f.ends_with(".."), "{f}");
        assert_eq!(fit("short", 60.0), "short");
    }

    /// The thumb stays in its box at both ends and is never smaller than 32.
    #[test]
    fn thumb_stays_in_the_box() {
        for (view, content) in [(172.0, 1400.0), (172.0, 200.0), (206.0, 5000.0)] {
            let max = content - view;
            let (top, h) = thumb(view, content, 0.0, max);
            let (bot, h2) = thumb(view, content, max, max);
            assert!(top == 0.0 && h >= 32.0 && h == h2 && bot + h <= view + 1e-3, "{view} {content}: {bot} {h}");
        }
    }

    /// The pad steps to the neighbour that lies that way, stops at the end of a row, and from off every rect goes to the nearest.
    #[test]
    fn a_pad_steps_between_targets() {
        let (a, b, c) = ([0.0, 0.0, 18.0, 18.0], [18.0, 0.0, 18.0, 18.0], [0.0, 18.0, 18.0, 18.0]);
        let t = [a, b, c];
        assert_eq!(step(&t, (9.0, 9.0), (1, 0)), Some(b));
        assert_eq!(step(&t, (27.0, 9.0), (-1, 0)), Some(a));
        assert_eq!(step(&t, (9.0, 9.0), (0, 1)), Some(c));
        assert_eq!(step(&t, (27.0, 9.0), (1, 0)), None, "nothing lies right of the row");
        assert_eq!(step(&t, (40.0, 9.0), (0, 0)), Some(b), "off every rect: the nearest");
    }
}
