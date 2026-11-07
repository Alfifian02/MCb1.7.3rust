//! Controller vibration engine: Controlify's `rumble` package (`RumbleManager`, `BasicRumbleEffect`, `ContinuousRumbleEffect`, `RumbleSource`,
//! `RumbleSettings.applyRumbleStrength`) and the effects its `mixins/feature/rumble/*` play, with the numbers copied from them.
//!
//! Pure `std`, no Android and no game types, so it can be tested alone. The pieces:
//! * `RumbleState`: the two motors, `strong` (the heavy, low-frequency one) and `weak` (the light, high-frequency one), 0..1 each.
//! * `Effect`: a fixed list of keyframes (`Effect::constant`, `by_time`, `by_tick`, `empty`, `seq`, `repeat`) or a continuous one that runs
//!   until it is stopped or its heartbeat stops (`Effect::continuous`).
//! * `Manager`: the effect queue of one controller. `tick` is 20 Hz like Minecraft's client tick; the strongest of every motor wins, each effect
//!   scaled by `Strengths` (master x source). It answers with an `Out` for the wire.
//! * `Wire`: turns the manager's per-tick `Out` into the few commands the phone needs (`Cmd`): Android's vibrators take "run this amplitude for
//!   N ms", not "set the motors now", so the same level is not sent again until it is about to run out.
//! * `fx`: the effects themselves, one function per Controlify mixin.
//!
//! Not ported (see `roadmap/controlify-phase5.md` for the list): the server vibration packets and `/vibratecontroller` (`VibrationPacket`...), trigger rumble
//! (`TriggerRumbleState`: Android has no API for the Xbox impulse triggers), the DualSense HD haptics, and the effects of things b1.7.3 does
//! not have (anvil, ender dragon, wither, crossbow, spyglass, goat horn, trident).

/// Minecraft's client tick, the unit of every effect's length.
pub const TICK_SECS: f32 = 0.05;
/// `SDL_RumbleGamepad(..., 5000)`: Controlify starts every level for five seconds and replaces it on the next tick. The wire does the same.
pub const WIRE_MS: u32 = 5000;
/// A level that has run this long is sent again before it ends (the same level would otherwise stop after `WIRE_MS`).
const REFRESH_MS: u64 = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RumbleState {
    pub strong: f32,
    pub weak: f32,
}

impl RumbleState {
    pub const NONE: RumbleState = RumbleState { strong: 0.0, weak: 0.0 };

    pub const fn new(strong: f32, weak: f32) -> Self {
        Self { strong, weak }
    }

    pub fn is_zero(&self) -> bool {
        self.strong == 0.0 && self.weak == 0.0
    }

    pub fn mul(self, m: f32) -> Self {
        Self { strong: self.strong * m, weak: self.weak * m }
    }
}

/// `RumbleSource`. Controlify's lang file also names `global_event` and `misc`, but nothing registers them: the five of `RumbleSource.java`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Master,
    Player,
    World,
    Interaction,
    Gui,
}

impl Source {
    pub const ALL: [Source; 5] = [Source::Master, Source::Player, Source::World, Source::Interaction, Source::Gui];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The id of its `options.txt` line and the label of its slider (`controlify.vibration_strength.controlify.*`).
    pub fn id(self) -> &'static str {
        ["master", "player", "world", "interaction", "gui"][self as usize]
    }
}

/// `RumbleSettings`: whether vibration is on and each source's strength (a multiplier, 0..2 in Controlify's slider, 1 by default).
#[derive(Clone, Debug, PartialEq)]
pub struct Strengths {
    pub enabled: bool,
    pub by_source: [f32; 5],
}

impl Default for Strengths {
    fn default() -> Self {
        Self { enabled: true, by_source: [1.0; 5] }
    }
}

impl Strengths {
    /// `RumbleSettings.applyRumbleStrength`: the master alone for the master source, master x source for the rest.
    pub fn apply(&self, s: RumbleState, src: Source) -> RumbleState {
        let master = self.by_source[Source::Master.index()];
        if src == Source::Master {
            s.mul(master)
        } else {
            s.mul(master * self.by_source[src.index()])
        }
    }
}

/// `Easings.easeInQuad` / `easeOutQuad`.
pub fn ease_in_quad(x: f32) -> f32 {
    x * x
}

pub fn ease_out_quad(x: f32) -> f32 {
    1.0 - (1.0 - x) * (1.0 - x)
}

/// The state function of a continuous effect (Controlify passes a lambda; the ones its mixins use are these).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// `constant(strong, weak)`.
    Constant(RumbleState),
    /// `MultiPlayerGameModeMixin.startRumble`: heavier the harder the block (`defaultDestroyTime`, 0.02 + easeInQuad(min(1, hardness / 20)) x 0.25 strong, 0.01 weak).
    BlockBreak { hardness: f32 },
    /// `slowblock/LocalPlayerMixin`: 0.3 / 0.5 of the length of the move vector, which the caller keeps current with `Manager::set_param`.
    SlowBlock,
    /// `useitem/LocalPlayerMixin` `BLOCK`: a weak ripple, `tick % 4 / 4 x 0.12 + 0.05`.
    Guard,
}

impl Shape {
    fn at(self, tick: u32, param: f32) -> RumbleState {
        match self {
            Shape::Constant(s) => s,
            Shape::BlockBreak { hardness } => RumbleState::new(0.02 + ease_in_quad((hardness / 20.0).min(1.0)) * 0.25, 0.01),
            Shape::SlowBlock => RumbleState::new(0.3 * param, 0.5 * param),
            Shape::Guard => RumbleState::new(0.0, (tick % 4) as f32 / 4.0 * 0.12 + 0.05),
        }
    }
}

#[derive(Clone, Debug)]
enum Kind {
    /// `BasicRumbleEffect`: one state per tick. `until_epoch`: `earlyFinish(finishOnScreenChange())`, the manager's epoch when it started.
    Basic { frames: Vec<RumbleState>, until_epoch: Option<u32> },
    /// `ContinuousRumbleEffect`: `timeout` ticks without a heartbeat ends it (-1: never), `min_time` ticks it always runs.
    Continuous { shape: Shape, timeout: i32, min_time: u32, stopped: bool, age: u32, param: f32 },
}

#[derive(Clone, Debug)]
pub struct Effect {
    kind: Kind,
    /// Kept for parity: Controlify sorts its queue by it but combines the states with a `max`, so it changes nothing about what is felt.
    pub priority: i32,
    tick: u32,
    finished: bool,
}

impl Effect {
    fn basic(frames: Vec<RumbleState>) -> Self {
        Self { kind: Kind::Basic { frames, until_epoch: None }, priority: 0, tick: 0, finished: false }
    }

    /// `BasicRumbleEffect.byTick`.
    pub fn by_tick(f: impl Fn(u32) -> RumbleState, ticks: u32) -> Self {
        Self::basic((0..ticks).map(f).collect())
    }

    /// `BasicRumbleEffect.byTime`: `f` gets 0 at the first tick up to (ticks - 1) / ticks at the last.
    pub fn by_time(f: impl Fn(f32) -> RumbleState, ticks: u32) -> Self {
        Self::by_tick(|t| f(t as f32 / ticks as f32), ticks)
    }

    /// `BasicRumbleEffect.constant`.
    pub fn constant(strong: f32, weak: f32, ticks: u32) -> Self {
        Self::by_tick(|_| RumbleState::new(strong, weak), ticks)
    }

    /// `BasicRumbleEffect.empty`.
    pub fn empty(ticks: u32) -> Self {
        Self::constant(0.0, 0.0, ticks)
    }

    /// `BasicRumbleEffect.seq`: the keyframes one after the other. Like Java's, the result has neither the priority nor the early finish of its parts.
    pub fn seq(parts: &[&Effect]) -> Self {
        Self::basic(parts.iter().flat_map(|e| e.frames().iter().copied()).collect())
    }

    /// `BasicRumbleEffect.repeat`: `count` copies one after the other (0 is treated as 1; Java throws).
    pub fn repeat(&self, count: u32) -> Self {
        let parts: Vec<&Effect> = (0..count.max(1)).map(|_| self).collect();
        Self::seq(&parts)
    }

    pub fn prioritised(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// `earlyFinish(finishOnScreenChange())`: ends when `Manager::set_epoch` is called with another number than the manager has now.
    pub fn until_epoch_changes(mut self, now: u32) -> Self {
        if let Kind::Basic { until_epoch, .. } = &mut self.kind {
            *until_epoch = Some(now);
        }
        self
    }

    /// `ContinuousRumbleEffect.builder()...build()`. `timeout` < 0: no timeout (Java's -1).
    pub fn continuous(shape: Shape, timeout: i32, min_time: u32) -> Self {
        Self { kind: Kind::Continuous { shape, timeout, min_time, stopped: false, age: 0, param: 0.0 }, priority: 0, tick: 0, finished: false }
    }

    /// The keyframes of a basic effect (a continuous one has none).
    pub fn frames(&self) -> &[RumbleState] {
        match &self.kind {
            Kind::Basic { frames, .. } => frames,
            Kind::Continuous { .. } => &[],
        }
    }

    /// `tick()`: one step; `epoch` is the manager's.
    fn step(&mut self, epoch: u32) {
        self.tick += 1;
        match &mut self.kind {
            Kind::Basic { frames, until_epoch } => {
                if self.tick as usize >= frames.len() || until_epoch.is_some_and(|e| e != epoch) {
                    self.finished = true;
                }
            }
            Kind::Continuous { age, .. } => *age += 1,
        }
    }

    /// `currentState()`: the keyframe of the tick just made (Java throws before the first tick; here that is silence).
    fn state(&self) -> RumbleState {
        if self.tick == 0 {
            return RumbleState::NONE;
        }
        match &self.kind {
            Kind::Basic { frames, .. } => frames.get(self.tick as usize - 1).copied().unwrap_or(RumbleState::NONE),
            Kind::Continuous { shape, param, .. } => shape.at(self.tick - 1, *param),
        }
    }

    /// `isFinished()`.
    pub fn is_finished(&self) -> bool {
        match &self.kind {
            Kind::Basic { .. } => self.finished,
            Kind::Continuous { timeout, min_time, stopped, age, .. } => (*stopped || (*timeout > 0 && *age as i32 >= *timeout)) && self.tick >= *min_time,
        }
    }

    pub fn age(&self) -> u32 {
        self.tick
    }
}

/// What the manager wants of the motors after a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// Run the motors at this (already scaled) level.
    Set(RumbleState),
    /// Stop them (sent once, then `Idle` until something plays again).
    Stop,
    /// Nothing to say.
    Idle,
}

/// A handle on a playing effect, to stop it, keep it alive or feed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(u32);

struct Inst {
    id: u32,
    source: Source,
    effect: Effect,
}

/// `RumbleManager`.
#[derive(Default)]
pub struct Manager {
    queue: Vec<Inst>,
    next: u32,
    was_silent: bool,
    epoch: u32,
    was_enabled: bool,
}

impl Manager {
    pub fn new() -> Self {
        Self { was_enabled: true, ..Default::default() }
    }

    /// `play(source, effect)`.
    pub fn play(&mut self, source: Source, effect: Effect) -> Handle {
        self.next += 1;
        self.queue.push(Inst { id: self.next, source, effect });
        Handle(self.next)
    }

    /// `finishOnScreenChange`: the caller bumps this when the open screen changes; effects made with `until_epoch_changes(epoch())` end.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    pub fn set_epoch(&mut self, e: u32) {
        self.epoch = e;
    }

    fn get(&mut self, h: Handle) -> Option<&mut Inst> {
        self.queue.iter_mut().find(|i| i.id == h.0)
    }

    /// `ContinuousRumbleEffect.stop()`: it ends once it has run its minimum time.
    pub fn stop(&mut self, h: Handle) {
        if let Some(Inst { effect: Effect { kind: Kind::Continuous { stopped, .. }, .. }, .. }) = self.get(h) {
            *stopped = true;
        }
    }

    /// `ContinuousRumbleEffect.heartbeat()`: the timeout starts over.
    pub fn heartbeat(&mut self, h: Handle) {
        if let Some(Inst { effect: Effect { kind: Kind::Continuous { age, .. }, .. }, .. }) = self.get(h) {
            *age = 0;
        }
    }

    /// The live number a shape reads (`Shape::SlowBlock`: the length of the move vector).
    pub fn set_param(&mut self, h: Handle, v: f32) {
        if let Some(Inst { effect: Effect { kind: Kind::Continuous { param, .. }, .. }, .. }) = self.get(h) {
            *param = v;
        }
    }

    /// `effect == null || effect.isFinished()`: a handle that is gone counts as finished.
    pub fn is_finished(&self, h: Handle) -> bool {
        self.queue.iter().find(|i| i.id == h.0).map_or(true, |i| i.effect.is_finished())
    }

    /// `clearEffects()`: leaving a world.
    pub fn clear_effects(&mut self) {
        self.queue.clear();
    }

    pub fn is_playing(&self) -> bool {
        !self.queue.is_empty()
    }

    /// `RumbleManager.tick()`, 20 times a second. `silent` is `setSilent` (the effects run on but the motors stay off).
    pub fn tick(&mut self, cfg: &Strengths, silent: bool) -> Out {
        self.queue.retain(|i| !i.effect.is_finished());
        let epoch = self.epoch;
        for i in &mut self.queue {
            i.effect.step(epoch);
        }
        let out = if self.queue.is_empty() {
            self.clear()
        } else {
            let (mut strong, mut weak) = (0.0f32, 0.0f32);
            for i in &self.queue {
                let s = cfg.apply(i.effect.state(), i.source);
                strong = strong.max(s.strong);
                weak = weak.max(s.weak);
            }
            let s = RumbleState::new(strong, weak);
            if s.is_zero() || silent {
                self.clear()
            } else {
                self.was_silent = false;
                Out::Set(s)
            }
        };
        // `RumbleComponent.queueRumble` ignores everything while vibration is off, which would leave a running motor to its 5 s timeout; stop it now.
        if !cfg.enabled {
            let stop = std::mem::replace(&mut self.was_enabled, false);
            return if stop { Out::Stop } else { Out::Idle };
        }
        self.was_enabled = true;
        out
    }

    /// `clearRumble()`: one `NONE`, then nothing until something plays again.
    fn clear(&mut self) -> Out {
        if self.was_silent {
            Out::Idle
        } else {
            self.was_silent = true;
            Out::Stop
        }
    }
}

/// A command for the phone's vibrators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// Run the motors at these amplitudes (0 = that motor off) for `ms`.
    Vibrate { strong: u8, weak: u8, ms: u32 },
    Cancel,
}

/// Turns `Out` into `Cmd`: only a changed level (or one close to running out) is sent.
#[derive(Default)]
pub struct Wire {
    last: Option<(u8, u8)>,
    sent_at: u64,
}

impl Wire {
    /// `now_ms`: any clock that only goes forward.
    pub fn feed(&mut self, out: Out, now_ms: u64) -> Option<Cmd> {
        match out {
            Out::Idle => None,
            Out::Stop => self.cancel(),
            Out::Set(s) => {
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let lvl = (q(s.strong), q(s.weak));
                if lvl == (0, 0) {
                    return self.cancel();
                }
                if self.last == Some(lvl) && now_ms.saturating_sub(self.sent_at) < REFRESH_MS {
                    return None;
                }
                self.last = Some(lvl);
                self.sent_at = now_ms;
                Some(Cmd::Vibrate { strong: lvl.0, weak: lvl.1, ms: WIRE_MS })
            }
        }
    }

    fn cancel(&mut self) -> Option<Cmd> {
        self.last.take().map(|_| Cmd::Cancel)
    }
}

/// The effects Controlify plays, by the mixin that plays them. Each returns the source it is played on.
pub mod fx {
    use super::*;

    /// `damage/ClientPacketListenerMixin.playIncomingDamageRumble`.
    pub fn damage_taken() -> (Source, Effect) {
        (Source::Player, Effect::constant(0.8, 0.5, 5))
    }

    /// `playOutgoingDamageRumble`: a hit you land.
    pub fn damage_dealt() -> (Source, Effect) {
        (Source::Interaction, Effect::seq(&[&Effect::constant(0.18, 0.4, 1), &Effect::constant(0.06, 0.16, 1)]))
    }

    /// `blockbreak/MultiPlayerGameModeMixin.startRumble`: from the first dig tick until the block is gone or the dig stops (`minTime(1)`).
    pub fn block_break(hardness: f32) -> (Source, Effect) {
        (Source::Interaction, Effect::continuous(Shape::BlockBreak { hardness }, -1, 1))
    }

    /// `explosion/ClientPacketListenerMixin.calculateMagnitude`: 0 beyond 32 blocks of the blast's surface, cubed in between, and a small blast is
    /// weaker (full strength at radius 6).
    pub fn explosion_magnitude(distance: f32, radius: f32) -> f32 {
        const FALLOFF: f32 = 32.0;
        const FULL_RADIUS: f32 = 6.0;
        let radius = radius.max(0.0);
        let surface = (distance - radius).max(0.0);
        if surface >= FALLOFF {
            return 0.0;
        }
        let proximity = 1.0 - surface / FALLOFF;
        let by_radius = (radius / FULL_RADIUS).sqrt().min(1.0);
        proximity * proximity * proximity * by_radius
    }

    /// `onClientExplosion`: the boom (2 ticks) and a decaying aftershock (8 ticks). `None` when too far to feel.
    pub fn explosion(distance: f32, radius: f32) -> Option<(Source, Effect)> {
        let m = explosion_magnitude(distance, radius);
        if m <= 0.0 {
            return None;
        }
        let boom = Effect::constant(m, m, 2);
        let after = Effect::by_time(
            |t| {
                let d = (1.0 - t) * (1.0 - t);
                RumbleState::new(m * 0.8 * d, m * 0.2 * d)
            },
            8,
        );
        Some((Source::World, Effect::seq(&[&boom, &after])))
    }

    /// `explosion/LightningBoltMixin`: a full boom (6 ticks), then the weak motor fading from 1 to 0.82 over 10.
    pub fn lightning() -> (Source, Effect) {
        let boom = Effect::constant(1.0, 0.2, 6);
        let rest = Effect::by_time(|t| RumbleState::new(0.0, 1.0 - t * 0.2), 10);
        (Source::World, Effect::seq(&[&boom, &rest]))
    }

    /// `itembreak/LocalPlayerMixin`: the heavy motor for 5 ticks, the weak one for 10.
    pub fn item_break() -> (Source, Effect) {
        (Source::Player, Effect::by_tick(|t| RumbleState::new(if t <= 4 { 1.0 } else { 0.0 }, 1.0), 10))
    }

    /// `waterland/LocalPlayerMixin`: `impact` is `min(1, sqrt(vx^2 x 0.2 + vy^2 + vz^2 x 0.2) x 0.2)` of the fall in (blocks a tick). Under 0.05 nothing.
    pub fn splash(impact: f32) -> Option<(Source, Effect)> {
        if impact < 0.05 {
            return None;
        }
        let m = (impact / 0.5).min(1.0);
        let ticks = if impact < 0.25 { 10 } else { 20 };
        Some((Source::Player, Effect::by_time(|t| RumbleState::new(m * (1.0 - t), m * 0.5), ticks)))
    }

    /// `useitem` `EAT, DRINK`: a light constant buzz for as long as the meal lasts.
    pub fn eating() -> (Source, Effect) {
        (Source::Interaction, Effect::continuous(Shape::Constant(RumbleState::new(0.1, 0.2)), -1, 0))
    }

    /// `useitem` `BLOCK`: the raised guard.
    pub fn guard() -> (Source, Effect) {
        (Source::Interaction, Effect::continuous(Shape::Guard, -1, 0))
    }

    /// `fishing/FishingHookMixin`: a faint weak-motor hum while a fish is on the line.
    pub fn bite() -> (Source, Effect) {
        (Source::Interaction, Effect::continuous(Shape::Constant(RumbleState::new(0.0, 0.05)), -1, 0))
    }

    /// `slowblock/LocalPlayerMixin`: walking through soul sand. `timeout(100)`: it ends by itself 100 ticks after the last `heartbeat`.
    pub fn slow_block() -> (Source, Effect) {
        (Source::Player, Effect::continuous(Shape::SlowBlock, 100, 0))
    }

    /// `ControllerConfigScreenFactory`'s Test Vibration: three ramps up and down on the weak motor, then ten alternations of the strong and the
    /// weak one; it ends when the screen is left (`finishOnScreenChange`, here `epoch`: the manager's current one).
    pub fn test(epoch: u32) -> (Source, Effect) {
        let up = Effect::by_time(|t| RumbleState::new(0.0, t), 20);
        let down = Effect::by_time(|t| RumbleState::new(0.0, 1.0 - t), 20);
        let ramps = Effect::seq(&[&up, &down]).repeat(3);
        let pair = Effect::seq(&[&Effect::constant(1.0, 0.0, 5), &Effect::constant(0.0, 1.0, 5)]).repeat(10);
        (Source::Master, Effect::seq(&[&ramps, &pair]).until_epoch_changes(epoch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &mut Manager, n: usize) -> Vec<Out> {
        let cfg = Strengths::default();
        (0..n).map(|_| m.tick(&cfg, false)).collect()
    }

    fn set(s: f32, w: f32) -> Out {
        Out::Set(RumbleState::new(s, w))
    }

    #[test]
    fn a_basic_effect_plays_each_keyframe_once_and_then_stops_the_motors() {
        let mut m = Manager::new();
        let (src, e) = fx::damage_taken();
        m.play(src, e);
        let outs = run(&mut m, 8);
        assert_eq!(&outs[..5], &[set(0.8, 0.5); 5], "damage: 5 ticks of (0.8, 0.5)");
        assert_eq!(outs[5], Out::Stop, "the tick after the last one clears the motors");
        assert_eq!(&outs[6..], &[Out::Idle, Out::Idle], "and then says nothing");
        assert!(!m.is_playing());
    }

    #[test]
    fn the_manager_sends_one_stop_when_idle_from_the_start_like_java() {
        let mut m = Manager::new();
        assert_eq!(run(&mut m, 3), vec![Out::Stop, Out::Idle, Out::Idle]);
    }

    #[test]
    fn the_strongest_of_each_motor_wins_across_effects() {
        let mut m = Manager::new();
        m.play(Source::Player, Effect::constant(0.8, 0.1, 3));
        m.play(Source::Player, Effect::constant(0.2, 0.6, 3));
        assert_eq!(run(&mut m, 1), vec![set(0.8, 0.6)]);
    }

    #[test]
    fn master_times_source_scales_an_effect_and_master_alone_scales_the_master_source() {
        let mut cfg = Strengths::default();
        cfg.by_source = [0.5, 0.5, 1.0, 1.0, 1.0];
        let s = RumbleState::new(1.0, 1.0);
        assert_eq!(cfg.apply(s, Source::Master), RumbleState::new(0.5, 0.5));
        assert_eq!(cfg.apply(s, Source::Player), RumbleState::new(0.25, 0.25));
        assert_eq!(cfg.apply(s, Source::World), RumbleState::new(0.5, 0.5));
        let mut m = Manager::new();
        m.play(Source::Player, Effect::constant(1.0, 1.0, 2));
        assert_eq!(m.tick(&cfg, false), set(0.25, 0.25));
    }

    #[test]
    fn a_silent_manager_keeps_the_effects_running_but_the_motors_off() {
        let mut m = Manager::new();
        m.play(Source::Player, Effect::constant(1.0, 1.0, 4));
        let cfg = Strengths::default();
        assert_eq!(m.tick(&cfg, false), set(1.0, 1.0));
        assert_eq!(m.tick(&cfg, true), Out::Stop);
        assert_eq!(m.tick(&cfg, true), Out::Idle);
        assert_eq!(m.tick(&cfg, false), set(1.0, 1.0), "the 4th keyframe, still there");
    }

    #[test]
    fn vibration_off_stops_a_running_motor_once_and_sends_nothing_after() {
        let mut m = Manager::new();
        m.play(Source::Player, Effect::constant(1.0, 1.0, 10));
        let mut cfg = Strengths::default();
        assert_eq!(m.tick(&cfg, false), set(1.0, 1.0));
        cfg.enabled = false;
        assert_eq!(m.tick(&cfg, false), Out::Stop);
        assert_eq!(m.tick(&cfg, false), Out::Idle);
        cfg.enabled = true;
        assert_eq!(m.tick(&cfg, false), set(1.0, 1.0));
    }

    #[test]
    fn seq_and_repeat_join_keyframes_and_drop_the_parts_priority() {
        let a = Effect::constant(1.0, 0.0, 2).prioritised(5);
        let b = Effect::constant(0.0, 1.0, 1);
        let s = Effect::seq(&[&a, &b]);
        assert_eq!(s.frames(), &[RumbleState::new(1.0, 0.0), RumbleState::new(1.0, 0.0), RumbleState::new(0.0, 1.0)]);
        assert_eq!(s.priority, 0);
        assert_eq!(s.repeat(3).frames().len(), 9);
        assert_eq!(s.repeat(1).frames().len(), 3);
    }

    #[test]
    fn by_time_runs_from_zero_up_to_just_short_of_one() {
        let e = Effect::by_time(|t| RumbleState::new(t, 0.0), 4);
        let v: Vec<f32> = e.frames().iter().map(|s| s.strong).collect();
        assert_eq!(v, vec![0.0, 0.25, 0.5, 0.75]);
    }

    #[test]
    fn a_continuous_effect_runs_until_stopped_and_always_for_its_minimum_time() {
        let mut m = Manager::new();
        let (src, e) = fx::block_break(1.5);
        let h = m.play(src, e);
        let cfg = Strengths::default();
        let first = m.tick(&cfg, false);
        let want = 0.02 + (1.5f32 / 20.0).powi(2) * 0.25;
        match first {
            Out::Set(s) => {
                assert!((s.strong - want).abs() < 1e-6 && (s.weak - 0.01).abs() < 1e-6, "{s:?}");
            }
            o => panic!("{o:?}"),
        }
        for _ in 0..30 {
            assert!(matches!(m.tick(&cfg, false), Out::Set(_)), "it never ends by itself");
        }
        m.stop(h);
        assert!(m.is_finished(h));
        assert_eq!(m.tick(&cfg, false), Out::Stop, "removed on the next tick");
    }

    #[test]
    fn an_instant_break_still_buzzes_for_one_tick() {
        // `onInstabreakBlockSurvival`: start, then stop in the same tick; `minTime(1)` keeps it for one.
        let mut m = Manager::new();
        let (src, e) = fx::block_break(0.0);
        let h = m.play(src, e);
        m.stop(h);
        assert!(!m.is_finished(h), "it has not ticked yet");
        let cfg = Strengths::default();
        assert_eq!(m.tick(&cfg, false), set(0.02, 0.01));
        assert_eq!(m.tick(&cfg, false), Out::Stop);
    }

    #[test]
    fn a_heartbeat_keeps_a_timeout_effect_alive_and_without_one_it_ends() {
        let mut m = Manager::new();
        let (src, e) = fx::slow_block();
        let h = m.play(src, e);
        m.set_param(h, 1.0);
        let cfg = Strengths::default();
        for _ in 0..250 {
            m.heartbeat(h);
            assert_eq!(m.tick(&cfg, false), set(0.3, 0.5));
        }
        for _ in 0..98 {
            assert!(matches!(m.tick(&cfg, false), Out::Set(_)));
            assert!(!m.is_finished(h));
        }
        assert!(matches!(m.tick(&cfg, false), Out::Set(_)), "its 99th tick without a heartbeat still plays");
        assert!(m.is_finished(h), "the age reaches the timeout of 100 ticks");
        assert_eq!(m.tick(&cfg, false), Out::Stop, "and it is gone on the next");
    }

    #[test]
    fn the_slow_block_effect_follows_the_move_vector() {
        let mut m = Manager::new();
        let (src, e) = fx::slow_block();
        let h = m.play(src, e);
        m.set_param(h, 0.5);
        let cfg = Strengths::default();
        assert_eq!(m.tick(&cfg, false), set(0.15, 0.25));
    }

    #[test]
    fn a_handle_that_is_gone_counts_as_finished() {
        let m = Manager::new();
        assert!(m.is_finished(Handle(99)));
    }

    #[test]
    fn leaving_the_screen_ends_the_test_vibration() {
        let mut m = Manager::new();
        let (src, e) = fx::test(m.epoch());
        assert_eq!(e.frames().len(), 120 + 100);
        m.play(src, e);
        let cfg = Strengths::default();
        for _ in 0..10 {
            m.tick(&cfg, false);
        }
        assert!(m.is_playing());
        m.set_epoch(m.epoch() + 1);
        assert!(matches!(m.tick(&cfg, false), Out::Set(_)), "the tick that sees the new screen still plays its keyframe");
        assert_eq!(m.tick(&cfg, false), Out::Stop);
        assert!(!m.is_playing());
    }

    #[test]
    fn explosion_strength_falls_off_cubically_and_with_a_smaller_radius() {
        use fx::explosion_magnitude as mag;
        assert_eq!(mag(40.0, 4.0), 0.0, "36 blocks from the surface: out of range");
        assert_eq!(mag(36.0, 4.0), 0.0, "exactly 32 from the surface");
        let tnt = mag(4.0, 4.0); // at the surface: proximity 1, sqrt(4/6)
        assert!((tnt - (4.0f32 / 6.0).sqrt()).abs() < 1e-6, "{tnt}");
        assert!((mag(0.0, 8.0) - 1.0).abs() < 1e-6, "a radius over 6 is capped at full");
        let half = mag(4.0 + 16.0, 4.0); // 16 from the surface: proximity 0.5
        assert!((half - 0.125 * (4.0f32 / 6.0).sqrt()).abs() < 1e-6);
        assert!(fx::explosion(100.0, 4.0).is_none());
    }

    #[test]
    fn an_explosion_booms_for_two_ticks_and_then_decays() {
        let (_, e) = fx::explosion(0.0, 6.0).unwrap();
        let f = e.frames();
        assert_eq!(f.len(), 10);
        assert_eq!((f[0], f[1]), (RumbleState::new(1.0, 1.0), RumbleState::new(1.0, 1.0)));
        assert_eq!(f[2], RumbleState::new(0.8, 0.2), "t = 0: no decay yet");
        assert!((f[9].strong - 0.8 / 64.0).abs() < 1e-6, "t = 7/8: (1/8)^2 of 0.8");
    }

    #[test]
    fn lightning_booms_then_fades_the_weak_motor() {
        let (_, e) = fx::lightning();
        let f = e.frames();
        assert_eq!(f.len(), 16);
        assert_eq!(f[0], RumbleState::new(1.0, 0.2));
        assert_eq!(f[6], RumbleState::new(0.0, 1.0));
        assert!((f[15].weak - 0.82).abs() < 1e-6);
    }

    #[test]
    fn a_splash_is_scaled_by_the_impact_and_short_for_a_gentle_one() {
        assert!(fx::splash(0.04).is_none());
        let (_, soft) = fx::splash(0.1).unwrap();
        assert_eq!(soft.frames().len(), 10);
        assert!((soft.frames()[0].strong - 0.2).abs() < 1e-6, "0.1 / 0.5 of the strong motor at t = 0");
        let (_, hard) = fx::splash(0.6).unwrap();
        assert_eq!(hard.frames().len(), 20);
        assert_eq!(hard.frames()[0], RumbleState::new(1.0, 0.5), "the multiplier is capped at 1");
    }

    #[test]
    fn a_tool_that_breaks_shakes_the_strong_motor_for_five_ticks() {
        let (_, e) = fx::item_break();
        let strong: Vec<f32> = e.frames().iter().map(|s| s.strong).collect();
        assert_eq!(strong, vec![1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(e.frames().iter().all(|s| s.weak == 1.0));
    }

    #[test]
    fn the_guard_ripples_on_the_weak_motor() {
        let mut m = Manager::new();
        let (src, e) = fx::guard();
        m.play(src, e);
        let cfg = Strengths::default();
        let w: Vec<f32> = (0..5).map(|_| if let Out::Set(s) = m.tick(&cfg, false) { s.weak } else { -1.0 }).collect();
        let want = [0.05, 0.08, 0.11, 0.14, 0.05];
        for (a, b) in w.iter().zip(want) {
            assert!((a - b).abs() < 1e-6, "{w:?}");
        }
    }

    #[test]
    fn the_wire_sends_a_level_once_and_again_before_it_runs_out() {
        let mut w = Wire::default();
        let lvl = set(1.0, 0.5);
        assert_eq!(w.feed(lvl, 0), Some(Cmd::Vibrate { strong: 255, weak: 128, ms: 5000 }));
        assert_eq!(w.feed(lvl, 50), None);
        assert_eq!(w.feed(lvl, 3950), None);
        assert!(w.feed(lvl, 4000).is_some(), "refreshed after 4 s");
        assert_eq!(w.feed(set(0.2, 0.5), 4050), Some(Cmd::Vibrate { strong: 51, weak: 128, ms: 5000 }), "a new level at once");
        assert_eq!(w.feed(Out::Stop, 4100), Some(Cmd::Cancel));
        assert_eq!(w.feed(Out::Stop, 4150), None, "already stopped");
        assert_eq!(w.feed(Out::Idle, 4200), None);
    }

    #[test]
    fn a_level_that_rounds_to_nothing_cancels_instead_of_vibrating_at_zero() {
        let mut w = Wire::default();
        assert!(w.feed(set(0.5, 0.5), 0).is_some());
        assert_eq!(w.feed(set(0.001, 0.001), 50), Some(Cmd::Cancel));
        assert_eq!(w.feed(set(0.001, 0.0), 100), None);
    }
}
