# NEXORA

**Material & Texture Storage for 3D Artists** — _Store. Organize. Preview. Use._

NEXORA is a local-first desktop app for storing, organizing, searching, previewing,
and applying textures and materials for 3D production — with a real Maya bridge so
you can push materials straight into a scene and capture existing shaders back into
your library. Your files stay as normal files on disk; NEXORA indexes them.

<p align="center"><em>Tauri 2 · React · Rust · SQLite · three.js</em></p>

---

## Download & install

Grab the latest Windows installer from the
[**Releases**](https://github.com/Frixxy93/NEXORA/releases/latest) page and run the
`.exe`. It installs NEXORA **and** drops the Maya plug-in into your Maya 2026 & 2027
`plug-ins` folders — then just enable `nexora_bridge.py` in Maya's Plug-in Manager.

The app **auto-updates**: new releases are delivered and installed from within the
app (Settings ▸ Updates ▸ Check for updates), verified against a signing key.

> **Windows SmartScreen note.** NEXORA is a small independent app and its installer
> isn't code-signed with a paid certificate, so the first time you run it Windows may
> show a blue **"Windows protected your PC"** screen. This is expected — click
> **More info**, then **Run anyway**. The warning eases on its own as more people
> download and run it (SmartScreen builds trust over time). The update package is
> still cryptographically signed and verified before installing.

## Features

- **One library for textures *and* materials.** Individual maps and complete
  materials are both first-class assets; a texture can live on its own or belong to
  a material. Stable IDs (`NX-TEX-81D4-9B22`), never filename-based.
- **Smart import.** Drag in files or folders — NEXORA detects map types
  (base color, roughness, normal, height…), groups **texture sets** by base name,
  and collapses **UDIM** tiles into one asset with missing-tile detection.
- **Live previews.** Real-time WebGL/PBR material preview (three.js) plus per-map
  thumbnails.
- **Find anything.** Full-text search, favorites, tags, collections, duplicate
  detection, recent-added / recently-used, and a library health view.
- **Maya bridge.** Send any material or texture straight into Maya, browse your
  library from Maya's NEXORA menu, and **capture** an existing Maya shader's textures
  back into NEXORA as a new material.
- **First-class renderer adapters.** Materials build correct shader networks for
  **Arnold** (`aiStandardSurface`, `aiNormalMap`/`aiBump2d`), **V-Ray** (`VRayMtl`),
  and generic PBR (`standardSurface`) — honoring your renderer preference, with UDIM
  wiring. Renderer logic is kept in dedicated adapters, not scattered.
- **Signed auto-update** from GitHub Releases.

## The Maya plug-in

Two variants live in [`plugins/maya/`](plugins/maya):

- **`nexora_bridge.py`** — a Python (API 2.0) scripted plug-in that loads on Maya
  2022+ (verified on 2026 & 2027), no compilation needed. This is what the installer
  ships and what the in-app **Settings ▸ Maya Bridge ▸ Install plug-in into Maya**
  button installs/repairs.
- **`cpp/`** — a compiled C++ `.mll` build of the same bridge, for a native binary
  plug-in. Build once per Maya version against that version's devkit
  (see [`plugins/maya/cpp/README.md`](plugins/maya/cpp/README.md)); drop the result
  in [`plugins/maya/prebuilt/`](plugins/maya/prebuilt) to have the installer bundle it too.

The plug-in connects over a localhost, token-authenticated Bridge API. NEXORA writes
`~/.nexora/bridge.json` on startup, so the plug-in auto-connects — no manual config.

## Architecture

```
NEXORA/
├── src/                # React + TS frontend (UI only; talks to Rust over IPC)
│   ├── lib/            # api bridge (+ browser mock), updater, types
│   ├── components/     # Sidebar, TopBar, previews, cards
│   └── pages/          # Home, Library, Search, Settings
├── src-tauri/          # Tauri desktop shell (thin) — commands, state, config, NSIS hook
├── core/               # nexora-core: the engine — DB, import, materials, library,
│                       #   bridge server. Pure Rust, headless-testable, no GUI deps.
├── plugins/maya/       # Maya plug-ins: nexora_bridge.py + compiled cpp/ + prebuilt/
└── .github/workflows/  # build.yml (installer artifact) · release.yml (signed release)
```

The key boundary: **all logic lives in `core`** so it can be unit-tested without a
GUI or Maya. `src-tauri` just locks state, calls core, and maps errors for IPC. The
frontend `api` layer has an in-memory **browser mock**, so `npm run dev` renders the
full UI in a plain browser with no Rust toolchain — handy for UI work.

## Build from source

Prerequisites: **Node 18+**, **Rust (stable)**, and Tauri 2 system deps (on Windows:
the WebView2 runtime + Microsoft C++ Build Tools — see
<https://v2.tauri.app/start/prerequisites/>).

```bash
npm install          # frontend deps
npm run app:dev      # launch the desktop app (Tauri) with hot reload
npm run dev          # UI-only preview in a browser (mock backend, no Rust)
npm run app:build    # build the installer  → target/release/bundle
```

Tests:

```bash
cargo test -p nexora-core   # engine tests: schema, import, sets/UDIM, materials, library, bridge
npm run build               # strict typecheck + production frontend build
```

## Releasing

Releases are built, signed, and published by GitHub Actions. Bump the version, then
`git tag vX.Y.Z && git push origin vX.Y.Z` — the `release` workflow builds the
installer, signs the update, generates `latest.json`, and publishes it as the latest
release so installed apps update themselves. Full details (signing keys, secrets) are
in [`RELEASING.md`](RELEASING.md).

## Status

All build phases are complete: library, import, texture sets & UDIM, materials,
search/tags/collections/health, WebGL previews, the Maya bridge, V-Ray and Arnold
adapters, Maya shader capture, and signed GitHub auto-update.

## License

MIT © FRIXXY
