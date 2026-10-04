//! Platform-independent frame loop: input -> simulation -> render. Platforms only supply GL and events.
use crate::game::{Game, HOTBAR};
use crate::input::Input;
use crate::math;
use crate::renderer::Renderer;
use crate::ui::{self, Layout, UiBatch};
use std::time::Instant;

pub struct Engine {
    pub game: Game,
    pub input: Input,
    renderer: Option<Renderer>,
    batch: UiBatch,
    size: (u32, u32),
    last: Instant,
}

impl Engine {
    pub fn new() -> Self {
        Self { game: Game::new(), input: Input::new(), renderer: None, batch: UiBatch::default(), size: (1, 1), last: Instant::now() }
    }

    /// Called when a GL context + surface become available (also after Android resumes).
    pub fn gfx_ready(&mut self, gl: glow::Context) {
        let mut r = Renderer::new(gl);
        r.remesh_all(&self.game.world); // edits made before a pause are preserved
        self.renderer = Some(r);
        self.last = Instant::now();
    }

    pub fn gfx_lost(&mut self) {
        self.renderer = None;
    }

    pub fn has_gfx(&self) -> bool {
        self.renderer.is_some()
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h);
        self.input.set_size(w, h);
    }

    pub fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;

        let controls = self.input.controls(dt);
        let dirty = self.game.update(&controls, dt);

        let (w, h) = self.size;
        let Some(r) = self.renderer.as_mut() else { return };
        if w == 0 || h == 0 {
            return;
        }
        if !dirty.is_empty() {
            r.remesh(&self.game.world, &dirty);
        }

        let p = &self.game.player;
        let eye = p.eye();
        let fwd = math::forward(p.yaw, p.pitch);
        let proj = math::perspective(1.2, w as f32 / h as f32, 0.1, 300.0);
        let mvp = math::mul(&proj, &math::view(eye, p.yaw, p.pitch));
        r.draw(self.size, &mvp, eye, fwd);
        if let Some(t) = self.game.target {
            r.draw_outline(&mvp, t.block);
        }

        self.batch.clear();
        let layout = Layout::new(w as f32, h as f32);
        ui::build(&mut self.batch, &layout, self.game.sel, &HOTBAR, self.input.show_touch_ui(), self.input.stick_vis());
        r.draw_ui(&self.batch, self.size);
    }
}
