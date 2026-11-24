#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

#[cfg(feature = "colour")]
pub mod colour;

#[cfg(feature = "colour")]
pub mod config;

#[cfg(feature = "engine")]
pub mod source;

#[cfg(feature = "engine")]
pub mod engine;

/// Crate version string matching `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }
}
