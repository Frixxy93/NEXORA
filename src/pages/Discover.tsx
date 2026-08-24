import { useEffect, useRef, useState } from "react";
import { api, onDiscoverProgress } from "../lib/api";
import type { AppSettings, SyncProgress } from "../lib/types";

const RESOLUTIONS: { id: string; label: string; note: string }[] = [
  { id: "1k", label: "1K", note: "smallest · recommended" },
  { id: "2k", label: "2K", note: "balanced" },
  { id: "4k", label: "4K", note: "large · lots of disk" },
];

function fmtBytes(n: number): string {
  if (!n) return "0 MB";
  const gb = n / 1_000_000_000;
  if (gb >= 1) return `${gb.toFixed(2)} GB`;
  return `${Math.round(n / 1_000_000)} MB`;
}

export function Discover() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [running, setRunning] = useState(false);
  const [synced, setSynced] = useState(0);
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<number | null>(null);

  const refresh = () =>
    api
      .getDiscoverStatus()
      .then((s) => {
        setRunning(s.running);
        setSynced(s.synced);
        if (s.progress && (s.progress.running || s.progress.finished)) setProgress(s.progress);
      })
      .catch(() => {});

  useEffect(() => {
    api.getSettings().then(setSettings).catch(console.error);
    refresh();
    let un: (() => void) | undefined;
    onDiscoverProgress((p) => {
      setProgress(p);
      setRunning(p.running);
      if (p.error) setError(p.error);
      if (p.finished) {
        setRunning(false);
        refresh();
      }
    }).then((u) => (un = u));
    // Light poll as a backstop (events are primary).
    pollRef.current = window.setInterval(refresh, 4000);
    return () => {
      un?.();
      if (pollRef.current) window.clearInterval(pollRef.current);
    };
  }, []);

  const setResolution = async (res: string) => {
    if (!settings) return;
    const next = structuredClone(settings);
    next.discover.resolution = res;
    setSettings(next);
    try {
      await api.saveSettings(next);
    } catch (err) {
      console.error(err);
    }
  };

  const toggleSource = async (key: "source_polyhaven" | "source_ambientcg") => {
    if (!settings || running) return;
    const next = structuredClone(settings);
    next.discover[key] = !next.discover[key];
    // Never let both sources end up off — keep at least one enabled.
    if (!next.discover.source_polyhaven && !next.discover.source_ambientcg) return;
    setSettings(next);
    try {
      await api.saveSettings(next);
    } catch (err) {
      console.error(err);
    }
  };

  const start = async () => {
    setError(null);
    try {
      await api.startDiscoverSync();
      setRunning(true);
    } catch (err) {
      setError(String(err));
    }
  };

  const stop = async () => {
    try {
      await api.stopDiscoverSync();
    } catch (err) {
      console.error(err);
    }
  };

  const pct =
    progress && progress.total ? Math.round((progress.done / progress.total) * 100) : 0;

  return (
    <div className="flex-1 overflow-y-auto px-8 py-7 max-w-3xl">
      <div className="mb-6 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold text-white">Discover</h1>
          <p className="text-sm text-muted mt-1">
            Auto-download free, public-domain (CC0) PBR textures into your library.
          </p>
        </div>
        <div className="text-right shrink-0">
          <div className="text-2xl font-semibold text-white tabular-nums">{synced}</div>
          <div className="text-[11px] text-muted">in your library</div>
        </div>
      </div>

      {/* Sources ---------------------------------------------------------- */}
      <section className="panel p-5 mb-4">
        <div className="field-label">Sources</div>
        <div className="space-y-2">
          <SourceToggle
            name="Poly Haven"
            desc="Thousands of CC0 PBR textures — one file per map."
            enabled={settings?.discover.source_polyhaven ?? true}
            disabled={running}
            onToggle={() => toggleSource("source_polyhaven")}
          />
          <SourceToggle
            name="ambientCG"
            desc="Thousands more CC0 materials — delivered as bundled ZIPs."
            enabled={settings?.discover.source_ambientcg ?? true}
            disabled={running}
            onToggle={() => toggleSource("source_ambientcg")}
          />
        </div>
        <p className="text-[11px] text-muted mt-3 leading-relaxed">
          Both libraries are CC0 (public domain) — free for any use, commercial included, no
          attribution required.
        </p>
      </section>

      {/* Resolution ------------------------------------------------------- */}
      <section className="panel p-5 mb-4">
        <div className="field-label">Download resolution</div>
        <div className="grid grid-cols-3 gap-2">
          {RESOLUTIONS.map((r) => {
            const active = settings?.discover.resolution === r.id;
            return (
              <button
                key={r.id}
                disabled={running}
                onClick={() => setResolution(r.id)}
                className={`text-left rounded-lg border px-3 py-2.5 transition-colors disabled:opacity-50 ${
                  active ? "border-accent bg-accent/10" : "border-line bg-ink-900 hover:border-ink-600"
                }`}
              >
                <div className={`text-sm font-medium ${active ? "text-white" : "text-slate-300"}`}>
                  {r.label}
                </div>
                <div className="text-[11px] text-muted mt-0.5">{r.note}</div>
              </button>
            );
          })}
        </div>
        <div className="text-[11px] text-warn leading-relaxed mt-3">
          Heads up: syncing the whole catalog is thousands of assets. At 1K that's roughly tens of
          GB; 4K is much larger. It runs in the background, skips anything already downloaded, and
          you can stop it any time.
        </div>
      </section>

      {/* Sync controls ---------------------------------------------------- */}
      <section className="panel p-5">
        <div className="flex items-center justify-between mb-3">
          <div>
            <h2 className="text-sm font-semibold text-slate-100">Auto-sync</h2>
            <p className="text-xs text-muted mt-0.5">
              {running ? "Downloading in the background…" : "Download the free catalog into your library."}
            </p>
          </div>
          {running ? (
            <button className="btn-ghost" onClick={stop}>
              Stop
            </button>
          ) : (
            <button className="btn-primary" onClick={start}>
              Start sync
            </button>
          )}
        </div>

        {progress && (progress.running || progress.finished || progress.done > 0) && (
          <div>
            <div className="h-2 bg-ink-700 rounded overflow-hidden">
              <div className="h-full bg-accent transition-all" style={{ width: `${pct}%` }} />
            </div>
            <div className="flex items-center justify-between text-[11px] text-muted mt-1.5">
              <span className="truncate">
                {progress.running
                  ? `Downloading ${progress.current || "…"}`
                  : progress.finished
                    ? "Finished"
                    : "Idle"}
              </span>
              <span className="tabular-nums">
                {progress.done}/{progress.total} · {fmtBytes(progress.bytes)}
              </span>
            </div>
            <div className="text-[11px] text-muted mt-1 tabular-nums">
              {progress.imported} imported · {progress.skipped} skipped · {progress.failed} failed
            </div>
          </div>
        )}

        {error && <div className="text-xs text-warn mt-3 leading-relaxed">{error}</div>}

        <div className="text-[11px] text-muted leading-relaxed border-t border-line pt-3 mt-4">
          Requires a library location (set it in Settings). Downloaded materials appear in your
          Materials library and work everywhere in NEXORA — previews, search, Send to Maya.
        </div>
      </section>
    </div>
  );
}

function SourceToggle({
  name,
  desc,
  enabled,
  disabled,
  onToggle,
}: {
  name: string;
  desc: string;
  enabled: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      className={`flex items-center justify-between rounded-lg border px-3 py-2.5 ${
        enabled ? "border-line bg-ink-900" : "border-line bg-ink-900/40"
      }`}
    >
      <div className="min-w-0">
        <div className={`text-sm font-medium ${enabled ? "text-slate-100" : "text-muted"}`}>
          {name}
        </div>
        <div className="text-[11px] text-muted mt-0.5 truncate">{desc}</div>
      </div>
      <button
        role="switch"
        aria-checked={enabled}
        disabled={disabled}
        onClick={onToggle}
        className={`relative ml-4 h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-50 ${
          enabled ? "bg-accent" : "bg-ink-600"
        }`}
      >
        <span
          className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${
            enabled ? "left-[18px]" : "left-0.5"
          }`}
        />
      </button>
    </div>
  );
}
