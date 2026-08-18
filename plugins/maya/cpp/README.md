# NEXORA Bridge — compiled Maya plug-in (.mll)

The C++ build of the NEXORA Maya bridge — a real compiled plug-in (like
`FrixxyMatLib.mll`). Same features as the Python version, but shipped as a
binary `.mll`: it registers Maya commands, runs a poller, talks to the NEXORA
desktop over the localhost Bridge API, and builds/assigns shader networks.

> There's also a pure-Python version in the parent folder
> (`../nexora_bridge.py`) that needs no compilation. Use whichever you prefer —
> this one is the compiled `.mll`.

## What it does

- **Commands** (real registered Maya commands):
  - `nexoraLibrary` — open a browser window of your NEXORA materials/textures;
    double-click (or the button) applies to the current selection.
  - `nexoraApply -id "<NX-...>" -k "material|texture"` — build + assign a
    material, or drop in a texture.
  - `nexoraCapture [-n "Name"]` — read the selected object's shader file
    textures and save them to NEXORA as a material (spec §39).
  - `nexoraSync` — heartbeat + apply anything queued by the desktop's
    "Send to Maya".
- **Poller** — an `MTimerMessage` fires every ~2.5s to heartbeat (so NEXORA
  shows "Maya connected") and drain the send queue automatically.
- **NEXORA menu + shelf** are added on load.

Connection details (host/port/token) come from `~/.nexora/bridge.json`, which
the NEXORA desktop app writes on startup. Everything is localhost + token-auth.

## Build (Maya 2026 & 2027)

A `.mll` is **version-specific** — build it once per Maya version against that
version's devkit, then install each into the matching Maya. Both 2026 and 2027
use **Visual Studio 2022 (x64)** and **C++17** (already set in CMake).

| Maya | Compiler | Python* | Qt/PySide* |
| ---- | -------- | ------- | ---------- |
| 2026 | VS 2022  | 3.11    | PySide6 6.5 |
| 2027 | VS 2022 (17.14.x) | 3.13 | PySide6 (Qt6) |

\* Python/PySide don't matter for this C++ build (no Python embedding) — they're
listed only because the Python plug-in variant uses them.

**Run from this folder** (`plugins\maya\cpp`) — that's where CMakeLists.txt is.
Commands below are **PowerShell** (`PS>`). In Command Prompt, use
`set VAR=value` instead of `$env:VAR="value"`.

First confirm the devkit headers are present:
```powershell
$env:MAYA_LOCATION = "C:\Program Files\Autodesk\Maya2027"
Test-Path "$env:MAYA_LOCATION\include\maya\MFnPlugin.h"   # must print True
```
If that's **False**, your Maya install doesn't ship headers — download the
**Maya devkit (devkitBase)** for that version, extract it, and set
`MAYA_LOCATION` to the extracted folder (it contains `include\` and `lib\`).

**Maya 2027 (PowerShell):**
```powershell
cd D:\FRIXXY\NEXORA\plugins\maya\cpp
$env:MAYA_LOCATION = "C:\Program Files\Autodesk\Maya2027"
cmake -B build2027 -S . -G "Visual Studio 17 2022" -A x64
cmake --build build2027 --config Release
# -> build2027\Release\nexora_bridge.mll
```

**Maya 2026 (PowerShell):**
```powershell
$env:MAYA_LOCATION = "C:\Program Files\Autodesk\Maya2026"
cmake -B build2026 -S . -G "Visual Studio 17 2022" -A x64
cmake --build build2026 --config Release
```

> Configure fetches header-only **nlohmann/json** over the internet. To build
> offline, drop `json.hpp` into `./nlohmann/json.hpp` and add
> `-DNEXORA_VENDORED_JSON=ON`.

## Install (per version)

Copy each build's `nexora_bridge.mll` into that version's plug-in path and enable
it in **Windows ▸ Settings/Preferences ▸ Plug-in Manager** (*Loaded*, and
*Auto load* for startup):

- Maya 2027 build → `Documents\maya\2027\plug-ins\`
- Maya 2026 build → `Documents\maya\2026\plug-ins\`

You'll get a **NEXORA** menu + shelf. (The Python variant in `../` needs no
compiling and loads in both 2026 and 2027 as-is.)

## Notes / honesty

This C++ source was written and statically reviewed but **not compiled here** (no
Maya devkit in the authoring environment). Treat the first build as a shakeout —
if the compiler flags anything (a header path, a symbol, a MEL string), send me
the exact error and I'll fix it. The heavy scene/shader work is done via MEL that
the plug-in executes, which is the same logic verified in the Python version, so
most risk is in the C++ glue (Maya API calls, sockets), not the shading.
