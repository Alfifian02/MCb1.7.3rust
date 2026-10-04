//! Platform-independent frame loop: input -> simulation -> streaming -> render.
//! Platforms only supply GL and events.
use crate::game::{Game, HOTBAR};
use crate::input::Input;
use crate::math;
use crate::renderer::Renderer;
use crate::stream::Streamer;
use crate::ui::{self, HudState, Layout, UiBatch};
use std::time::Instant;

pub struct Engine {
    pub game: Game,
    pub input: Input,
    stream: Streamer,
    renderer: Option<Renderer>,
    batch: UiBatch,
    size: (u32, u32),
    last: Instant,
}

impl Engine {
    pub fn new() -> Self {
        let game = Game::new();
        let mut stream = Streamer::new(crate::game::SEED, Streamer::default_workers());
        stream.adopt(&game.world); // the start-up area is already generated; only meshing remains
        Self { game, input: Input::new(), stream, renderer: None, batch: UiBatch::default(), size: (1, 1), last: Instant::now() }
    }

    /// Called when a GL context + surface become available (also after Android resumes).
    pub fn gfx_ready(&mut self, gl: glow::Context) {
        self.renderer = Some(Renderer::new(gl));
        self.stream.invalidate_meshes(); // meshes are rebuilt by the workers, no hitch on resume
        self.last = Instant::now();
    }

    pub fn gfx_lost(&mut self) {
        self.renderer = None;
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
        if controls.view_cycle {
            self.stream.cycle_radius();
        }
        let dirty = self.game.update(&controls, dt);
        self.stream.on_edit(&dirty);

        let (w, h) = self.size;
        let Some(r) = self.renderer.as_mut() else { return };
        if w == 0 || h == 0 {
            return;
        }

        let p = &self.game.player;
        let center = ((p.pos[0].floor() as i32).div_euclid(16), (p.pos[2].floor() as i32).div_euclid(16));
        let out = self.stream.update(&mut self.game.world, center, true);
        for (cx, cz) in out.unloaded {
            r.remove_chunk(cx, cz);
        }
        for (cx, cz, sections) in out.meshed {
            r.set_chunk_meshes(cx, cz, sections);
        }
        if !dirty.is_empty() {
            r.remesh(&self.game.world, &dirty);
        }

        let p = &self.game.player;
        let eye = p.eye();
        let proj = math::perspective(1.2, w as f32 / h as f32, 0.1, 300.0);
        let mvp = math::mul(&proj, &math::view(eye, p.yaw, p.pitch));
        r.draw(self.size, &mvp, eye, self.stream.fog());
        if let Some(t) = self.game.target {
            r.draw_outline(&mvp, t.block);
        }

        self.batch.clear();
        let layout = Layout::new(w as f32, h as f32);
        let hud = HudState {
            sel: self.game.sel,
            hotbar: &HOTBAR,
            touch_ui: self.input.show_touch_ui(),
            stick: self.input.stick_vis(),
            view_radius: self.stream.radius(),
        };
        ui::build(&mut self.batch, &layout, &hud);
        r.draw_ui(&self.batch, self.size);
    }
}
