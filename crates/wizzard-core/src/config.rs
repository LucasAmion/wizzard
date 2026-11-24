//! Versioned application configuration schema.
//!
//! All configuration types are pure data owned by `wizzard-core` so they stay
//! usable in headless tools and `wasm32` builds. Every mode retains its own
//! settings when inactive so switching modes is lossless.

use serde::{Deserialize, Serialize};

use crate::colour::{Hsv, clamp_unit};

/// Current on-disk configuration schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Default engine tick rate in Hz.
///
/// Matches the measured 5 Hz (200 ms) zero-skip optical ceiling of WiZ bulbs'
/// internal ~300 ms cross-fade ramp.
pub const DEFAULT_TICK_RATE_HZ: u16 = 5;

/// Root configuration document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Explicit schema version for forward-compatible migrations.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Saved bulbs, in sidebar display order.
    #[serde(default)]
    pub bulbs: Vec<BulbConfig>,
    /// User-saved hex favourite colours (`#rrggbb`).
    #[serde(default)]
    pub favourite_colours: Vec<String>,
    /// Global application settings.
    #[serde(default)]
    pub settings: Settings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            bulbs: Vec::new(),
            favourite_colours: Vec::new(),
            settings: Settings::default(),
        }
    }
}

/// Persisted configuration for one saved bulb.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BulbConfig {
    /// Stable application-assigned identifier.
    pub id: String,
    /// User-facing bulb name.
    pub name: String,
    /// Hardware MAC address (12 lowercase hex digits, no separators).
    pub mac: String,
    /// Last-known LAN IP address (re-resolved by MAC on startup).
    pub ip: String,
    /// WiZ `moduleName`, if reported by `getSystemConfig`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Bulb firmware version string, if reported by `getSystemConfig`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    /// Whether the bulb is currently powered on.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Colour pipeline configuration.
    #[serde(default)]
    pub colour: ColourConfig,
    /// Brightness pipeline configuration.
    #[serde(default)]
    pub brightness: BrightnessConfig,
}

impl BulbConfig {
    /// Creates a new saved bulb config with default constant colour and full brightness.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        mac: impl Into<String>,
        ip: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            mac: mac.into(),
            ip: ip.into(),
            model: None,
            firmware: None,
            enabled: true,
            colour: ColourConfig::default(),
            brightness: BrightnessConfig::default(),
        }
    }
}

/// Active colour mode for a bulb.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourModeKind {
    /// Fixed colour value.
    #[default]
    Constant,
}

/// Colour configuration for a bulb, preserving each mode's settings when inactive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ColourConfig {
    /// Currently active colour mode.
    #[serde(default)]
    pub mode: ColourModeKind,
    /// Settings for [`ColourModeKind::Constant`].
    #[serde(default)]
    pub constant: ConstantColourConfig,
}

/// Settings for constant colour mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConstantColourConfig {
    /// Target HSV colour.
    #[serde(default)]
    pub colour: Hsv,
}

/// Active brightness mode for a bulb.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrightnessModeKind {
    /// Fixed brightness scalar.
    #[default]
    Constant,
}

/// Brightness configuration for a bulb, preserving each mode's settings when inactive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrightnessConfig {
    /// Currently active brightness mode.
    #[serde(default)]
    pub mode: BrightnessModeKind,
    /// Settings for [`BrightnessModeKind::Constant`].
    #[serde(default)]
    pub constant: ConstantBrightnessConfig,
}

/// Settings for constant brightness mode.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConstantBrightnessConfig {
    /// Scalar brightness in `0.0..=1.0`.
    #[serde(default = "default_one")]
    pub value: f32,
}

impl Default for ConstantBrightnessConfig {
    fn default() -> Self {
        Self { value: 1.0 }
    }
}

impl ConstantBrightnessConfig {
    /// Creates a constant brightness config clamped into `0.0..=1.0`.
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self {
            value: clamp_unit(value),
        }
    }
}

/// UI theme preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Dark theme with purple accents.
    #[default]
    Dark,
}

/// What the engine does to bulbs when the application quits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnQuitBehaviour {
    /// Stop cleanly on a frame boundary so bulbs retain their last sent colour.
    #[default]
    KeepLastColour,
}

/// Global application settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// UI theme.
    #[serde(default)]
    pub theme: Theme,
    /// Per-bulb render tick rate in Hz (default [`DEFAULT_TICK_RATE_HZ`]).
    #[serde(default = "default_tick_rate_hz")]
    pub tick_rate_hz: u16,
    /// Behaviour when the application exits.
    #[serde(default)]
    pub on_quit: OnQuitBehaviour,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            tick_rate_hz: DEFAULT_TICK_RATE_HZ,
            on_quit: OnQuitBehaviour::KeepLastColour,
        }
    }
}

const fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

const fn default_true() -> bool {
    true
}

const fn default_one() -> f32 {
    1.0
}

const fn default_tick_rate_hz() -> u16 {
    DEFAULT_TICK_RATE_HZ
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_json_round_trip() {
        let mut config = AppConfig::default();
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.settings.tick_rate_hz, 5);
        assert_eq!(config.settings.on_quit, OnQuitBehaviour::KeepLastColour);

        let mut bulb = BulbConfig::new("bulb-1", "Living Room", "9877d5230f0a", "192.168.0.7");
        bulb.model = Some("ESP25_SHRGB_01".into());
        bulb.firmware = Some("1.38.0".into());
        bulb.colour.constant.colour = Hsv::new(280.0, 0.9, 1.0);
        bulb.brightness.constant = ConstantBrightnessConfig::new(0.75);
        config.bulbs.push(bulb);
        config.favourite_colours.push("#a855f7".into());

        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn minimal_json_fills_defaults() {
        let parsed: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, AppConfig::default());
    }
}
