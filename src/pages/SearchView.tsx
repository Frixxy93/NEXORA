import { useEffect, useState } from "react";
import { api, onLibraryChanged } from "../lib/api";
import type { MaterialDto, SearchResults, TextureDto, TextureSetDto, UdimInfo } from "../lib/types";
import { TextureCard } from "../components/TextureCard";
import { TextureInspector } from "../components/TextureInspector";
import { TextureSetCard } from "../components/TextureSetCard";
import { TextureSetInspector } from "../components/TextureSetInspector";
import { MaterialCard } from "../components/MaterialCard";
import { MaterialInspector } from "../components/MaterialInspector";
import { EmptyState } from "../components/EmptyState";

type Selection =
  | { kind: "texture"; id: string }
  | { kind: "set"; id: string }
  | { kind: "material"; id: string }
  | null;

// Global search results (spec §17), rendered when the top-bar query is non-empty.
export function SearchView({ query, onNavigate }: { query: string; onNavigate: (v: string) => void }) {
  const [res, setRes] = useState<SearchResults>({ materials: [], textures: [], sets: [] });
  const [selected, setSelected] = useState<Selection>(null);
  const [udim, setUdim] = useState<UdimInfo | null>(null);

  useEffect(() => {
    const run = () => api.search(query).then(setRes).catch(console.error);
    const h = window.setTimeout(run, 150); // debounce
    let unlisten = () => {};
    onLibraryChanged(run).then((u) => (unlisten = u));
    return () => {
      window.clearTimeout(h);
      unlisten();
    };
  }, [query]);

  const selectTexture = (t: TextureDto) => {
    setSelected({ kind: "texture", id: t.id });
    api.recordUsage(t.id).catch(() => {});
    setUdim(null);
    if (t.is_udim) api.getUdimInfo(t.id).then(setUdim).catch(() => setUdim(null));
  };
  const selectMaterial = (m: MaterialDto) => {
    setSelected({ kind: "material", id: m.id });
    api.recordUsage(m.id).catch(() => {});
  };
  const createMaterialFromSet = async (set: TextureSetDto) => {
    await api.createMaterialFromSet(set.id);
    onNavigate("lib.materials");
  };

  const total = res.materials.length + res.textures.length + res.sets.length;
  const selMat = selected?.kind === "material" ? res.materials.find((m) => m.id === selected.id) : null;
  const selSet = selected?.kind === "set" ? res.sets.find((s) => s.id === selected.id) : null;
  const selTex = selected?.kind === "texture" ? res.textures.find((t) => t.id === selected.id) : null;

  return (
    <div className="flex-1 flex min-h-0">
      <div className="flex-1 flex flex-col min-w-0">
        <div className="h-12 shrink-0 border-b border-line flex items-center px-6">
          <div className="text-sm text-slate-200">
            {total} result{total === 1 ? "" : "s"} for <span className="text-white font-semibold">“{query}”</span>
          </div>
        </div>

        {total === 0 ? (
          <EmptyState icon="search" title="No matches" hint="Try a different name, category, or tag." />
        ) : (
          <div className="flex-1 overflow-y-auto p-4 space-y-5">
            {res.materials.length > 0 && (
              <Section label="Materials">
                {res.materials.map((m) => (
                  <MaterialCard
                    key={m.id}
                    material={m}
                    selected={selected?.kind === "material" && selected.id === m.id}
                    onSelect={() => selectMaterial(m)}
                  />
                ))}
              </Section>
            )}
            {res.sets.length > 0 && (
              <Section label="Texture Sets">
                {res.sets.map((s) => (
                  <TextureSetCard
                    key={s.id}
                    set={s}
                    selected={selected?.kind === "set" && selected.id === s.id}
                    onSelect={() => setSelected({ kind: "set", id: s.id })}
                  />
                ))}
              </Section>
            )}
            {res.textures.length > 0 && (
              <Section label="Textures">
                {res.textures.map((t) => (
                  <TextureCard
                    key={t.id}
                    texture={t}
                    selected={selected?.kind === "texture" && selected.id === t.id}
                    onSelect={() => selectTexture(t)}
                  />
                ))}
              </Section>
            )}
          </div>
        )}
      </div>

      <aside className="w-72 shrink-0 border-l border-line bg-ink-850 hidden xl:flex flex-col">
        <div className="h-12 border-b border-line flex items-center px-4 text-sm font-semibold text-slate-200">
          Inspector
        </div>
        <div className="flex-1 min-h-0">
          {selMat ? (
            <MaterialInspector material={selMat} />
          ) : selSet ? (
            <TextureSetInspector set={selSet} onCreateMaterial={createMaterialFromSet} />
          ) : selTex ? (
            <TextureInspector texture={selTex} udim={udim} />
          ) : (
            <div className="h-full flex items-center justify-center text-center px-6">
              <p className="text-xs text-muted">Select a result to inspect it.</p>
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-[11px] font-semibold uppercase tracking-widest text-muted mb-2">
        {label}
      </div>
      <div className="grid gap-3" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(150px, 1fr))" }}>
        {children}
      </div>
    </div>
  );
}
