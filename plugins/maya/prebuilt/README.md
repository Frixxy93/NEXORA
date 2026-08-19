# Prebuilt Maya plug-ins (optional — for the installer)

Drop compiled `.mll` plug-ins here and the NEXORA Windows installer will copy
them into the matching Maya `plug-ins` folder automatically, alongside the
Python plug-in it always installs.

Expected layout (one `.mll` per Maya version, each named `nexora_bridge.mll`):

```
plugins/maya/prebuilt/
  2026/nexora_bridge.mll   ← built against the Maya 2026 devkit
  2027/nexora_bridge.mll   ← built against the Maya 2027 devkit
```

Why this is manual: a `.mll` is a compiled binary that must be built against
each Maya version's devkit, which GitHub's build runners don't have — so CI
can't produce it. Build it once per version on your machine (see
`../cpp/README.md`), copy the result here as `<version>/nexora_bridge.mll`,
commit it, and every installer from then on includes it.

If this folder has no `.mll` files, the installer just installs the Python
plug-in (`../nexora_bridge.py`), which works on Maya 2026 and 2027 with no
compilation and has the same features.
