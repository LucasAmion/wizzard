//! Colour types, HSV composition, and WiZ wire-frame mapping.
//!
//! Brightness and colour are kept orthogonal all the way to the wire:
//!
//! 1. Effective value is `final_V = clamp(colour.v * brightness, 0.0, 1.0)`.
//! 2. Hue and saturation are converted at full value (`V = 1.0`) so colour
//!    channels retain 8-bit resolution at low brightness.
//! 3. `final_V` travels in WiZ's hardware `dimming` field (`1..=100`), while
//!    `final_V == 0.0` maps to [`WireFrame::Off`] (`state: false`) because
//!    `dimming: 0` is clamped to `1` by the bulb rather than switching it off.

use palette::{FromColor, Hsv as PaletteHsv, Srgb};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced when parsing colour values.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ColourError {
    /// Input was not a valid 6-digit hexadecimal sRGB colour (`#rrggbb` or `rrggbb`).
    #[error("invalid hex colour {0:?}: expected #rrggbb or rrggbb")]
    InvalidHex(String),
}

/// An 8-bit sRGB colour triple.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb {
    /// Red channel (`0..=255`).
    pub r: u8,
    /// Green channel (`0..=255`).
    pub g: u8,
    /// Blue channel (`0..=255`).
    pub b: u8,
}

impl Rgb {
    /// Creates an sRGB triple.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Formats the colour as lowercase `#rrggbb`.
    ///
    /// ```
    /// use wizzard_core::colour::Rgb;
    ///
    /// assert_eq!(Rgb::new(168, 85, 247).to_hex(), "#a855f7");
    /// ```
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Parses a 6-digit hex colour (`#rrggbb` or `rrggbb`, case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns [`ColourError::InvalidHex`] if the string is not six valid hex digits
    /// with an optional leading `#`.
    ///
    /// ```
    /// use wizzard_core::colour::Rgb;
    ///
    /// assert_eq!(Rgb::from_hex("#a855f7")?, Rgb::new(168, 85, 247));
    /// assert_eq!(Rgb::from_hex("22D3EE")?, Rgb::new(34, 211, 238));
    /// # Ok::<(), wizzard_core::colour::ColourError>(())
    /// ```
    pub fn from_hex(input: &str) -> Result<Self, ColourError> {
        let digits = input.trim().strip_prefix('#').unwrap_or(input.trim());
        if digits.len() != 6 || !digits.is_ascii() {
            return Err(ColourError::InvalidHex(input.to_owned()));
        }
        let r = u8::from_str_radix(&digits[0..2], 16)
            .map_err(|_| ColourError::InvalidHex(input.to_owned()))?;
        let g = u8::from_str_radix(&digits[2..4], 16)
            .map_err(|_| ColourError::InvalidHex(input.to_owned()))?;
        let b = u8::from_str_radix(&digits[4..6], 16)
            .map_err(|_| ColourError::InvalidHex(input.to_owned()))?;
        Ok(Self { r, g, b })
    }
}

/// An HSV colour with hue in degrees (`0.0..360.0`) and saturation/value in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hsv {
    /// Hue in degrees, normalised into `[0.0, 360.0)`.
    pub h: f32,
    /// Saturation in `0.0..=1.0`.
    pub s: f32,
    /// Value (brightness factor) in `0.0..=1.0`.
    pub v: f32,
}

impl Default for Hsv {
    fn default() -> Self {
        Self::new(280.0, 0.75, 1.0)
    }
}

impl Hsv {
    /// Creates an HSV colour, wrapping `h` into `[0.0, 360.0)` and clamping `s`/`v`
    /// into `0.0..=1.0`. Non-finite inputs default to `0.0`.
    ///
    /// ```
    /// use wizzard_core::colour::Hsv;
    ///
    /// let c = Hsv::new(400.0, 1.5, -0.2);
    /// assert_eq!(c, Hsv { h: 40.0, s: 1.0, v: 0.0 });
    /// ```
    #[must_use]
    pub fn new(h: f32, s: f32, v: f32) -> Self {
        Self {
            h: normalise_hue(h),
            s: clamp_unit(s),
            v: clamp_unit(v),
        }
    }

    /// Returns a copy of this colour with `v = 1.0`.
    #[must_use]
    pub fn with_full_value(self) -> Self {
        Self::new(self.h, self.s, 1.0)
    }

    /// Converts this HSV colour into 8-bit sRGB.
    ///
    /// ```
    /// use wizzard_core::colour::{Hsv, Rgb};
    ///
    /// assert_eq!(Hsv::new(0.0, 1.0, 1.0).to_rgb(), Rgb::new(255, 0, 0));
    /// assert_eq!(Hsv::new(120.0, 1.0, 1.0).to_rgb(), Rgb::new(0, 255, 0));
    /// assert_eq!(Hsv::new(240.0, 1.0, 1.0).to_rgb(), Rgb::new(0, 0, 255));
    /// ```
    #[must_use]
    pub fn to_rgb(self) -> Rgb {
        let hsv = PaletteHsv::new(
            normalise_hue(self.h),
            clamp_unit(self.s),
            clamp_unit(self.v),
        );
        let srgb: Srgb<f32> = Srgb::from_color(hsv);
        let (r, g, b) = srgb.into_format::<u8>().into_components();
        Rgb::new(r, g, b)
    }

    /// Converts an 8-bit sRGB colour into HSV.
    #[must_use]
    pub fn from_rgb(rgb: Rgb) -> Self {
        let srgb = Srgb::new(rgb.r, rgb.g, rgb.b).into_format::<f32>();
        let hsv = PaletteHsv::from_color(srgb);
        Self::new(hsv.hue.into_positive_degrees(), hsv.saturation, hsv.value)
    }

    /// Formats the sRGB equivalent of this HSV colour as lowercase `#rrggbb`.
    #[must_use]
    pub fn to_hex(self) -> String {
        self.to_rgb().to_hex()
    }

    /// Parses a 6-digit hex colour and converts it into HSV.
    ///
    /// # Errors
    ///
    /// Returns [`ColourError::InvalidHex`] if `input` is not valid hex.
    pub fn from_hex(input: &str) -> Result<Self, ColourError> {
        Ok(Self::from_rgb(Rgb::from_hex(input)?))
    }
}

/// Emitter channel values sent to a WiZ bulb.
///
/// The five emitters (`r`, `g`, `b`, `c`, `w`) form one replacing instruction on
/// WiZ hardware: omitting the white channels turns them off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Channels {
    /// Raw RGB channels only (`c` and `w` left dark).
    Rgb {
        /// Red emitter (`0..=255`).
        r: u8,
        /// Green emitter (`0..=255`).
        g: u8,
        /// Blue emitter (`0..=255`).
        b: u8,
    },
    /// Five-channel RGB + cold/warm white mix.
    Rgbww {
        /// Red emitter (`0..=255`).
        r: u8,
        /// Green emitter (`0..=255`).
        g: u8,
        /// Blue emitter (`0..=255`).
        b: u8,
        /// Cold white emitter (`0..=255`).
        c: u8,
        /// Warm white emitter (`0..=255`).
        w: u8,
    },
}

impl Channels {
    /// Constructs a raw RGB channel group from an [`Rgb`] triple.
    #[must_use]
    pub const fn from_rgb(rgb: Rgb) -> Self {
        Self::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }
    }

    /// Returns `true` if at least one emitter channel is non-zero.
    #[must_use]
    pub const fn is_lit(self) -> bool {
        match self {
            Self::Rgb { r, g, b } => (r | g | b) != 0,
            Self::Rgbww { r, g, b, c, w } => (r | g | b | c | w) != 0,
        }
    }
}

/// Strategy that turns a full-value `(hue, saturation)` colour into bulb emitter
/// channel values.
pub trait ColourMixer: Send + Sync {
    /// Computes emitter channels for hue `h` (`[0, 360)`) and saturation `s`
    /// (`0.0..=1.0`) at full value (`V = 1.0`).
    fn mix(&self, h: f32, s: f32) -> Channels;
}

/// Baseline mixer that converts `(H, S, V=1)` directly into raw `r`/`g`/`b`
/// without lighting the cold or warm white emitters.
#[derive(Clone, Copy, Debug, Default)]
pub struct RawRgbMixer;

impl ColourMixer for RawRgbMixer {
    fn mix(&self, h: f32, s: f32) -> Channels {
        Channels::from_rgb(Hsv::new(h, s, 1.0).to_rgb())
    }
}

/// A composed output frame ready to transmit to a bulb.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireFrame {
    /// Switch the bulb off (`state: false`).
    Off,
    /// Drive emitter channels alongside hardware `dimming` (`1..=100`).
    Channels {
        /// Emitter channel values (at least one channel non-zero).
        channels: Channels,
        /// Hardware PWM dimming percentage (`1..=100`).
        dimming: u8,
    },
}

#[cfg(feature = "net")]
impl WireFrame {
    /// Converts this frame into a [`wizlight::PilotBuilder`] ready for streaming
    /// or `setPilot`.
    #[must_use]
    pub fn to_pilot_builder(self) -> wizlight::PilotBuilder {
        use wizlight::{Channel, Dimming, PilotBuilder};

        match self {
            Self::Off => PilotBuilder::new().state(false),
            Self::Channels { channels, dimming } => {
                let dimming = Dimming::new(dimming.clamp(1, 100))
                    .expect("clamped dimming is always within 1..=100");
                let builder = match channels {
                    Channels::Rgb { r, g, b } => {
                        PilotBuilder::new().rgb(Channel::new(r), Channel::new(g), Channel::new(b))
                    }
                    Channels::Rgbww { r, g, b, c, w } => PilotBuilder::new().rgbww(
                        Channel::new(r),
                        Channel::new(g),
                        Channel::new(b),
                        Channel::new(c),
                        Channel::new(w),
                    ),
                };
                builder.dimming(dimming)
            }
        }
    }
}

/// Multiplies a colour's HSV value by a brightness factor (`0.0..=1.0`).
///
/// ```
/// use wizzard_core::colour::{Hsv, compose_value};
///
/// let v = compose_value(Hsv::new(200.0, 0.8, 0.8), 0.5);
/// assert!((v - 0.4).abs() < 1e-6);
/// ```
#[must_use]
pub fn compose_value(colour: Hsv, brightness: f32) -> f32 {
    clamp_unit(clamp_unit(colour.v) * clamp_unit(brightness))
}

/// Maps an effective value (`0.0..=1.0`) to a WiZ hardware `dimming` percentage
/// (`1..=100`), returning `None` when `final_v == 0.0` so the caller switches
/// the bulb off instead of sending out-of-range `dimming: 0`.
#[must_use]
pub fn value_to_dimming(final_v: f32) -> Option<u8> {
    let clamped = clamp_unit(final_v);
    if clamped == 0.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pct = (clamped * 100.0).round().clamp(1.0, 100.0) as u8;
    Some(pct)
}

/// Composes an HSV colour and brightness factor into a [`WireFrame`] using the
/// given [`ColourMixer`].
///
/// ```
/// use wizzard_core::colour::{Channels, Hsv, RawRgbMixer, WireFrame, compose_frame};
///
/// let frame = compose_frame(Hsv::new(0.0, 1.0, 0.8), 0.5, &RawRgbMixer);
/// assert_eq!(
///     frame,
///     WireFrame::Channels {
///         channels: Channels::Rgb { r: 255, g: 0, b: 0 },
///         dimming: 40,
///     },
/// );
/// assert_eq!(compose_frame(Hsv::new(0.0, 1.0, 1.0), 0.0, &RawRgbMixer), WireFrame::Off);
/// ```
#[must_use]
pub fn compose_frame(colour: Hsv, brightness: f32, mixer: &dyn ColourMixer) -> WireFrame {
    let Some(dimming) = value_to_dimming(compose_value(colour, brightness)) else {
        return WireFrame::Off;
    };
    let channels = mixer.mix(normalise_hue(colour.h), clamp_unit(colour.s));
    if !channels.is_lit() {
        return WireFrame::Off;
    }
    WireFrame::Channels { channels, dimming }
}

fn normalise_hue(h: f32) -> f32 {
    if !h.is_finite() {
        return 0.0;
    }
    let wrapped = h.rem_euclid(360.0);
    if wrapped >= 360.0 { 0.0 } else { wrapped }
}

pub(crate) fn clamp_unit(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_invalid_inputs() {
        let rgb = Rgb::from_hex("#A855F7").unwrap();
        assert_eq!(rgb, Rgb::new(168, 85, 247));
        assert_eq!(rgb.to_hex(), "#a855f7");

        let hsv = Hsv::from_hex("22d3ee").unwrap();
        assert_eq!(hsv.to_hex(), "#22d3ee");

        assert!(Rgb::from_hex("#fff").is_err());
        assert!(Rgb::from_hex("#gggggg").is_err());
        assert!(Rgb::from_hex("1234567").is_err());
        assert!(Rgb::from_hex("ééé").is_err());
    }

    #[test]
    fn hsv_normalises_hue_and_clamps_saturation_value() {
        assert_eq!(
            Hsv::new(-30.0, 2.0, -1.0),
            Hsv {
                h: 330.0,
                s: 1.0,
                v: 0.0
            }
        );
        assert_eq!(
            Hsv::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY),
            Hsv {
                h: 0.0,
                s: 0.0,
                v: 0.0
            }
        );
    }

    #[test]
    fn composition_preserves_full_rgb_resolution_and_uses_dimming() {
        let colour = Hsv::new(280.0, 0.9, 0.8);
        let frame = compose_frame(colour, 0.5, &RawRgbMixer);
        let expected_rgb = Hsv::new(280.0, 0.9, 1.0).to_rgb();
        assert_eq!(
            frame,
            WireFrame::Channels {
                channels: Channels::from_rgb(expected_rgb),
                dimming: 40,
            }
        );
    }

    #[test]
    fn zero_value_or_brightness_maps_to_off_never_dimming_zero() {
        assert_eq!(
            compose_frame(Hsv::new(120.0, 1.0, 1.0), 0.0, &RawRgbMixer),
            WireFrame::Off
        );
        assert_eq!(
            compose_frame(Hsv::new(120.0, 1.0, 0.0), 1.0, &RawRgbMixer),
            WireFrame::Off
        );
        // Any tiny positive value clamps to dimming 1 rather than turning off.
        assert_eq!(value_to_dimming(0.001), Some(1));
        assert_eq!(value_to_dimming(1.0), Some(100));
    }

    #[cfg(feature = "net")]
    #[test]
    fn wire_frame_builds_valid_wizlight_pilot_requests() {
        let on = compose_frame(Hsv::new(0.0, 1.0, 0.8), 0.5, &RawRgbMixer);
        let req = on.to_pilot_builder().set_pilot().unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "method": "setPilot",
                "params": { "r": 255, "g": 0, "b": 0, "dimming": 40 }
            })
        );

        let off = WireFrame::Off.to_pilot_builder().set_pilot().unwrap();
        let json = serde_json::to_value(&off).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "method": "setPilot",
                "params": { "state": false }
            })
        );

        let rgbww = WireFrame::Channels {
            channels: Channels::Rgbww {
                r: 255,
                g: 180,
                b: 120,
                c: 0,
                w: 40,
            },
            dimming: 75,
        };
        let req = rgbww.to_pilot_builder().set_pilot().unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "method": "setPilot",
                "params": { "r": 255, "g": 180, "b": 120, "c": 0, "w": 40, "dimming": 75 }
            })
        );
    }
}
