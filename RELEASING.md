# Releasing NEXORA (auto-update via GitHub Releases)

NEXORA ships built-in auto-update (spec §60). The desktop app checks a
`latest.json` manifest on your GitHub Releases, and — when a newer, **signed**
build exists — downloads it, verifies the signature, installs it, and relaunches.

This file is the one-time setup plus the per-release checklist.

---

## How it works

- The app is built with `"createUpdaterArtifacts": true` (in `tauri.conf.json`),
  so `tauri build` emits an updater bundle (`.zip` / `.tar.gz` / NSIS installer)
  **and** a detached signature (`.sig`) for each target.
- On launch (and from **Settings ▸ Updates ▸ Check for updates**) the app calls
  the updater endpoint:
  `https://github.com/FRIXXY/NEXORA/releases/latest/download/latest.json`
- `latest.json` lists the newest version, per-platform download URLs, and the
  signature. The app verifies that signature against the **public key** baked
  into `tauri.conf.json` before installing. No valid signature → no install.

> Update the `endpoints` URL in `tauri.conf.json` if your repo isn't
> `FRIXXY/NEXORA`.

---

## One-time setup — signing keys

The updater will not install anything that isn't signed with your private key.
Generate the keypair once:

```powershell
npm run tauri signer generate -- -w $HOME\.nexora-updater.key
```

This prints (and saves) two things:

1. A **private key** (file `~/.nexora-updater.key`) + its password. Keep both
   secret — anyone with them can push an update to every NEXORA install. Never
   commit them.
2. A **public key** (base64). Paste it into `src-tauri/tauri.conf.json`:

```json
"plugins": {
  "updater": {
    "endpoints": ["https://github.com/FRIXXY/NEXORA/releases/latest/download/latest.json"],
    "pubkey": "PASTE_THE_PUBLIC_KEY_HERE"
  }
}
```

(The repo currently has the placeholder `REPLACE_WITH_TAURI_SIGNER_PUBLIC_KEY`
there — the app runs fine with it, but update checks will report "no signed
release" until you replace it and cut a real release.)

---

## Per-release checklist

1. **Bump the version** in `tauri.conf.json` (`version`) and `package.json`.
   The updater compares this against the running app's version.

2. **Build with the signing key in the environment** so Tauri signs the bundle:

   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.nexora-updater.key -Raw
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<the password you set>"
   npm run app:build
   ```

   Output lands in `src-tauri/target/release/bundle/`. For each updater target
   you'll get an artifact and a matching `.sig`, e.g.
   `NEXORA_0.2.0_x64-setup.nsis.zip` + `.sig` on Windows.

3. **Write `latest.json`** next to the artifacts (Tauri's manifest format):

   ```json
   {
     "version": "0.2.0",
     "notes": "What changed in this release.",
     "pub_date": "2026-01-01T00:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "<contents of the .sig file>",
         "url": "https://github.com/FRIXXY/NEXORA/releases/download/v0.2.0/NEXORA_0.2.0_x64-setup.nsis.zip"
       }
     }
   }
   ```

   `signature` is the **text inside** the `.sig` file, not its path. Add
   `darwin-x86_64` / `darwin-aarch64` / `linux-x86_64` entries if you build
   those targets.

4. **Publish a GitHub Release** tagged `v0.2.0` and upload the artifact(s), each
   `.sig`, and `latest.json`. Because the endpoint points at
   `releases/latest/download/latest.json`, marking the release "latest" is what
   makes existing installs see it.

5. **Verify**: open an older NEXORA build → **Settings ▸ Updates ▸ Check for
   updates** → it should offer 0.2.0, download with a progress bar, install, and
   relaunch on the new version.

---

## Automating it (optional)

The [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action)
GitHub Action builds per-OS, signs (reading the key from repo secrets
`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), generates
`latest.json`, and attaches everything to the release — so a pushed tag ships an
update to every platform. Add the secrets under **Settings ▸ Secrets and
variables ▸ Actions** and never store the private key in the repo.

---

## Channels

Settings exposes a **Stable / Beta** channel toggle. To honor it, publish beta
builds to a separate manifest (e.g. `latest-beta.json`) and switch the
`endpoints` URL — or use per-release prerelease tags — when wiring the channel
through. The UI preference is stored today; the split-manifest plumbing is the
follow-up.
