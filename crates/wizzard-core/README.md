# `wizzard-core`

The standalone Rust engine behind [WiZzard](https://github.com/LucasAmion/wizzard),
driving Philips WiZ smart bulbs over the local network with independent colour
and brightness pipelines.

`wizzard-core` has no Tauri or UI dependency. Pure computation features
(`colour`, `engine`) compile to `wasm32-unknown-unknown` with
`--no-default-features`, while network I/O lives behind the `net` feature.

## Feature flags

| Feature | Default | WASM-safe | Purpose |
| --- | --- | --- | --- |
| `colour` | yes | yes | HSV colour model, composition, and wire mapping |
| `engine` | yes | yes | Source traits and render loop (pluggable clock and sink) |
| `net` | yes | no | Philips WiZ LAN transport and discovery via [`wizlight`](https://crates.io/crates/wizlight) |

## License

MIT
