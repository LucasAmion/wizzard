# WiZzard

Cross-platform desktop app (Rust + Tauri v2 + React) and reusable core engine
for controlling Philips WiZ smart bulbs over the local network in real time.

## Workspace layout

- `crates/wizzard-core` — standalone lighting engine, colour pipeline, and
  configuration model with no Tauri or UI dependency. Pure computation features
  compile to `wasm32-unknown-unknown`.
- `src-tauri` — thin Tauri v2 desktop adapter over `wizzard-core`.
- `src` — React 19 + TypeScript + Tailwind CSS v4 frontend.

## Development

Prerequisites: Rust (1.90+ for the desktop app; 1.85+ for `wizzard-core`),
Node.js 22+, and the [Tauri v2 system prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev
```

### Verification

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p wizzard-core --all-features
cargo check -p wizzard-core --target wasm32-unknown-unknown --no-default-features --features colour
npx tsc --noEmit
npm run lint
npm run build
```

## License

MIT
