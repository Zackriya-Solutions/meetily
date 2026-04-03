# Contributing

## Branching

- Branch from `main`.
- Keep each branch focused on one change set.
- Open pull requests back to `main` unless the repository maintainers specify a different target.

## Development Areas

- Desktop app UI: [`desktop/src/`](desktop/src/)
- Tauri and Rust core: [`desktop/src-tauri/`](desktop/src-tauri/)
- Workspace manifest: [`Cargo.toml`](Cargo.toml)

## Expected Workflow

1. Install dependencies for the desktop app.
2. Make the code change.
3. Run the narrowest relevant verification you can from this machine.
4. Update documentation whenever behavior, architecture, or build steps change.

## Useful Commands

From [`desktop/`](desktop/):

```bash
pnpm install
pnpm run tauri:dev
pnpm run tauri:build
pnpm run tauri:dev:cpu
pnpm run tauri:dev:cuda
pnpm run tauri:dev:vulkan
pnpm run tauri:dev:metal
```

From the repository root:

```bash
cargo metadata --no-deps --format-version 1
cargo check -p meetily
```

## Documentation Standard

- Only document behavior that is present in the current codebase or explicitly planned in the active change set.
- Remove or correct stale claims in the same change that exposes them.
- Prefer one source of truth over status-summary duplicates.
