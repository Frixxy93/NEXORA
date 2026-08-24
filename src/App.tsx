import { useEffect, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { Discover } from "./pages/Discover";
import { Home } from "./pages/Home";
import { Library } from "./pages/Library";
import { SearchView } from "./pages/SearchView";
import { Settings } from "./pages/Settings";
import type { View } from "./lib/nav";
import {
  api,
  onFileDrop,
  onImportDone,
  onImportProgress,
  onMaterialImported,
} from "./lib/api";
import type { ImportProgress, ImportReport, MayaStatus } from "./lib/types";

export default function App() {
  const [view, setView] = useState<View>("home");
  const [query, setQuery] = useState("");
  const [maya, setMaya] = useState<MayaStatus | null>(null);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [materialMsg, setMaterialMsg] = useState<string | null>(null);
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        const root = document.documentElement;
        const dark =
          s.appearance.theme === "dark" ||
          (s.appearance.theme === "system" &&
            window.matchMedia("(prefers-color-scheme: dark)").matches);
        root.classList.toggle("dark", dark);
      })
      .catch(console.error);
  }, []);

  // Poll Maya connection status so the indicator reflects heartbeats live
  // (the plug-in connects a few seconds after the app launches).
  useEffect(() => {
    const poll = () => api.getMayaStatus().then(setMaya).catch(() => {});
    poll();
    const iv = window.setInterval(poll, 3000);
    return () => window.clearInterval(iv);
  }, []);

  // Silent update check on launch (only if the user left "Check on startup" on).
  // Surfaces a dismissible toast when a newer release is available; the actual
  // download/install happens from Settings.
  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then(async (s) => {
        if (!s.updates.check_on_startup) return;
        const { checkForUpdate } = await import("./lib/updater");
        const result = await checkForUpdate();
        if (!cancelled && result.available && result.version) {
          setUpdateVersion(result.version);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // Global import wiring: OS file drops + progress/done toasts.
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    onFileDrop((paths) => {
      setDragging(false);
      if (paths.length) api.importPaths(paths).catch(console.error);
    }).then((u) => unlisteners.push(u));
    onImportProgress((p) => {
      setReport(null);
      setProgress(p);
    }).then((u) => unlisteners.push(u));
    onImportDone((r) => {
      setProgress(null);
      setReport(r);
      window.setTimeout(() => setReport(null), 5000);
    }).then((u) => unlisteners.push(u));
    onMaterialImported((name) => {
      setMaterialMsg(name);
      window.setTimeout(() => setMaterialMsg(null), 5000);
    }).then((u) => unlisteners.push(u));
    return () => unlisteners.forEach((u) => u());
  }, []);

  const renderPage = () => {
    // A non-empty search query takes over the main area (spec §17).
    if (query.trim()) return <SearchView query={query.trim()} onNavigate={setView} />;
    if (view === "home") return <Home onNavigate={setView} />;
    if (view === "discover") return <Discover />;
    if (view === "settings") return <Settings />;
    return <Library view={view} onNavigate={setView} key={view} />;
  };

  return (
    <div
      className="flex h-full w-full relative"
      onDragOver={(e) => {
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(e) => {
        // Only clear when leaving the window, not moving between children.
        if (e.currentTarget === e.target) setDragging(false);
      }}
      onDrop={() => setDragging(false)}
    >
      <Sidebar view={view} onNavigate={setView} />
      <div className="flex-1 flex flex-col min-w-0">
        <TopBar query={query} onQuery={setQuery} maya={maya} />
        <main className="flex-1 flex min-h-0">{renderPage()}</main>
      </div>

      {dragging && (
        <div className="absolute inset-0 z-40 bg-ink-900/70 backdrop-blur-sm flex items-center justify-center pointer-events-none">
          <div className="border-2 border-dashed border-accent rounded-2xl px-10 py-8 text-center">
            <div className="text-lg font-semibold text-white">Drop to import</div>
            <div className="text-sm text-muted mt-1">Textures and folders are scanned automatically</div>
          </div>
        </div>
      )}

      {(progress || report) && <ImportToast progress={progress} report={report} />}

      {materialMsg && (
        <div className="absolute bottom-4 right-4 z-50 panel px-4 py-3 w-72 shadow-xl">
          <div className="text-xs text-slate-200">
            <div className="font-medium text-white mb-1">Material created</div>
            <div className="text-muted truncate">“{materialMsg}” added to your library</div>
          </div>
        </div>
      )}

      {updateVersion && (
        <div className="absolute bottom-4 left-4 z-50 panel px-4 py-3 w-80 shadow-xl">
          <div className="text-xs text-slate-200">
            <div className="font-medium text-white mb-1">Update available</div>
            <div className="text-muted mb-2">
              NEXORA <span className="font-mono">{updateVersion}</span> is ready to install.
            </div>
            <div className="flex gap-2">
              <button
                className="btn-primary text-xs py-1"
                onClick={() => {
                  setView("settings");
                  setUpdateVersion(null);
                }}
              >
                View in Settings
              </button>
              <button className="btn-ghost text-xs py-1" onClick={() => setUpdateVersion(null)}>
                Later
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function ImportToast({
  progress,
  report,
}: {
  progress: ImportProgress | null;
  report: ImportReport | null;
}) {
  return (
    <div className="absolute bottom-4 right-4 z-50 panel px-4 py-3 w-72 shadow-xl">
      {progress ? (
        <>
          <div className="flex items-center justify-between text-xs text-slate-200">
            <span>Importing…</span>
            <span className="tabular-nums text-muted">
              {progress.done}/{progress.total}
            </span>
          </div>
          <div className="mt-2 h-1.5 bg-ink-700 rounded overflow-hidden">
            <div
              className="h-full bg-accent transition-all"
              style={{
                width: `${progress.total ? (progress.done / progress.total) * 100 : 0}%`,
              }}
            />
          </div>
          <div className="text-[11px] text-muted mt-1.5 truncate">{progress.current}</div>
        </>
      ) : report ? (
        <div className="text-xs text-slate-200">
          <div className="font-medium text-white mb-1">Import complete</div>
          <div className="text-muted">
            {report.imported} added
            {report.duplicates ? `, ${report.duplicates} duplicate` : ""}
            {report.failed ? `, ${report.failed} failed` : ""} · {report.total} scanned
          </div>
        </div>
      ) : null}
    </div>
  );
}
