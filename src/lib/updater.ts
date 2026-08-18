// Auto-update via GitHub Releases (spec §60).
//
// Thin wrapper over the Tauri updater plugin so the UI stays declarative. In a
// plain browser (dev/preview) every call resolves to a safe "not available"
// result, so Settings renders without a Tauri runtime.
//
// The real flow: check() hits the `latest.json` published on GitHub Releases,
// compares versions, and — if newer — hands back a handle. installUpdate() then
// downloads the signed bundle (streaming progress), applies it, and relaunches.
// Signature verification uses the pubkey in tauri.conf.json (see RELEASING.md).

import { runningInTauri } from "./api";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "uptodate"
  | "downloading"
  | "installing"
  | "ready"
  | "error";

export interface UpdateState {
  phase: UpdatePhase;
  currentVersion: string;
  availableVersion?: string;
  notes?: string;
  /** 0..1 while downloading. */
  progress?: number;
  error?: string;
}

// The updater plugin's Update handle is opaque to us; we just pass it back in.
type UpdateHandle = {
  version: string;
  currentVersion: string;
  body?: string;
  downloadAndInstall: (
    onEvent?: (e: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void,
  ) => Promise<void>;
};

/** The running app's version (from the bundle in Tauri, package.json in dev). */
export async function currentAppVersion(): Promise<string> {
  if (runningInTauri) {
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  }
  return "0.1.0";
}

export interface CheckResult {
  available: boolean;
  handle?: UpdateHandle;
  version?: string;
  notes?: string;
}

/**
 * Ask GitHub whether a newer release exists. Resolves `{available:false}` in the
 * browser or when already up to date; throws with a readable message when the
 * updater is unreachable or misconfigured (caller shows it as an error state).
 */
export async function checkForUpdate(): Promise<CheckResult> {
  if (!runningInTauri) return { available: false };
  const { check } = await import("@tauri-apps/plugin-updater");
  const update = (await check()) as UpdateHandle | null;
  if (!update) return { available: false };
  return {
    available: true,
    handle: update,
    version: update.version,
    notes: update.body,
  };
}

/**
 * Download + install a pending update, reporting fractional progress, then
 * relaunch the app onto the new version.
 */
export async function installUpdate(
  handle: UpdateHandle,
  onProgress?: (fraction: number) => void,
): Promise<void> {
  let total = 0;
  let received = 0;
  await handle.downloadAndInstall((e) => {
    if (e.event === "Started") {
      total = e.data?.contentLength ?? 0;
      onProgress?.(0);
    } else if (e.event === "Progress") {
      received += e.data?.chunkLength ?? 0;
      if (total > 0) onProgress?.(Math.min(1, received / total));
    } else if (e.event === "Finished") {
      onProgress?.(1);
    }
  });
  // Restart onto the freshly installed version.
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}

/** A friendlier message for common updater failures. */
export function describeUpdateError(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err);
  if (/network|dns|connect|timeout|request/i.test(msg)) {
    return "Couldn't reach the update server. Check your connection and try again.";
  }
  if (/404|not found|no release|parse|json/i.test(msg)) {
    return "No published release found yet. Once a signed release is on GitHub, updates will appear here.";
  }
  if (/signature|pubkey|verify/i.test(msg)) {
    return "Update signature couldn't be verified. The release may be unsigned or the signing key doesn't match.";
  }
  return msg;
}
