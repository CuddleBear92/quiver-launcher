# Quiver Launcher

A desktop and Android launcher for games and apps that people release on GitHub, GitLab and other forges. It uses
the catalog at quiverlauncher.com. Built with Tauri 2: a Rust backend and a React 19 + TypeScript + Vite frontend.
Upstream is `tgeorgiadis/quiver-launcher`, licensed under BSL 1.1 with a CLA. See CONTRIBUTING.md.

## Layout (pnpm workspace)

- `apps/launcher/` (`@quiver/launcher`): the app.
  - `src/`: React frontend. Flat folder with one module per feature, e.g. `downloads.ts`, `sync.ts`, `telemetry.ts`
    and `catalog.ts`. Tests sit next to the code as `*.test.ts`.
  - `src-tauri/src/`: the Rust side. Includes install, launch, update, steam, saves, sandbox, flatpak, gamepad and
    android. `lib.rs` wires the commands. `tauri.cef.conf.json` and `Cargo.cef.lock` are an experimental CEF build;
    see `CEF-SPIKE.md`.
  - `flatpak/` and `src-tauri/appimage/`: Linux packaging.
- `packages/api` (`@quiverlauncher/api`): typed client for the catalog API, plus release-host providers.
- `packages/ui` (`@quiverlauncher/ui`): catalog components and CSS tokens shared with the website.
- `packages/i18n` (`@quiverlauncher/i18n`): Lingui catalogs shared with the website.
- `e2e/`: Playwright and WebDriver suites: `phone` for Android sizes, `web` for controller and keyboard navigation in
  WebKit, and `live`.
- `docs/`: user docs covering providers, cloud saves, the sync folder and usage data.

`packages/ui`, `packages/api` and `packages/i18n` are also used by the quiverlauncher.com website repo. Keep their
public exports backwards-compatible.

## Commands

Run all commands from the repo root. Node 22 and pnpm 10.16.0 are required (`corepack enable`).

```bash
pnpm install --frozen-lockfile
pnpm typecheck                                   # tsc in every package
pnpm test                                        # vitest in every package
pnpm --filter @quiverlauncher/i18n extract       # after adding/changing any UI string
pnpm --filter @quiverlauncher/i18n check         # CI fails if catalogs are stale
cargo test --manifest-path apps/launcher/src-tauri/Cargo.toml
pnpm dev                                         # tauri dev (needs a desktop + Linux webkit libs)
```

On Linux, the Rust build needs `libdbus-1-dev libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev
libudev-dev`.

## Conventions

- Every user-facing string goes through Lingui. Run `extract` afterwards and commit the updated `locales/`. The app
  ships in 10 languages.
- UI must work with a controller and keyboard (TV mode / Steam Deck) and at phone widths (Android). The e2e `web` and
  `phone` suites cover this.
- Add or extend the colocated vitest file when changing logic in `src/*.ts`.
- Commit messages follow upstream's style, `Area: what changed for the user (#issue)`. For example: `Tags: the
  catalog's tags can be hidden ... (#431)`.
- CI (`.github/workflows/ci.yml`) runs typecheck, test, the i18n check, cargo test, and the phone, web, windows,
  installer and appimage jobs.

## Fork workflow

- Work on a branch in the user's fork. Upstream is `tgeorgiadis/quiver-launcher`, which may be added as the `upstream`
  remote.
- Before opening a PR to upstream, rebase onto `upstream/main`. The first upstream PR requires signing the CLA.
