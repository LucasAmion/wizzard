# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `wizzard-core` foundational modules: HSV/RGB colour composition and wire
  mapping (`colour`), versioned `schema_version = 1` configuration types with a
  5 Hz default tick rate (`config`), `ColourSource` / `BrightnessSource` traits
  and constant sources (`source`), and the deterministic `BulbEngine` with
  pluggable clock/sink plus the Tokio/WiZ `BulbRunner` behind `net` (`engine`).
- Initial repository scaffold: Cargo workspace (`wizzard-core` + `src-tauri`),
  Tauri v2 desktop shell, and React 19 + Vite + TypeScript + Tailwind CSS v4 +
  shadcn/ui frontend setup.
