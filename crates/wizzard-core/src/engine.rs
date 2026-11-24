//! Deterministic per-bulb lighting engine and optional WiZ network runner.
//!
//! [`BulbEngine`] is pure computation parameterised over a [`Clock`] and
//! [`FrameSink`], so it compiles for `wasm32-unknown-unknown` and tests run
//! deterministically without sleeping or opening sockets.
//!
//! Behind the `net` feature, [`WizBulbSink`] adapts [`wizlight::BulbStream`]
//! and [`BulbRunner`] drives a [`BulbEngine`] on a Tokio task with immediate
//! reaction to interactive config changes and graceful frame-boundary shutdown.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::colour::{
    ColourMixer, Hsv, RawRgbMixer, Rgb, WireFrame, clamp_unit, compose_frame, compose_value,
};
use crate::config::{BulbConfig, DEFAULT_TICK_RATE_HZ};
use crate::source::{
    BrightnessSource, ColourSource, ConstantBrightnessSource, ConstantColourSource, Inputs,
};

/// Errors produced by a [`FrameSink`] when submitting a frame.
#[derive(Debug, Error)]
pub enum SinkError {
    /// Underlying transport or encoder rejected the frame.
    #[error("failed to submit bulb frame: {0}")]
    Transport(String),
}

/// Monotonic time source for driving [`BulbEngine`].
pub trait Clock: Send {
    /// Elapsed monotonic time since engine start.
    fn now(&self) -> Duration;
}

/// Manually controlled [`Clock`] for deterministic tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ManualClock {
    now: Duration,
}

impl ManualClock {
    /// Creates a clock starting at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            now: Duration::ZERO,
        }
    }

    /// Advances the clock by `delta`.
    pub fn advance(&mut self, delta: Duration) {
        self.now = self.now.saturating_add(delta);
    }

    /// Sets the clock to an exact elapsed timestamp.
    pub fn set(&mut self, now: Duration) {
        self.now = now;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.now
    }
}

/// Output destination for composed [`WireFrame`] values.
pub trait FrameSink: Send {
    /// Submits a frame to the target bulb.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if the frame could not be encoded or handed off.
    fn send(&mut self, frame: WireFrame) -> Result<(), SinkError>;

    /// Flushes any buffered state during clean engine shutdown.
    fn flush(&mut self) {}
}

/// Live preview snapshot for a single bulb.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreviewSnapshot {
    /// Whether the bulb is currently enabled (powered on).
    pub enabled: bool,
    /// Last sampled HSV colour.
    pub colour: Hsv,
    /// Last sampled brightness scalar (`0.0..=1.0`).
    pub brightness: f32,
    /// Effective composed brightness (`0.0..=1.0`), `0.0` when disabled.
    pub effective_v: f32,
    /// Effective sRGB colour swatch for UI previews.
    pub rgb: Rgb,
    /// Last composed wire frame.
    pub frame: WireFrame,
}

impl PreviewSnapshot {
    fn from_state(enabled: bool, colour: Hsv, brightness: f32, mixer: &dyn ColourMixer) -> Self {
        let brightness = clamp_unit(brightness);
        let effective_v = if enabled {
            compose_value(colour, brightness)
        } else {
            0.0
        };
        let frame = if enabled {
            compose_frame(colour, brightness, mixer)
        } else {
            WireFrame::Off
        };
        let rgb = Hsv::new(colour.h, colour.s, effective_v).to_rgb();
        Self {
            enabled,
            colour,
            brightness,
            effective_v,
            rgb,
            frame,
        }
    }
}

/// Converts a configured tick rate in Hz into a frame interval.
///
/// A zero tick rate falls back to [`DEFAULT_TICK_RATE_HZ`].
#[must_use]
pub fn tick_interval(tick_rate_hz: u16) -> Duration {
    let hz = if tick_rate_hz == 0 {
        DEFAULT_TICK_RATE_HZ
    } else {
        tick_rate_hz
    };
    Duration::from_secs_f64(1.0 / f64::from(hz))
}

/// Single-bulb render engine combining independent colour and brightness
/// sources into [`WireFrame`] updates.
pub struct BulbEngine<S: FrameSink> {
    enabled: bool,
    colour_source: Box<dyn ColourSource>,
    brightness_source: Box<dyn BrightnessSource>,
    mixer: Box<dyn ColourMixer>,
    sink: S,
    last_sent: Option<WireFrame>,
    preview: PreviewSnapshot,
    dirty: bool,
}

impl<S: FrameSink> BulbEngine<S> {
    /// Builds an engine from a [`BulbConfig`] using [`RawRgbMixer`].
    #[must_use]
    pub fn from_config(config: &BulbConfig, sink: S) -> Self {
        let mixer: Box<dyn ColourMixer> = Box::new(RawRgbMixer);
        let colour = config.colour.constant.colour;
        let brightness = config.brightness.constant.value;
        let preview =
            PreviewSnapshot::from_state(config.enabled, colour, brightness, mixer.as_ref());
        Self {
            enabled: config.enabled,
            colour_source: Box::new(ConstantColourSource::new(colour)),
            brightness_source: Box::new(ConstantBrightnessSource::new(brightness)),
            mixer,
            sink,
            last_sent: None,
            preview,
            dirty: true,
        }
    }

    /// Replaces the [`ColourMixer`] used to map `(H, S)` into emitter channels.
    #[must_use]
    pub fn with_mixer(mut self, mixer: Box<dyn ColourMixer>) -> Self {
        self.mixer = mixer;
        self.dirty = true;
        self
    }

    /// Replaces engine state from a full [`BulbConfig`].
    pub fn apply_config(&mut self, config: &BulbConfig) {
        self.enabled = config.enabled;
        self.colour_source = Box::new(ConstantColourSource::new(config.colour.constant.colour));
        self.brightness_source = Box::new(ConstantBrightnessSource::new(
            config.brightness.constant.value,
        ));
        self.dirty = true;
    }

    /// Sets the bulb power state.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.dirty = true;
        }
    }

    /// Switches the colour pipeline to a constant HSV colour.
    pub fn set_constant_colour(&mut self, colour: Hsv) {
        self.colour_source = Box::new(ConstantColourSource::new(colour));
        self.dirty = true;
    }

    /// Switches the brightness pipeline to a constant scalar (`0.0..=1.0`).
    pub fn set_constant_brightness(&mut self, value: f32) {
        self.brightness_source = Box::new(ConstantBrightnessSource::new(value));
        self.dirty = true;
    }

    /// Replaces the colour source with a custom [`ColourSource`].
    pub fn set_colour_source(&mut self, source: Box<dyn ColourSource>) {
        self.colour_source = source;
        self.dirty = true;
    }

    /// Replaces the brightness source with a custom [`BrightnessSource`].
    pub fn set_brightness_source(&mut self, source: Box<dyn BrightnessSource>) {
        self.brightness_source = source;
        self.dirty = true;
    }

    /// Forces the next [`tick`](Self::tick) to transmit even if the composed
    /// frame matches the previous one (useful after reconnecting to a bulb).
    pub fn force_resend(&mut self) {
        self.last_sent = None;
        self.dirty = true;
    }

    /// Returns the most recent [`PreviewSnapshot`].
    #[must_use]
    pub const fn preview(&self) -> PreviewSnapshot {
        self.preview
    }

    /// Returns a shared reference to the underlying sink.
    #[must_use]
    pub const fn sink(&self) -> &S {
        &self.sink
    }

    /// Consumes the engine and returns its underlying sink.
    #[must_use]
    pub fn into_sink(self) -> S {
        self.sink
    }

    /// Samples both sources at timestamp `t`, updates [`preview`](Self::preview),
    /// and transmits to the sink only when the composed [`WireFrame`] changed.
    ///
    /// Returns `Ok(Some(frame))` if a frame was emitted, or `Ok(None)` if the
    /// output matched the last sent frame.
    ///
    /// # Errors
    ///
    /// Propagates [`SinkError`] if the sink rejects or fails to transmit the frame.
    pub fn tick(&mut self, t: Duration, inputs: &Inputs) -> Result<Option<WireFrame>, SinkError> {
        let colour = self.colour_source.sample(t, inputs);
        let brightness = clamp_unit(self.brightness_source.sample(t, inputs));
        let preview =
            PreviewSnapshot::from_state(self.enabled, colour, brightness, self.mixer.as_ref());
        self.preview = preview;
        self.dirty = false;

        if self.last_sent == Some(preview.frame) {
            return Ok(None);
        }

        self.sink.send(preview.frame)?;
        self.last_sent = Some(preview.frame);
        Ok(Some(preview.frame))
    }

    /// Convenience wrapper that reads `t` from `clock` and runs [`tick`](Self::tick).
    ///
    /// # Errors
    ///
    /// Propagates [`SinkError`] from [`tick`](Self::tick).
    pub fn tick_with_clock<C: Clock>(
        &mut self,
        clock: &C,
        inputs: &Inputs,
    ) -> Result<Option<WireFrame>, SinkError> {
        self.tick(clock.now(), inputs)
    }

    /// Stops cleanly on a frame boundary without turning the bulb off.
    ///
    /// If a configuration update has not yet been emitted, transmits one final
    /// coherent frame first, then flushes the sink so bulbs hold their last colour.
    ///
    /// # Errors
    ///
    /// Propagates [`SinkError`] if transmitting a pending final frame fails.
    pub fn shutdown(
        &mut self,
        t: Duration,
        inputs: &Inputs,
    ) -> Result<Option<WireFrame>, SinkError> {
        let emitted = if self.dirty {
            self.tick(t, inputs)?
        } else {
            None
        };
        self.sink.flush();
        Ok(emitted)
    }
}

#[cfg(feature = "net")]
mod net_runner {
    use tokio::sync::{mpsc, watch};
    use tokio::task::JoinHandle;
    use tokio::time::{Instant, MissedTickBehavior, interval};
    use wizlight::{Bulb, BulbStream, StreamConfig, StreamCounters};

    use super::{BulbEngine, Clock, FrameSink, SinkError, tick_interval};
    use crate::colour::Hsv;
    use crate::config::BulbConfig;
    use crate::engine::PreviewSnapshot;
    use crate::source::Inputs;

    /// Monotonic [`Clock`] backed by [`tokio::time::Instant`].
    #[derive(Clone, Copy, Debug)]
    pub struct SystemClock {
        start: Instant,
    }

    impl Default for SystemClock {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SystemClock {
        /// Starts a monotonic clock anchored at the current instant.
        #[must_use]
        pub fn new() -> Self {
            Self {
                start: Instant::now(),
            }
        }
    }

    impl Clock for SystemClock {
        fn now(&self) -> std::time::Duration {
            self.start.elapsed()
        }
    }

    /// [`FrameSink`] forwarding composed frames onto a [`wizlight::BulbStream`].
    #[derive(Debug)]
    pub struct WizBulbSink {
        stream: BulbStream,
    }

    impl WizBulbSink {
        /// Starts a rate-limited [`BulbStream`] on `bulb` using `tick_rate_hz`.
        ///
        /// # Panics
        ///
        /// Panics if called outside a Tokio runtime.
        #[must_use]
        pub fn new(bulb: &Bulb, tick_rate_hz: u16) -> Self {
            let config = StreamConfig {
                min_interval: tick_interval(tick_rate_hz),
            };
            Self {
                stream: bulb.stream_with_config(config),
            }
        }

        /// Returns a snapshot of the underlying stream's counters.
        #[must_use]
        pub fn counters(&self) -> StreamCounters {
            self.stream.counters()
        }

        /// Flushes the newest pending frame and shuts down the stream task.
        pub async fn shutdown(self) -> StreamCounters {
            self.stream.shutdown().await
        }
    }

    impl FrameSink for WizBulbSink {
        fn send(&mut self, frame: crate::colour::WireFrame) -> Result<(), SinkError> {
            let pilot = frame.to_pilot_builder();
            self.stream
                .send(&pilot)
                .map_err(|err| SinkError::Transport(err.to_string()))
        }
    }

    enum RunnerCommand {
        ApplyConfig(Box<BulbConfig>),
        SetEnabled(bool),
        SetConstantColour(Hsv),
        SetConstantBrightness(f32),
        ForceResend,
        Shutdown,
    }

    /// Live Tokio task handle driving one [`BulbEngine`] against a [`Bulb`].
    ///
    /// Interactive updates (`set_constant_colour`, `set_constant_brightness`,
    /// `set_enabled`) execute immediately on arrival while [`WizBulbSink`]
    /// enforces the configured hardware rate limit and coalesces stale frames.
    pub struct BulbRunner {
        tx: mpsc::UnboundedSender<RunnerCommand>,
        preview_rx: watch::Receiver<PreviewSnapshot>,
        task: Option<JoinHandle<StreamCounters>>,
    }

    impl BulbRunner {
        /// Spawns a background engine loop for `bulb` configured from `config`.
        ///
        /// # Panics
        ///
        /// Panics if called outside a Tokio runtime.
        #[must_use]
        pub fn spawn(bulb: &Bulb, config: BulbConfig, tick_rate_hz: u16) -> Self {
            let sink = WizBulbSink::new(bulb, tick_rate_hz);
            let engine = BulbEngine::from_config(&config, sink);
            let initial_preview = engine.preview();
            let (tx, rx) = mpsc::unbounded_channel();
            let (preview_tx, preview_rx) = watch::channel(initial_preview);
            let period = tick_interval(tick_rate_hz);

            let task = tokio::spawn(drive_runner(engine, period, rx, preview_tx));
            Self {
                tx,
                preview_rx,
                task: Some(task),
            }
        }

        /// Applies a full [`BulbConfig`] update.
        pub fn apply_config(&self, config: BulbConfig) {
            let _ = self.tx.send(RunnerCommand::ApplyConfig(Box::new(config)));
        }

        /// Toggles bulb power on or off.
        pub fn set_enabled(&self, enabled: bool) {
            let _ = self.tx.send(RunnerCommand::SetEnabled(enabled));
        }

        /// Updates the constant HSV colour immediately.
        pub fn set_constant_colour(&self, colour: Hsv) {
            let _ = self.tx.send(RunnerCommand::SetConstantColour(colour));
        }

        /// Updates the constant brightness scalar (`0.0..=1.0`) immediately.
        pub fn set_constant_brightness(&self, value: f32) {
            let _ = self.tx.send(RunnerCommand::SetConstantBrightness(value));
        }

        /// Forces the engine to re-emit the current frame on the next tick.
        pub fn force_resend(&self) {
            let _ = self.tx.send(RunnerCommand::ForceResend);
        }

        /// Returns the latest [`PreviewSnapshot`].
        #[must_use]
        pub fn preview(&self) -> PreviewSnapshot {
            *self.preview_rx.borrow()
        }

        /// Subscribes to live [`PreviewSnapshot`] updates.
        #[must_use]
        pub fn subscribe_preview(&self) -> watch::Receiver<PreviewSnapshot> {
            self.preview_rx.clone()
        }

        /// Stops the engine cleanly on a frame boundary and flushes the final
        /// pending UDP frame without turning the bulb off.
        pub async fn shutdown(mut self) -> StreamCounters {
            let _ = self.tx.send(RunnerCommand::Shutdown);
            if let Some(task) = self.task.take() {
                task.await.unwrap_or_default()
            } else {
                StreamCounters::default()
            }
        }
    }

    impl Drop for BulbRunner {
        fn drop(&mut self) {
            let _ = self.tx.send(RunnerCommand::Shutdown);
        }
    }

    async fn drive_runner(
        mut engine: BulbEngine<WizBulbSink>,
        period: std::time::Duration,
        mut rx: mpsc::UnboundedReceiver<RunnerCommand>,
        preview_tx: watch::Sender<PreviewSnapshot>,
    ) -> StreamCounters {
        let clock = SystemClock::new();
        let inputs = Inputs::default();
        let mut ticker = interval(period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let _ = engine.tick_with_clock(&clock, &inputs);
        let _ = preview_tx.send(engine.preview());

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let _ = engine.tick_with_clock(&clock, &inputs);
                    let _ = preview_tx.send(engine.preview());
                }
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else {
                        break;
                    };
                    match cmd {
                        RunnerCommand::ApplyConfig(cfg) => engine.apply_config(&cfg),
                        RunnerCommand::SetEnabled(on) => engine.set_enabled(on),
                        RunnerCommand::SetConstantColour(c) => engine.set_constant_colour(c),
                        RunnerCommand::SetConstantBrightness(v) => engine.set_constant_brightness(v),
                        RunnerCommand::ForceResend => engine.force_resend(),
                        RunnerCommand::Shutdown => break,
                    }
                    let _ = engine.tick_with_clock(&clock, &inputs);
                    let _ = preview_tx.send(engine.preview());
                }
            }
        }

        let _ = engine.shutdown(clock.now(), &inputs);
        let _ = preview_tx.send(engine.preview());
        engine.into_sink().shutdown().await
    }
}

#[cfg(feature = "net")]
pub use net_runner::{BulbRunner, SystemClock, WizBulbSink};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colour::Channels;
    use crate::config::ConstantBrightnessConfig;

    #[derive(Default)]
    struct RecordingSink {
        frames: Vec<WireFrame>,
        flushed: usize,
        fail_next: bool,
    }

    impl FrameSink for RecordingSink {
        fn send(&mut self, frame: WireFrame) -> Result<(), SinkError> {
            if self.fail_next {
                self.fail_next = false;
                return Err(SinkError::Transport("injected error".into()));
            }
            self.frames.push(frame);
            Ok(())
        }

        fn flush(&mut self) {
            self.flushed += 1;
        }
    }

    struct SteppingHueSource;

    impl ColourSource for SteppingHueSource {
        fn sample(&mut self, t: Duration, _inputs: &Inputs) -> Hsv {
            #[allow(clippy::cast_possible_truncation)]
            let hue = (t.as_millis() as f32 / 10.0) % 360.0;
            Hsv::new(hue, 1.0, 1.0)
        }
    }

    #[test]
    fn constant_mode_emits_once_and_only_again_on_change() {
        let mut cfg = BulbConfig::new("b1", "Lamp", "9877d5230f0a", "192.168.0.7");
        cfg.colour.constant.colour = Hsv::new(0.0, 1.0, 1.0);
        cfg.brightness.constant = ConstantBrightnessConfig::new(0.5);

        let mut clock = ManualClock::new();
        let inputs = Inputs::default();
        let mut engine = BulbEngine::from_config(&cfg, RecordingSink::default());

        let first = engine.tick_with_clock(&clock, &inputs).unwrap();
        assert_eq!(
            first,
            Some(WireFrame::Channels {
                channels: Channels::Rgb { r: 255, g: 0, b: 0 },
                dimming: 50,
            })
        );

        // Subsequent unchanged ticks emit nothing.
        clock.advance(Duration::from_millis(200));
        assert_eq!(engine.tick_with_clock(&clock, &inputs).unwrap(), None);
        clock.advance(Duration::from_millis(200));
        assert_eq!(engine.tick_with_clock(&clock, &inputs).unwrap(), None);
        assert_eq!(engine.sink().frames.len(), 1);

        // Updating brightness emits one new frame.
        engine.set_constant_brightness(0.8);
        let updated = engine.tick_with_clock(&clock, &inputs).unwrap();
        assert_eq!(
            updated,
            Some(WireFrame::Channels {
                channels: Channels::Rgb { r: 255, g: 0, b: 0 },
                dimming: 80,
            })
        );
        assert_eq!(engine.sink().frames.len(), 2);
    }

    #[test]
    fn power_toggle_and_zero_brightness_emit_off_and_update_preview() {
        let mut cfg = BulbConfig::new("b1", "Lamp", "9877d5230f0a", "192.168.0.7");
        cfg.colour.constant.colour = Hsv::new(120.0, 1.0, 1.0);
        let clock = ManualClock::new();
        let inputs = Inputs::default();
        let mut engine = BulbEngine::from_config(&cfg, RecordingSink::default());

        engine.tick_with_clock(&clock, &inputs).unwrap();
        assert_eq!(engine.preview().rgb, Rgb::new(0, 255, 0));

        engine.set_enabled(false);
        assert_eq!(
            engine.tick_with_clock(&clock, &inputs).unwrap(),
            Some(WireFrame::Off)
        );
        assert!(!engine.preview().enabled);
        assert_eq!(engine.preview().effective_v, 0.0);
        assert_eq!(engine.preview().rgb, Rgb::new(0, 0, 0));

        // Re-enabling restores colour; brightness 0 also maps to Off.
        engine.set_enabled(true);
        engine.set_constant_brightness(0.0);
        assert_eq!(engine.tick_with_clock(&clock, &inputs).unwrap(), None);
        engine.set_constant_brightness(0.25);
        assert_eq!(
            engine.tick_with_clock(&clock, &inputs).unwrap(),
            Some(WireFrame::Channels {
                channels: Channels::Rgb { r: 0, g: 255, b: 0 },
                dimming: 25,
            })
        );
    }

    #[test]
    fn deterministic_dynamic_source_tracks_manual_clock() {
        let cfg = BulbConfig::new("b1", "Lamp", "9877d5230f0a", "192.168.0.7");
        let mut clock = ManualClock::new();
        let inputs = Inputs::default();
        let mut engine = BulbEngine::from_config(&cfg, RecordingSink::default());
        engine.set_colour_source(Box::new(SteppingHueSource));

        // t = 0 -> hue 0 (red)
        assert_eq!(
            engine.tick_with_clock(&clock, &inputs).unwrap(),
            Some(WireFrame::Channels {
                channels: Channels::Rgb { r: 255, g: 0, b: 0 },
                dimming: 100,
            })
        );
        // t = 1200ms -> hue 120 (green)
        clock.set(Duration::from_millis(1200));
        assert_eq!(
            engine.tick_with_clock(&clock, &inputs).unwrap(),
            Some(WireFrame::Channels {
                channels: Channels::Rgb { r: 0, g: 255, b: 0 },
                dimming: 100,
            })
        );
        // t = 2400ms -> hue 240 (blue)
        clock.set(Duration::from_millis(2400));
        assert_eq!(
            engine.tick_with_clock(&clock, &inputs).unwrap(),
            Some(WireFrame::Channels {
                channels: Channels::Rgb { r: 0, g: 0, b: 255 },
                dimming: 100,
            })
        );
    }

    #[test]
    fn shutdown_flushes_pending_change_and_keeps_last_colour() {
        let mut cfg = BulbConfig::new("b1", "Lamp", "9877d5230f0a", "192.168.0.7");
        cfg.colour.constant.colour = Hsv::new(240.0, 1.0, 1.0);
        let clock = ManualClock::new();
        let inputs = Inputs::default();
        let mut engine = BulbEngine::from_config(&cfg, RecordingSink::default());

        engine.tick_with_clock(&clock, &inputs).unwrap();
        engine.set_constant_colour(Hsv::new(60.0, 1.0, 1.0));
        let final_frame = engine.shutdown(clock.now(), &inputs).unwrap();

        assert_eq!(
            final_frame,
            Some(WireFrame::Channels {
                channels: Channels::Rgb {
                    r: 255,
                    g: 255,
                    b: 0
                },
                dimming: 100,
            })
        );
        assert_eq!(engine.sink().flushed, 1);
        assert_ne!(engine.sink().frames.last(), Some(&WireFrame::Off));
    }

    #[test]
    fn failed_send_does_not_poison_last_sent() {
        let cfg = BulbConfig::new("b1", "Lamp", "9877d5230f0a", "192.168.0.7");
        let clock = ManualClock::new();
        let inputs = Inputs::default();
        let sink = RecordingSink {
            fail_next: true,
            ..Default::default()
        };
        let mut engine = BulbEngine::from_config(&cfg, sink);

        assert!(engine.tick_with_clock(&clock, &inputs).is_err());
        // Retrying on the next tick must still attempt emission.
        assert!(engine.tick_with_clock(&clock, &inputs).unwrap().is_some());
    }

    #[cfg(feature = "net")]
    #[tokio::test]
    async fn bulb_runner_streams_to_udp_socket_and_flushes_on_shutdown() {
        use tokio::net::UdpSocket;

        let mock_bulb = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = mock_bulb.local_addr().unwrap();

        let bulb = wizlight::Bulb::connect_to(addr).await.unwrap();
        let mut cfg = BulbConfig::new("b1", "Test Bulb", "9877d5230f0a", "127.0.0.1");
        cfg.colour.constant.colour = Hsv::new(0.0, 1.0, 1.0);
        cfg.brightness.constant = ConstantBrightnessConfig::new(0.6);

        let runner = BulbRunner::spawn(&bulb, cfg, 50);

        let mut buf = [0u8; 512];
        let (len, _) = tokio::time::timeout(Duration::from_secs(2), mock_bulb.recv_from(&mut buf))
            .await
            .expect("timed out waiting for initial setPilot")
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&buf[..len]).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "method": "setPilot",
                "params": { "r": 255, "g": 0, "b": 0, "dimming": 60 }
            })
        );

        runner.set_constant_colour(Hsv::new(240.0, 1.0, 1.0));
        let (len, _) = tokio::time::timeout(Duration::from_secs(2), mock_bulb.recv_from(&mut buf))
            .await
            .expect("timed out waiting for updated setPilot")
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&buf[..len]).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "method": "setPilot",
                "params": { "r": 0, "g": 0, "b": 255, "dimming": 60 }
            })
        );

        let counters = runner.shutdown().await;
        assert!(counters.sent >= 2);
        assert_eq!(counters.dropped, 0);
    }
}
