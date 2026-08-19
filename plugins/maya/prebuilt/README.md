# Prebuilt Maya plug-ins (optional — for the installer)

The installer always installs the Python plug-in (`../nexora_bridge.py`), which
works on Maya 2026 and 2027 with no compilation and has the same features. If
you'd also like the installer to drop in a **compiled `.mll`**, this is where it
goes.

Why it's manual: a `.mll` is a compiled binary that must be built against each
Maya version's devkit, which GitHub's build runners don't have — so CI can't
produce it. Build it once per version on your machine (see `../cpp/README.md`).

To include a compiled `.mll` in the installer:

1. Drop the built file here as `<version>/nexora_bridge.mll`, e.g.
   ```
   plugins/maya/prebuilt/2026/nexora_bridge.mll
   plugins/maya/prebuilt/2027/nexora_bridge.mll
   ```
2. Bundle it by adding one line per version to the `resources` map in
   `src-tauri/tauri.conf.json` (the installer hook installs it into the matching
   Maya folder automatically once it's bundled):
   ```json
   "resources": {
     "../plugins/maya/nexora_bridge.py": "maya-plugin/",
     "../plugins/maya/prebuilt/2026/nexora_bridge.mll": "maya-plugin/2026/",
     "../plugins/maya/prebuilt/2027/nexora_bridge.mll": "maya-plugin/2027/"
   }
   ```
   (Keep each destination a distinct sibling like `maya-plugin/2026/` — don't
   nest under an existing resource destination, or the bundler errors.)
3. Commit and rebuild — every installer from then on includes the `.mll`.
