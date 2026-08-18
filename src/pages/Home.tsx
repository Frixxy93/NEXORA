import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { LibraryHealth, LibraryStats, LibraryStatus, MixedAssets } from "../lib/types";
import { StatCard } from "../components/StatCard";
import { Icon, type IconName } from "../components/Icon";
import type { View } from "../lib/nav";

function greeting(): string {
  const h = new Date().getHours();
  if (h < 12) return "Good morning.";
  if (h < 18) return "Good afternoon.";
  return "Good evening.";
}

export function Home({ onNavigate }: { onNavigate: (v: View) => void }) {
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [status, setStatus] = useState<LibraryStatus | null>(null);
  const [health, setHealth] = useState<LibraryHealth | null>(null);
  const [recent, setRecent] = useState<MixedAssets>({ materials: [], textures: [] });

  useEffect(() => {
    api.getLibraryStats().then(setStats).catch(console.error);
    api.getLibraryStatus().then(setStatus).catch(console.error);
    api.getLibraryHealth().then(setHealth).catch(console.error);
    api.listRecentAdded().then(setRecent).catch(console.error);
  }, []);

  const configured = status?.configured ?? false;

  return (
    <div className="flex-1 overflow-y-auto px-8 py-7">
      <h1 className="text-2xl font-semibold text-white">{greeting()}</h1>
      <p className="text-sm text-muted mt-1">
        {configured
          ? `Library at ${status?.location}`
          : "No library configured yet — set one up in Settings to start importing."}
      </p>

      <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-5 gap-3 mt-6">
        <StatCard label="Materials" value={stats?.materials ?? 0} icon="material" />
        <StatCard label="Textures" value={stats?.textures ?? 0} icon="texture" />
        <StatCard label="Texture Sets" value={stats?.texture_sets ?? 0} icon="layers" />
        <StatCard label="Favorites" value={stats?.favorites ?? 0} icon="star" />
        <StatCard label="Recently Added" value={stats?.recently_added ?? 0} icon="plus" />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 mt-7">
        <section className="panel p-5 lg:col-span-2 space-y-5">
          <div>
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-sm font-semibold text-slate-200">Recent Materials</h2>
              <button className="btn-ghost text-xs" onClick={() => onNavigate("lib.materials")}>
                View all
              </button>
            </div>
            {recent.materials.length === 0 ? (
              <p className="text-[11px] text-muted">No materials yet — import a material folder to see it here.</p>
            ) : (
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                {recent.materials.slice(0, 4).map((m) => (
                  <RecentTile
                    key={m.id}
                    name={m.name}
                    sub={m.category ?? "Material"}
                    texId={m.preview_texture_id}
                    icon="material"
                    onClick={() => onNavigate("lib.materials")}
                  />
                ))}
              </div>
            )}
          </div>

          <div>
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-sm font-semibold text-slate-200">Recent Textures</h2>
              <button className="btn-ghost text-xs" onClick={() => onNavigate("lib.textures")}>
                View all
              </button>
            </div>
            {recent.textures.length === 0 ? (
              <p className="text-[11px] text-muted">No textures yet — drag some onto the window to import.</p>
            ) : (
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                {recent.textures.slice(0, 4).map((t) => (
                  <RecentTile
                    key={t.id}
                    name={t.name}
                    sub={t.map_type ? t.map_type.replace("_", " ") : "Texture"}
                    texId={t.id}
                    icon="texture"
                    onClick={() => onNavigate("lib.textures")}
                  />
                ))}
              </div>
            )}
          </div>
        </section>

        <section className="panel p-5">
          <h2 className="text-sm font-semibold text-slate-200 mb-4">Library Health</h2>
          <HealthRow label="Assets" value={health?.assets ?? 0} tone="neutral" />
          <HealthRow label="Healthy" value={health?.healthy ?? 0} tone="good" />
          <HealthRow label="Missing files" value={health?.missing_files ?? 0} tone="bad" />
          <HealthRow label="Duplicates" value={health?.duplicates ?? 0} tone="warn" />
          <HealthRow
            label="Incomplete materials"
            value={health?.incomplete_materials ?? 0}
            tone="warn"
          />
          <HealthRow label="Broken references" value={health?.broken_references ?? 0} tone="bad" />
        </section>
      </div>
    </div>
  );
}

function RecentTile({
  name,
  sub,
  texId,
  icon,
  onClick,
}: {
  name: string;
  sub: string;
  texId: string | null;
  icon: IconName;
  onClick: () => void;
}) {
  const [thumb, setThumb] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    if (texId) api.getThumbnail(texId).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [texId]);
  return (
    <button
      onClick={onClick}
      className="rounded-lg overflow-hidden bg-ink-800 border border-line hover:border-ink-600 text-left"
    >
      <div className="aspect-square bg-ink-900 flex items-center justify-center overflow-hidden">
        {thumb ? (
          <img src={thumb} alt={name} className="w-full h-full object-cover" loading="lazy" />
        ) : (
          <span className="text-ink-600">
            <Icon name={icon} size={24} />
          </span>
        )}
      </div>
      <div className="px-2.5 py-2">
        <div className="text-xs font-medium text-slate-300 truncate">{name}</div>
        <div className="text-[10px] text-muted truncate capitalize">{sub}</div>
      </div>
    </button>
  );
}

function HealthRow({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone: "good" | "bad" | "warn" | "neutral";
}) {
  const dot = {
    good: "bg-good",
    bad: "bg-bad",
    warn: "bg-warn",
    neutral: "bg-ink-600",
  }[tone];
  return (
    <div className="flex items-center justify-between py-1.5 border-b border-line/60 last:border-0">
      <div className="flex items-center gap-2 text-sm text-slate-300">
        <span className={`w-2 h-2 rounded-full ${dot}`} />
        {label}
      </div>
      <span className="text-sm tabular-nums text-slate-200">{value.toLocaleString()}</span>
    </div>
  );
}
