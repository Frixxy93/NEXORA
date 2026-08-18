import { useEffect, useState } from "react";
import { api, pickFolder, runningInTauri } from "../lib/api";
import type { AppSettings, BridgeInfo, Renderer, ThemeMode } from "../lib/types";
import {
  checkForUpdate,
  currentAppVersion,
  describeUpdateError,
  installUpdate,
  type CheckResult,
  type UpdatePhase,
} from "../lib/updater";

export function Settings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [bridge, setBridge] = useState<BridgeInfo | null>(null);
  const [showToken, setShowToken] = useState(false);

  useEffect(() => {
    api.getSettings().then(setSettings).catch(console.error);
    const loadBridge = () => api.getBridgeInfo().then(setBridge).catch(() => {});
    loadBridge();
    const iv = window.setInterval(loadBridge, 4000); // live Maya status
    return () => window.clearInterval(iv);
  }, []);

  if (!settings) {
    return <div className="flex-1 flex items-center justify-center text-muted">Loading…</div>;
  }

  const update = (patch: (s: AppSettings) => void) => {
    setSettings((prev) => {
      if (!prev) return prev;
      const next = structuredClone(prev);
      patch(next);
      return next;
    });
    setSaved(false);
  };

  const save = async () => {
    setBusy(true);
    try {
      await api.saveSettings(settings);
      setSaved(true);
    } catch (err) {
      console.error(err);
    } finally {
      setBusy(false);
    }
  };

  const chooseLibrary = async () => {
    const path = await pickFolder();
    if (!path) return;
    setBusy(true);
    try {
      const status = await api.initLibrary(path, settings.library.storage_mode === "managed");
      update((s) => {
        s.library.location = status.location;
      });
      setSaved(true);
    } catch (err) {
      console.error(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto px-8 py-7 max-w-3xl">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-semibold text-white">Settings</h1>
          <p className="text-sm text-muted mt-1">Configure your library, imports, and updates.</p>
        </div>
        <button className="btn-primary" onClick={save} disabled={busy}>
          {busy ? "Saving…" : saved ? "Saved ✓" : "Save changes"}
        </button>
      </div>

      {/* Library ---------------------------------------------------------- */}
      <Section title="Library" subtitle="Where NEXORA keeps your assets (files stay on disk).">
        <Field label="Library location">
          <div className="flex gap-2">
            <input
              className="input font-mono text-xs"
              readOnly
              value={settings.library.location ?? ""}
              placeholder="Not set — choose a folder"
            />
            <button className="btn-ghost whitespace-nowrap" onClick={chooseLibrary} disabled={busy}>
              Choose…
            </button>
          </div>
        </Field>

        <Field label="Storage mode">
          <div className="grid grid-cols-2 gap-2">
            <ModeCard
              active={settings.library.storage_mode === "managed"}
              title="Managed"
              desc="Copy files into the NEXORA library."
              onClick={() => update((s) => (s.library.storage_mode = "managed"))}
            />
            <ModeCard
              active={settings.library.storage_mode === "referenced"}
              title="Referenced"
              desc="Index files in place, don't copy."
              onClick={() => update((s) => (s.library.storage_mode = "referenced"))}
            />
          </div>
        </Field>

        <Toggle
          label="Auto-scan library folders"
          checked={settings.library.auto_scan}
          onChange={(v) => update((s) => (s.library.auto_scan = v))}
        />
      </Section>

      {/* Appearance ------------------------------------------------------- */}
      <Section title="Appearance">
        <Field label="Theme">
          <div className="grid grid-cols-3 gap-2">
            {(["dark", "light", "system"] as ThemeMode[]).map((t) => (
              <ModeCard
                key={t}
                active={settings.appearance.theme === t}
                title={t[0].toUpperCase() + t.slice(1)}
                onClick={() => update((s) => (s.appearance.theme = t))}
              />
            ))}
          </div>
        </Field>
        <Field label={`Grid size — ${settings.appearance.grid_size}px`}>
          <input
            type="range"
            min={120}
            max={320}
            step={10}
            value={settings.appearance.grid_size}
            onChange={(e) => update((s) => (s.appearance.grid_size = Number(e.target.value)))}
            className="w-full accent-accent"
          />
        </Field>
      </Section>

      {/* Import ----------------------------------------------------------- */}
      <Section title="Import" subtitle="What NEXORA does automatically on import.">
        <Toggle
          label="Auto-detect map types"
          checked={settings.import.auto_detect_maps}
          onChange={(v) => update((s) => (s.import.auto_detect_maps = v))}
        />
        <Toggle
          label="Auto-group texture sets"
          checked={settings.import.auto_group_texture_sets}
          onChange={(v) => update((s) => (s.import.auto_group_texture_sets = v))}
        />
        <Toggle
          label="Auto-generate previews"
          checked={settings.import.auto_generate_preview}
          onChange={(v) => update((s) => (s.import.auto_generate_preview = v))}
        />
        <Toggle
          label="Auto-tag"
          checked={settings.import.auto_tag}
          onChange={(v) => update((s) => (s.import.auto_tag = v))}
        />
      </Section>

      {/* Renderer --------------------------------------------------------- */}
      <Section title="Renderer" subtitle="Default target when applying materials in Maya.">
        <div className="grid grid-cols-3 gap-2">
          {(
            [
              ["generic_pbr", "Generic PBR"],
              ["vray", "V-Ray"],
              ["arnold", "Arnold"],
            ] as [Renderer, string][]
          ).map(([id, label]) => (
            <ModeCard
              key={id}
              active={settings.default_renderer === id}
              title={label}
              onClick={() => update((s) => (s.default_renderer = id))}
            />
          ))}
        </div>
      </Section>

      {/* Maya Bridge ------------------------------------------------------ */}
      <Section title="Maya Bridge" subtitle="Connect Maya to NEXORA via the local plug-in.">
        <div className="flex items-center gap-2 text-sm">
          <span
            className={`w-2.5 h-2.5 rounded-full ${
              bridge?.connected ? "bg-good" : "bg-ink-600"
            }`}
          />
          <span className="text-slate-200">
            {bridge?.connected
              ? `Maya connected${bridge.maya_version ? ` (${bridge.maya_version})` : ""}`
              : "Maya not connected"}
          </span>
        </div>

        <Field label="Bridge API">
          <div className="text-xs text-muted">
            Serving on{" "}
            <span className="font-mono text-slate-300">
              127.0.0.1:{bridge?.port ?? "…"}
            </span>{" "}
            (localhost only, token-authenticated)
          </div>
        </Field>

        <Field label="Auth token">
          <div className="flex gap-2">
            <input
              className="input font-mono text-xs"
              readOnly
              value={showToken ? bridge?.token ?? "" : "•".repeat((bridge?.token ?? "").length || 24)}
            />
            <button className="btn-ghost whitespace-nowrap" onClick={() => setShowToken((s) => !s)}>
              {showToken ? "Hide" : "Show"}
            </button>
          </div>
        </Field>

        <div className="text-[11px] text-muted leading-relaxed border-t border-line pt-3">
          Install the plug-in from{" "}
          <span className="font-mono text-slate-300">plugins/maya/nexora_bridge.py</span> into your
          Maya <span className="font-mono text-slate-300">plug-ins</span> folder and enable it in the
          Plug-in Manager. NEXORA auto-writes the connection to{" "}
          <span className="font-mono text-slate-300">~/.nexora/bridge.json</span>, so the plug-in
          connects on its own. Then use <b>Send to Maya</b> on any texture or material, or browse the
          library from Maya’s <b>NEXORA</b> menu.
        </div>
      </Section>

      {/* Updates ---------------------------------------------------------- */}
      <Section title="Updates" subtitle="Application updates via GitHub Releases.">
        <Toggle
          label="Automatic updates"
          checked={settings.updates.automatic_updates}
          onChange={(v) => update((s) => (s.updates.automatic_updates = v))}
        />
        <Toggle
          label="Check on startup"
          checked={settings.updates.check_on_startup}
          onChange={(v) => update((s) => (s.updates.check_on_startup = v))}
        />
        <Field label="Channel">
          <div className="grid grid-cols-2 gap-2">
            <ModeCard
              active={settings.updates.channel === "stable"}
              title="Stable"
              onClick={() => update((s) => (s.updates.channel = "stable"))}
            />
            <ModeCard
              active={settings.updates.channel === "beta"}
              title="Beta"
              onClick={() => update((s) => (s.updates.channel = "beta"))}
            />
          </div>
        </Field>

        <div className="border-t border-line pt-4">
          <UpdatePanel />
        </div>
      </Section>
    </div>
  );
}

// --- update panel ----------------------------------------------------------

function UpdatePanel() {
  const [version, setVersion] = useState<string>("");
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [found, setFound] = useState<CheckResult | null>(null);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    currentAppVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  const check = async () => {
    setPhase("checking");
    setError(null);
    setFound(null);
    try {
      const result = await checkForUpdate();
      if (result.available) {
        setFound(result);
        setPhase("available");
      } else {
        setPhase("uptodate");
      }
    } catch (err) {
      setError(describeUpdateError(err));
      setPhase("error");
    }
  };

  const install = async () => {
    if (!found?.handle) return;
    setPhase("downloading");
    setProgress(0);
    try {
      await installUpdate(found.handle, setProgress);
      setPhase("ready"); // (app relaunches; this is a fallback if it doesn't)
    } catch (err) {
      setError(describeUpdateError(err));
      setPhase("error");
    }
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="text-sm text-slate-300">
          Current version
          <span className="font-mono text-xs text-muted ml-2">
            {version ? `v${version}` : "…"}
          </span>
        </div>
        <button
          className="btn-ghost"
          onClick={check}
          disabled={phase === "checking" || phase === "downloading"}
        >
          {phase === "checking" ? "Checking…" : "Check for updates"}
        </button>
      </div>

      {!runningInTauri && (
        <div className="text-[11px] text-muted">
          Update checks run in the installed desktop app.
        </div>
      )}

      {phase === "uptodate" && (
        <div className="text-xs text-good">You’re on the latest version.</div>
      )}

      {phase === "error" && error && (
        <div className="text-xs text-warn leading-relaxed">{error}</div>
      )}

      {(phase === "available" || phase === "downloading") && found && (
        <div className="rounded-lg border border-accent/40 bg-accent/5 p-3 space-y-2">
          <div className="text-sm text-white">
            Version <span className="font-mono">{found.version}</span> is available
          </div>
          {found.notes && (
            <div className="text-[11px] text-muted whitespace-pre-line max-h-28 overflow-y-auto">
              {found.notes}
            </div>
          )}
          {phase === "downloading" ? (
            <div>
              <div className="h-1.5 bg-ink-700 rounded overflow-hidden">
                <div
                  className="h-full bg-accent transition-all"
                  style={{ width: `${Math.round(progress * 100)}%` }}
                />
              </div>
              <div className="text-[11px] text-muted mt-1">
                Downloading… {Math.round(progress * 100)}% — the app will restart when it’s done.
              </div>
            </div>
          ) : (
            <button className="btn-primary" onClick={install}>
              Download &amp; install
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// --- small building blocks -------------------------------------------------

function Section({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="panel p-5 mb-4">
      <h2 className="text-sm font-semibold text-slate-100">{title}</h2>
      {subtitle && <p className="text-xs text-muted mt-0.5 mb-3">{subtitle}</p>}
      <div className="space-y-4 mt-3">{children}</div>
    </section>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="field-label">{label}</div>
      {children}
    </div>
  );
}

function ModeCard({
  active,
  title,
  desc,
  onClick,
}: {
  active: boolean;
  title: string;
  desc?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`text-left rounded-lg border px-3 py-2.5 transition-colors ${
        active
          ? "border-accent bg-accent/10"
          : "border-line bg-ink-900 hover:border-ink-600"
      }`}
    >
      <div className={`text-sm font-medium ${active ? "text-white" : "text-slate-300"}`}>
        {title}
      </div>
      {desc && <div className="text-[11px] text-muted mt-0.5">{desc}</div>}
    </button>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between cursor-pointer">
      <span className="text-sm text-slate-300">{label}</span>
      <button
        type="button"
        onClick={() => onChange(!checked)}
        className={`w-10 h-6 rounded-full p-0.5 transition-colors ${
          checked ? "bg-accent" : "bg-ink-700"
        }`}
      >
        <span
          className={`block w-5 h-5 rounded-full bg-white transition-transform ${
            checked ? "translate-x-4" : ""
          }`}
        />
      </button>
    </label>
  );
}
