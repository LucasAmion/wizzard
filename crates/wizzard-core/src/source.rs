//! Pluggable colour and brightness sources.
//!
//! Colour and brightness pipelines are sampled independently on every tick and
//! composed by the engine. Sources receive a monotonic elapsed [`Duration`]
//! rather than a wall-clock `Instant` so the same traits compile cleanly for
//! `wasm32-unknown-unknown` and run deterministically in unit tests.

use std::time::Duration;

use crate::colour::{Hsv, clamp_unit};

/// Shared per-tick input snapshot passed to every source.
///
/// Empty for constant modes; audio and screen capture buses populate fields
/// here in later milestones without changing source trait signatures.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Inputs {}

/// Produces an HSV colour sample for a given monotonic timestamp.
pub trait ColourSource: Send {
    /// Samples the target HSV colour at elapsed time `t`.
    fn sample(&mut self, t: Duration, inputs: &Inputs) -> Hsv;
}

/// Produces a normalised brightness scalar (`0.0..=1.0`) for a given monotonic
/// timestamp.
pub trait BrightnessSource: Send {
    /// Samples the target brightness scalar (`0.0..=1.0`) at elapsed time `t`.
    fn sample(&mut self, t: Duration, inputs: &Inputs) -> f32;
}

/// Colour source that always yields a fixed [`Hsv`] colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConstantColourSource {
    colour: Hsv,
}

impl ConstantColourSource {
    /// Creates a constant colour source with the given HSV value.
    #[must_use]
    pub fn new(colour: Hsv) -> Self {
        Self {
            colour: Hsv::new(colour.h, colour.s, colour.v),
        }
    }

    /// Returns the configured colour.
    #[must_use]
    pub const fn colour(&self) -> Hsv {
        self.colour
    }

    /// Updates the configured colour.
    pub fn set_colour(&mut self, colour: Hsv) {
        self.colour = Hsv::new(colour.h, colour.s, colour.v);
    }
}

impl ColourSource for ConstantColourSource {
    fn sample(&mut self, _t: Duration, _inputs: &Inputs) -> Hsv {
        self.colour
    }
}

/// Brightness source that always yields a fixed scalar in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstantBrightnessSource {
    value: f32,
}

impl Default for ConstantBrightnessSource {
    fn default() -> Self {
        Self { value: 1.0 }
    }
}

impl ConstantBrightnessSource {
    /// Creates a constant brightness source clamped into `0.0..=1.0`.
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self {
            value: clamp_unit(value),
        }
    }

    /// Returns the configured brightness scalar (`0.0..=1.0`).
    #[must_use]
    pub const fn value(&self) -> f32 {
        self.value
    }

    /// Updates the configured brightness scalar, clamping into `0.0..=1.0`.
    pub fn set_value(&mut self, value: f32) {
        self.value = clamp_unit(value);
    }
}

impl BrightnessSource for ConstantBrightnessSource {
    fn sample(&mut self, _t: Duration, _inputs: &Inputs) -> f32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_sources_return_normalised_values() {
        let mut colour = ConstantColourSource::new(Hsv::new(390.0, 0.8, 0.9));
        let mut bri = ConstantBrightnessSource::new(1.5);
        let inputs = Inputs::default();

        assert_eq!(
            colour.sample(Duration::ZERO, &inputs),
            Hsv::new(30.0, 0.8, 0.9)
        );
        assert_eq!(bri.sample(Duration::from_secs(5), &inputs), 1.0);

        colour.set_colour(Hsv::new(120.0, 0.5, 0.4));
        bri.set_value(-0.2);
        assert_eq!(colour.colour(), Hsv::new(120.0, 0.5, 0.4));
        assert_eq!(bri.value(), 0.0);
    }
}
