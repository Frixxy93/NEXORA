# NEXORA Bridge — Maya plug-in

Connects Maya to the NEXORA desktop app over its localhost Bridge API. Browse
your NEXORA library inside Maya, apply materials/textures, and capture Maya
shaders back into NEXORA.

## Requirements

- Maya 2022+ (Python 3; PySide2 **or** PySide6 — both supported). Verified for
  **Maya 2026** (Python 3.11 / PySide6) and **Maya 2027** (Python 3.13 / PySide6)
  — this script loads on both with no changes.
- The NEXORA desktop app running (it hosts the Bridge API and writes the
  connection file `~/.nexora/bridge.json`)

> No compilation needed — this is the drop-in variant. There's also a compiled
> C++ `.mll` in `cpp/` if you prefer a binary plug-in like `FrixxyMatLib.mll`
> (build it once per Maya version).

## Install

1. Copy `nexora_bridge.py` into a Maya plug-in path, e.g.
   - Windows: `Documents\maya\<version>\plug-ins\`
   - macOS: `~/Library/Preferences/Autodesk/maya/<version>/plug-ins/`
   - Linux: `~/maya/<version>/plug-ins/`
   (Create the `plug-ins` folder if it doesn't exist.)
2. In Maya: **Windows ▸ Settings/Preferences ▸ Plug-in Manager**, find
   `nexora_bridge.py`, tick **Loaded** (and **Auto load** to load on startup).

On load you'll get a **NEXORA** menu and a **NEXORA** shelf.

## Use

- **NEXORA ▸ Open NEXORA Library** — browse materials/textures from your NEXORA
  library; double-click (or *Apply / Import Selected*) to build the shader and
  assign it to the selected objects, or drop a texture in as a `file` node.
- **Send to Maya** from the desktop app — the plug-in polls every ~2.5s and
  applies queued sends automatically (no clicking needed).
- **Capture Selected** — select a shaded object (or its shader) and NEXORA reads
  the connected file textures and saves them as a new material in your library
  (spec §39).
- **Scan Scene** — reports scene file textures and any missing paths.

## Renderer support

The shader builder targets, in order of what's loaded:

- **Arnold** (`mtoa`) → `aiStandardSurface`
- **V-Ray** (`vrayformaya`) → `VRayMtl`
- otherwise → `standardSurface` (generic PBR)

Base color, roughness, metalness, normal, bump, and height/displacement are
wired up; missing maps are skipped. Each renderer has a dedicated first-class
adapter: **Arnold** uses `aiNormalMap`/`aiBump2d`; **V-Ray** uses `VRayMtl`'s
`bumpMap` (with `useRoughness`); generic PBR uses `bump2d`. Which renderer is
targeted follows your NEXORA renderer preference when that renderer is loaded,
otherwise it auto-detects. UDIM sets are wired with Mari tiling (`uvTilingMode 3`).

## Scripting

The plug-in registers a real Maya command, so you can also open the library from
the Script Editor or a hotkey:

```python
import maya.cmds as cmds
cmds.nexoraLibrary()
```

## Connection

The plug-in reads `~/.nexora/bridge.json` (`{host, port, token}`), written by the
desktop app on startup. If you started NEXORA after Maya, use **NEXORA ▸
Reconnect** (or *Refresh* in the library window). All traffic is localhost-only
and authenticated with the token.
