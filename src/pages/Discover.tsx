import { useEffect, useMemo, useRef, useState } from "react";
import { api, onDiscoverProgress } from "../lib/api";
import type { AppSettings, CatalogAsset, SyncProgress } from "../lib/types";
import { Icon } from "../components/Icon";

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
  const [mode, setMode] = useState<"sync" | "browse">("sync");
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

  const resolution = settings?.discover.resolution ?? "1k";

  return (
    <div className="flex-1 overflow-y-auto px-8 py-7">
      <div className="mb-5 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold text-white">Discover</h1>
          <p className="text-sm text-muted mt-1">
            Free, public-domain (CC0) PBR textures — sync the whole catalog or browse and pick.
          </p>
        </div>
        <div className="text-right shrink-0">
          <div className="text-2xl font-semibold text-white tabular-nums">{synced}</div>
          <div className="text-[11px] text-muted">in your library</div>
        </div>
      </div>

      {/* Mode tabs -------------------------------------------------------- */}
      <div className="inline-flex items-center gap-0.5 bg-ink-800 border border-line rounded-lg p-0.5 mb-4">
        {(["sync", "browse"] as const).map((m) => (
          <button
            key={m}
            onClick={() => setMode(m)}
            className={`px-3 py-1 rounded text-xs capitalize ${
              mode === m ? "bg-ink-700 text-white" : "text-muted hover:text-slate-200"
            }`}
          >
            {m === "sync" ? "Auto-sync" : "Browse"}
          </button>
        ))}
      </div>

      {mode === "sync" ? (
        <div className="max-w-3xl">
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

          <section className="panel p-5 mb-4">
            <div className="field-label">Download resolution</div>
            <ResolutionPicker value={resolution} disabled={running} onPick={setResolution} />
            <div className="text-[11px] text-warn leading-relaxed mt-3">
              Heads up: syncing the whole catalog is thousands of assets. At 1K that's roughly tens
              of GB; 4K is much larger. It runs in the background (several downloads at once,
              retrying transient errors), skips anything already downloaded, and you can stop it any
              time.
            </div>
          </section>

          <section className="panel p-5">
            <div className="flex items-center justify-between mb-3">
              <div>
                <h2 className="text-sm font-semibold text-slate-100">Auto-sync</h2>
                <p className="text-xs text-muted mt-0.5">
                  {running
                    ? "Downloading in the background…"
                    : "Download the free catalog into your library."}
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

            <ProgressBlock progress={progress} />
            {error && <div className="text-xs text-warn mt-3 leading-relaxed">{error}</div>}

            <div className="text-[11px] text-muted leading-relaxed border-t border-line pt-3 mt-4">
              Requires a library location (set it in Settings). Downloaded materials appear in your
              Materials library and work everywhere in NEXORA — previews, search, Send to Maya.
            </div>
          </section>
        </div>
      ) : (
        <BrowseView
          resolution={resolution}
          onSetResolution={setResolution}
          running={running}
          progress={progress}
          onStop={stop}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Browse mode
// ---------------------------------------------------------------------------

function BrowseView({
  resolution,
  onSetResolution,
  running,
  progress,
  onStop,
}: {
  resolution: string;
  onSetResolution: (r: string) => void;
  running: boolean;
  progress: SyncProgress | null;
  onStop: () => void;
}) {
  const [assets, setAssets] = useState<CatalogAsset[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [visible, setVisible] = useState(120);
  const [dlError, setDlError] = useState<string | null>(null);

  const load = () => {
    setAssets(null);
    setLoadErr(null);
    api
      .discoverBrowse()
      .then(setAssets)
      .catch((e) => setLoadErr(String(e)));
  };
  useEffect(load, []);

  const categories = useMemo(() => {
    const s = new Set<string>();
    (assets ?? []).forEach((a) => a.categories.forEach((c) => s.add(c)));
    return ["all", ...[...s].sort()];
  }, [assets]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (assets ?? []).filter((a) => {
      if (category !== "all" && !a.categories.includes(category)) return false;
      if (q && !a.name.toLowerCase().includes(q) && !a.categories.some((c) => c.includes(q)))
        return false;
      return true;
    });
  }, [assets, search, category]);

  useEffect(() => setVisible(120), [search, category]);

  const shown = filtered.slice(0, visible);

  const toggle = (id: string) =>
    setSelected((prev) => {
      const n = new Set(prev);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const selectAll = () => setSelected(new Set(filtered.filter((a) => !a.synced).map((a) => a.id)));
  const clear = () => setSelected(new Set());

  const download = async () => {
    setDlError(null);
    const items = filtered
      .filter((a) => selected.has(a.id) && !a.synced)
      .map((a) => ({ source: a.source, id: a.id }));
    if (!items.length) return;
    try {
      await api.startDiscoverDownload(items);
      clear();
    } catch (err) {
      setDlError(String(err));
    }
  };

  if (loadErr)
    return (
      <div className="panel p-5 text-xs text-warn max-w-3xl">
        Couldn’t load the catalog: {loadErr}
        <button className="btn-ghost text-xs ml-3" onClick={load}>
          Retry
        </button>
      </div>
    );
  if (!assets)
    return (
      <div className="panel p-5 text-sm text-muted max-w-3xl">Loading Poly Haven catalog…</div>
    );

  return (
    <>
      <div className="panel p-4 mb-4 space-y-3">
        <div className="flex gap-2 flex-wrap items-center">
          <input
            className="input text-xs py-1.5 flex-1 min-w-[160px]"
            placeholder="Search textures…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <select
            className="input text-xs py-1.5 w-40"
            value={category}
            onChange={(e) => setCategory(e.target.value)}
          >
            {categories.map((c) => (
              <option key={c} value={c}>
                {c === "all" ? "All categories" : c}
              </option>
            ))}
          </select>
          <select
            className="input text-xs py-1.5 w-24"
            value={resolution}
            onChange={(e) => onSetResolution(e.target.value)}
          >
            {RESOLUTIONS.map((r) => (
              <option key={r.id} value={r.id}>
                {r.label}
              </option>
            ))}
          </select>
        </div>

        <div className="flex items-center gap-2 text-xs">
          <span className="text-muted tabular-nums">
            {filtered.length} textures · {selected.size} selected
          </span>
          <div className="ml-auto flex items-center gap-2">
            <button className="btn-ghost text-xs" onClick={selectAll}>
              Select all
            </button>
            <button className="btn-ghost text-xs" onClick={clear} disabled={!selected.size}>
              Clear
            </button>
            {running ? (
              <button className="btn-ghost text-xs" onClick={onStop}>
                Stop
              </button>
            ) : (
              <button
                className="btn-primary text-xs"
                disabled={!selected.size}
                onClick={download}
              >
                Download {selected.size || ""}
              </button>
            )}
          </div>
        </div>

        <ProgressBlock progress={progress} />
        {dlError && <div className="text-xs text-warn">{dlError}</div>}
      </div>

      <div
        className="grid gap-3"
        style={{ gridTemplateColumns: "repeat(auto-fill, minmax(130px, 1fr))" }}
      >
        {shown.map((a) => (
          <BrowseCard
            key={a.id}
            asset={a}
            selected={selected.has(a.id)}
            onToggle={() => toggle(a.id)}
          />
        ))}
      </div>

      {visible < filtered.length && (
        <div className="text-center py-4">
          <button className="btn-ghost text-xs" onClick={() => setVisible((v) => v + 120)}>
            Show more ({filtered.length - visible} more)
          </button>
        </div>
      )}
    </>
  );
}

function BrowseCard({
  asset,
  selected,
  onToggle,
}: {
  asset: CatalogAsset;
  selected: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      onClick={onToggle}
      disabled={asset.synced}
      className={`group relative rounded-lg overflow-hidden bg-ink-800 border text-left transition-colors ${
        selected ? "border-accent" : "border-line hover:border-ink-600"
      } ${asset.synced ? "opacity-50 cursor-default" : ""}`}
    >
      <div className="aspect-square bg-ink-900 relative overflow-hidden">
        {asset.thumbnail_url ? (
          <img
            src={asset.thumbnail_url}
            alt={asset.name}
            loading="lazy"
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-ink-600">
            <Icon name="texture" size={22} />
          </div>
        )}
        {selected && <div className="absolute inset-0 ring-2 ring-accent ring-inset" />}
        {asset.synced && (
          <span className="absolute top-1.5 left-1.5 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-ink-900/80 text-good">
            In library
          </span>
        )}
        {selected && (
          <span className="absolute top-1.5 right-1.5 text-accent bg-ink-900/80 rounded-full p-0.5">
            <Icon name="check" size={13} />
          </span>
        )}
      </div>
      <div className="px-2 py-1.5 text-[11px] text-slate-200 truncate">{asset.name}</div>
    </button>
  );
}

// ---------------------------------------------------------------------------
// Shared bits
// ---------------------------------------------------------------------------

function ResolutionPicker({
  value,
  disabled,
  onPick,
}: {
  value: string;
  disabled: boolean;
  onPick: (r: string) => void;
}) {
  return (
    <div className="grid grid-cols-3 gap-2">
      {RESOLUTIONS.map((r) => {
        const active = value === r.id;
        return (
          <button
            key={r.id}
            disabled={disabled}
            onClick={() => onPick(r.id)}
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
  );
}

function ProgressBlock({ progress }: { progress: SyncProgress | null }) {
  if (!progress || !(progress.running || progress.finished || progress.done > 0)) return null;
  const pct = progress.total ? Math.round((progress.done / progress.total) * 100) : 0;
  return (
    <div className="mt-1">
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
      {progress.failed > 0 && progress.last_error && (
        <div className="text-[11px] text-muted mt-1 truncate">Last failure: {progress.last_error}</div>
      )}
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
