# NEXORA

**Material & Texture Storage for 3D Artists** — _Store. Organize. Preview. Use._

NEXORA is a local-first desktop app for storing, organizing, searching, previewing,
and applying textures and materials for 3D production, with a real Maya bridge
(later phase). This repository is the **Phase 1 foundation**: a runnable Tauri 2 +
React + Rust + SQLite application shell.

---

## What's in this scaffold (Phase 1)

| Area | Status |
| --- | --- |
| Tauri 2 desktop shell (Rust) | ✅ window, app-data DB, IPC commands |
| React + TypeScript + Tailwind UI | ✅ dark theme, full sidebar nav, Home, Library shell, Settings |
| SQLite database + migrations | ✅ full v1 schema (17 tables + FTS5), versioned migrations |
| Settings (library, appearance, import, renderer, updates) | ✅ typed, persisted, editable in UI |
| Library configuration (managed / referenced) | ✅ folder picker + skeleton creation |
| Asset ID system (`NX-TEX-81D4-9B22`) | ✅ immutable IDs, never filename-based |
| Map-type recognition registry | ✅ configurable registry + matcher (wired to import in Phase 2) |
| Import / scanning / previews / Maya bridge | ⏳ later phases (see below) |

Everything above the dashed line in the roadmap is intentionally **not** built yet —
Phase 1 is the skeleton the rest hangs on.

## Architecture

```
NEXORA/
├── src/                # React + TS frontend (UI only; talks to Rust over IPC)
│   ├── lib/            # api bridge (+ browser mock), types, nav model
│   ├── components/     # Sidebar, TopBar, StatCard, Icon, EmptyState
│   └── pages/          # Home, Library, Settings
├── src-tauri/          # Tauri desktop shell (thin) — commands, state, config
├── core/               # nexora-core: the engine (DB, settings, IDs, map registry)
│                       #   → pure Rust, fully unit-tested, no GUI/Maya deps
└── scripts/            # icon generation
```

The important boundary: **all logic lives in `core`** so it can be tested headless.
`src-tauri` only locks state, calls core, and maps errors for IPC. Future crates
(renderers, api server, updater, Maya bridge) join the Cargo workspace beside `core`.

The frontend `api` layer includes an in-memory **browser mock**, so `npm run dev`
renders the full UI in a plain browser without the Rust toolchain — handy for UI work.

## Prerequisites

- **Node.js** 18+ and npm
- **Rust** (stable) — https://rustup.rs
- **Tauri 2 system deps** — on Windows: the *WebView2 runtime* (preinstalled on
  Win 11) and *Microsoft C++ Build Tools*. See
  https://v2.tauri.app/start/prerequisites/

## Run it

```bash
npm install          # install frontend deps
npm run app:dev      # launch the desktop app (Tauri) with hot reload
```

UI-only preview in a browser (no Rust needed):

```bash
npm run dev          # http://localhost:1420  (uses the mock backend)
```

Build a distributable:

```bash
npm run app:build    # produces an installer in src-tauri/target/release/bundle
```

## Test

```bash
cargo test -p nexora-core   # 12 engine tests: schema, migrations, FTS, IDs, map registry, settings
npm run build               # typecheck (strict) + production frontend build
```

## Roadmap (from the product spec)

Phase 1 **Foundation** ✅ · Phase 2 Texture Storage + Import · Phase 3 Texture Sets ·
Phase 4 Material Storage · Phase 5 Library (search/filters/tags/collections/health) ·
Phase 6 Preview Engine · Phase 7 Maya Bridge · Phase 8 V-Ray · Phase 9 Arnold ·
Phase 10 Maya Capture · Phase 11 Auto-update (GitHub Releases).

## License

MIT © FRIXXY
