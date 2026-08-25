import { useCallback, useEffect, useRef, useState } from "react";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";
import { TextureCard } from "../components/TextureCard";
import { TextureInspector } from "../components/TextureInspector";
import { TextureSetCard } from "../components/TextureSetCard";
import { TextureSetInspector } from "../components/TextureSetInspector";
import { MaterialCard } from "../components/MaterialCard";
import { MaterialInspector } from "../components/MaterialInspector";
import { VIEW_META, type View } from "../lib/nav";
import { api, onImportDone, onLibraryChanged, pickFiles, pickFolder } from "../lib/api";
import type {
  CollectionDto,
  DuplicateGroup,
  MaterialDto,
  MixedAssets,
  TextureDto,
  TextureSetDto,
  UdimInfo,
} from "../lib/types";

type Kind =
  | { t: "library" }
  | { t: "textures"; slug: string }
  | { t: "udim" }
  | { t: "materials" }
  | { t: "materials-pbr" }
  | { t: "materials-renderer"; renderer: string }
  | { t: "mixed"; source: "favorites" | "recent_added" | "recent_used" }
  | { t: "duplicates" }
  | { t: "collections" }
  | null;

function viewKind(view: View): Kind {
  if (view === "lib.textures") return { t: "library" };
  if (view === "mtype.udim") return { t: "udim" };
  if (view.startsWith("ttype.")) return { t: "textures", slug: view.slice("ttype.".length) };
  if (view === "lib.materials") return { t: "materials" };
  if (view === "mtype.pbr") return { t: "materials-pbr" };
  if (view === "mtype.vray") return { t: "materials-renderer", renderer: "vray" };
  if (view === "mtype.arnold") return { t: "materials-renderer", renderer: "arnold" };
  if (view === "smart.favorites") return { t: "mixed", source: "favorites" };
  if (view === "smart.recent_added") return { t: "mixed", source: "recent_added" };
  if (view === "smart.recent_used") return { t: "mixed", source: "recent_used" };
  if (view === "smart.duplicates") return { t: "duplicates" };
  if (view === "collections") return { t: "collections" };
  return null;
}

type Selection =
  | { kind: "texture"; id: string }
  | { kind: "set"; id: string }
  | { kind: "material"; id: string }
  | null;

// How many cards to mount initially and reveal per scroll step. Large libraries
// (thousands of assets from Discover) would otherwise mount every card — and its
// IntersectionObserver — at once. We window the render instead: only the first
// `REVEAL_STEP` cards mount, and more reveal as the user nears the bottom.
const REVEAL_STEP = 300;

// One render descriptor per grid cell, so materials/sets/textures window together.
type Cell =
  | { t: "material"; m: MaterialDto }
  | { t: "set"; s: TextureSetDto }
  | { t: "texture"; x: TextureDto };

export function Library({ view, onNavigate }: { view: View; onNavigate: (v: View) => void }) {
  const meta = VIEW_META[view] ?? { title: view, subtitle: "" };
  const kind = viewKind(view);
  const isMaterialView = !!kind && kind.t.startsWith("materials");

  const [mode, setMode] = useState<"textures" | "sets">("textures");
  const [textures, setTextures] = useState<TextureDto[]>([]);
  const [sets, setSets] = useState<TextureSetDto[]>([]);
  const [materials, setMaterials] = useState<MaterialDto[]>([]);
  const [mixed, setMixed] = useState<MixedAssets>({ materials: [], textures: [] });
  const [duplicates, setDuplicates] = useState<DuplicateGroup[]>([]);
  const [collections, setCollections] = useState<CollectionDto[]>([]);
  const [activeCollection, setActiveCollection] = useState<CollectionDto | null>(null);
  const [selected, setSelected] = useState<Selection>(null);
  const [udim, setUdim] = useState<UdimInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [visibleCount, setVisibleCount] = useState(REVEAL_STEP);
  const [gridSize, setGridSize] = useState(200);
  const gridScrollRef = useRef<HTMLDivElement>(null);

  // Card size comes from Settings ▸ Appearance (reloaded on remount per view).
  useEffect(() => {
    api.getSettings().then((s) => setGridSize(s.appearance.grid_size)).catch(() => {});
  }, []);

  // Reset the window (and scroll to top) whenever the displayed set changes.
  useEffect(() => {
    setVisibleCount(REVEAL_STEP);
    if (gridScrollRef.current) gridScrollRef.current.scrollTop = 0;
  }, [view, mode, activeCollection]);

  const showingSets = kind?.t === "library" && mode === "sets";
  const inCollection = kind?.t === "collections" && activeCollection !== null;

  const load = useCallback(() => {
    if (!kind) return;
    setLoading(true);
    const done = () => setLoading(false);
    if (kind.t === "collections") {
      if (activeCollection) {
        api.collectionMembers(activeCollection.id).then(setMixed).catch(console.error).finally(done);
      } else {
        api.listCollections().then(setCollections).catch(console.error).finally(done);
      }
    } else if (kind.t === "mixed") {
      const p =
        kind.source === "favorites"
          ? api.listFavorites()
          : kind.source === "recent_added"
          ? api.listRecentAdded()
          : api.listRecentUsed();
      p.then(setMixed).catch(console.error).finally(done);
    } else if (kind.t === "duplicates") {
      api.listDuplicates().then(setDuplicates).catch(console.error).finally(done);
    } else if (isMaterialView) {
      api
        .listMaterials(null)
        .then((list) => {
          if (kind.t === "materials-pbr") setMaterials(list.filter((m) => m.is_pbr));
          else if (kind.t === "materials-renderer")
            setMaterials(list.filter((m) => m.renderers.includes(kind.renderer)));
          else setMaterials(list);
        })
        .catch(console.error)
        .finally(done);
    } else if (showingSets) {
      api.listTextureSets().then(setSets).catch(console.error).finally(done);
    } else {
      const slug = kind.t === "textures" ? kind.slug : null;
      api
        .listTextures(slug)
        .then((list) => setTextures(kind.t === "udim" ? list.filter((t) => t.is_udim) : list))
        .catch(console.error)
        .finally(done);
    }
  }, [kind?.t, showingSets, isMaterialView, view, activeCollection]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    onImportDone(() => load()).then((u) => unlisteners.push(u));
    onLibraryChanged(() => load()).then((u) => unlisteners.push(u));
    return () => unlisteners.forEach((u) => u());
  }, [load]);

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

  const addFiles = async () => {
    const paths = await pickFiles();
    if (paths.length) await api.importPaths(paths);
  };
  const addFolder = async () => {
    const folder = await pickFolder();
    if (folder) await api.importPaths([folder]);
  };
  const addMaterial = async () => {
    const folder = await pickFolder();
    if (folder) await api.importMaterial(folder);
  };
  const createMaterialFromSet = async (set: TextureSetDto) => {
    await api.createMaterialFromSet(set.id);
    onNavigate("lib.materials");
  };

  if (!kind) {
    return (
      <Shell title={meta.title} subtitle={meta.subtitle}>
        <EmptyState icon="grid" title={`${meta.title} arrive in a later phase`} hint="" />
      </Shell>
    );
  }

  // Collections landing (no collection selected): show collection cards.
  if (kind.t === "collections" && !activeCollection) {
    return (
      <Shell
        title="Collections"
        subtitle="Virtual groups of assets"
        actions={<NewCollectionButton onCreated={load} />}
      >
        {collections.length === 0 ? (
          <EmptyState
            icon="folder"
            title="No collections yet"
            hint="Create a collection and add materials or textures to it from their inspector. Collections are virtual — no files are duplicated."
          />
        ) : (
          <GridWrap>
            {collections.map((c) => (
              <button
                key={c.id}
                onClick={() => {
                  setActiveCollection(c);
                  setSelected(null);
                }}
                className="rounded-lg bg-ink-800 border border-line hover:border-ink-600 p-4 text-left flex flex-col gap-2"
              >
                <div className="text-2xl">{c.icon ?? "📁"}</div>
                <div className="text-sm font-medium text-slate-200 truncate">{c.name}</div>
                <div className="text-[11px] text-muted">{c.count} items</div>
              </button>
            ))}
          </GridWrap>
        )}
      </Shell>
    );
  }

  // Which arrays are currently displayed (drives selection + empty state).
  const usingMixed = kind.t === "mixed" || inCollection;
  const curMaterials = usingMixed ? mixed.materials : isMaterialView ? materials : [];
  const dupTextures = duplicates.flatMap((g) => g.textures);
  const curTextures =
    kind.t === "duplicates"
      ? dupTextures
      : usingMixed
      ? mixed.textures
      : !isMaterialView && !showingSets
      ? textures
      : [];
  const curSets = showingSets ? sets : [];

  // Flatten the three arrays into one windowed cell list (only one or two are
  // ever non-empty for a given view).
  const cells: Cell[] = [
    ...curMaterials.map((m) => ({ t: "material", m }) as Cell),
    ...curSets.map((s) => ({ t: "set", s }) as Cell),
    ...curTextures.map((x) => ({ t: "texture", x }) as Cell),
  ];
  const revealed = cells.slice(0, visibleCount);
  const onGridScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 800) {
      setVisibleCount((c) => (c < cells.length ? c + REVEAL_STEP : c));
    }
  };

  const selectedTexture =
    selected?.kind === "texture" ? curTextures.find((t) => t.id === selected.id) ?? null : null;
  const selectedSet =
    selected?.kind === "set" ? curSets.find((s) => s.id === selected.id) ?? null : null;
  const selectedMaterial =
    selected?.kind === "material" ? curMaterials.find((m) => m.id === selected.id) ?? null : null;

  const empty =
    curMaterials.length + curTextures.length + curSets.length === 0 && kind.t !== "duplicates";

  const headerActions = (
    <>
      {kind.t === "library" && (
        <div className="flex items-center gap-0.5 bg-ink-800 border border-line rounded-lg p-0.5 mr-1">
          {(["textures", "sets"] as const).map((m) => (
            <button
              key={m}
              onClick={() => {
                setMode(m);
                setSelected(null);
              }}
              className={`px-2.5 py-1 rounded text-xs capitalize ${
                mode === m ? "bg-ink-700 text-white" : "text-muted"
              }`}
            >
              {m}
            </button>
          ))}
        </div>
      )}
      {isMaterialView && (
        <button className="btn-ghost text-xs flex items-center gap-1.5" onClick={addMaterial}>
          <Icon name="folder" size={14} /> Add Material
        </button>
      )}
      {(kind.t === "library" || kind.t === "textures" || kind.t === "udim") && (
        <>
          <button className="btn-ghost text-xs flex items-center gap-1.5" onClick={addFiles}>
            <Icon name="plus" size={14} /> Add Textures
          </button>
          <button className="btn-ghost text-xs flex items-center gap-1.5" onClick={addFolder}>
            <Icon name="folder" size={14} /> Add Folder
          </button>
        </>
      )}
    </>
  );

  const title = inCollection ? activeCollection!.name : meta.title;
  const subtitle = inCollection ? "Collection" : meta.subtitle;

  return (
    <div className="flex-1 flex min-h-0">
      <div className="flex-1 flex flex-col min-w-0">
        <div className="h-12 shrink-0 border-b border-line flex items-center gap-3 px-6">
          {inCollection && (
            <button
              className="text-muted hover:text-slate-200"
              onClick={() => {
                setActiveCollection(null);
                setSelected(null);
              }}
              title="Back to collections"
            >
              <Icon name="chevron" size={16} className="rotate-180" />
            </button>
          )}
          <div className="min-w-0">
            <div className="text-sm font-semibold text-white truncate">{title}</div>
            <div className="text-[11px] text-muted truncate">{subtitle}</div>
          </div>
          <div className="ml-auto flex items-center gap-2">{headerActions}</div>
        </div>

        {kind.t === "duplicates" ? (
          <DuplicatesView
            groups={duplicates}
            loading={loading}
            selectedId={selected?.kind === "texture" ? selected.id : null}
            onSelect={selectTexture}
          />
        ) : empty ? (
          loading ? (
            <div className="flex-1 flex items-center justify-center text-muted text-sm">Loading…</div>
          ) : (
            <EmptyState
              icon="plus"
              title={emptyTitle(kind, title)}
              hint={emptyHint(kind)}
              action={
                kind.t === "library" || kind.t === "textures" ? (
                  <button className="btn-primary" onClick={addFiles}>
                    Add Textures
                  </button>
                ) : isMaterialView ? (
                  <button className="btn-primary" onClick={addMaterial}>
                    Add Material
                  </button>
                ) : undefined
              }
            />
          )
        ) : (
          <div ref={gridScrollRef} onScroll={onGridScroll} className="flex-1 overflow-y-auto p-4">
            <div
              className="grid gap-3"
              style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${gridSize}px, 1fr))` }}
            >
              {revealed.map((cell) =>
                cell.t === "material" ? (
                  <MaterialCard
                    key={cell.m.id}
                    material={cell.m}
                    selected={selected?.kind === "material" && selected.id === cell.m.id}
                    onSelect={() => selectMaterial(cell.m)}
                  />
                ) : cell.t === "set" ? (
                  <TextureSetCard
                    key={cell.s.id}
                    set={cell.s}
                    selected={selected?.kind === "set" && selected.id === cell.s.id}
                    onSelect={() => setSelected({ kind: "set", id: cell.s.id })}
                  />
                ) : (
                  <TextureCard
                    key={cell.x.id}
                    texture={cell.x}
                    selected={selected?.kind === "texture" && selected.id === cell.x.id}
                    onSelect={() => selectTexture(cell.x)}
                  />
                ),
              )}
            </div>
            {visibleCount < cells.length && (
              <div className="text-center text-[11px] text-muted py-4">
                Showing {revealed.length} of {cells.length} — scroll for more
              </div>
            )}
          </div>
        )}
      </div>

      <Inspector>
        {selectedMaterial ? (
          <MaterialInspector material={selectedMaterial} />
        ) : selectedSet ? (
          <TextureSetInspector set={selectedSet} onCreateMaterial={createMaterialFromSet} />
        ) : selectedTexture ? (
          <TextureInspector texture={selectedTexture} udim={udim} />
        ) : (
          <Placeholder />
        )}
      </Inspector>
    </div>
  );
}

function emptyTitle(kind: NonNullable<Kind>, title: string): string {
  if (kind.t === "mixed" && kind.source === "favorites") return "No favorites yet";
  if (kind.t === "mixed" && kind.source === "recent_used") return "Nothing used yet";
  return `No ${title.toLowerCase()} yet`;
}
function emptyHint(kind: NonNullable<Kind>): string {
  switch (kind.t) {
    case "mixed":
      if (kind.source === "favorites") return "Star a texture or material to see it here.";
      if (kind.source === "recent_used") return "Assets you open or send to Maya will appear here.";
      return "Newly imported assets show up here.";
    case "udim":
      return "Import a UDIM sequence (body.1001.exr, …) and NEXORA collapses the tiles and flags gaps.";
    case "materials":
    case "materials-pbr":
    case "materials-renderer":
      return "Import a folder of maps as a material, or open a texture set and choose “Create Material”.";
    default:
      return "Drag files or a folder anywhere in the window, or use Add Textures.";
  }
}

function DuplicatesView({
  groups,
  loading,
  selectedId,
  onSelect,
}: {
  groups: DuplicateGroup[];
  loading: boolean;
  selectedId: string | null;
  onSelect: (t: TextureDto) => void;
}) {
  if (groups.length === 0) {
    return loading ? (
      <div className="flex-1 flex items-center justify-center text-muted text-sm">Loading…</div>
    ) : (
      <EmptyState
        icon="copy"
        title="No duplicates found"
        hint="NEXORA hashes every texture's content on import; files with identical content are grouped here. Nothing is ever deleted automatically."
      />
    );
  }
  return (
    <div className="flex-1 overflow-y-auto p-4 space-y-5">
      {groups.map((g) => (
        <div key={g.hash}>
          <div className="text-[11px] text-muted mb-2 font-mono">
            {g.textures.length} identical · {g.hash.slice(0, 12)}…
          </div>
          <div
            className="grid gap-3"
            style={{ gridTemplateColumns: "repeat(auto-fill, minmax(150px, 1fr))" }}
          >
            {g.textures.map((t) => (
              <TextureCard
                key={t.id}
                texture={t}
                selected={selectedId === t.id}
                onSelect={() => onSelect(t)}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function NewCollectionButton({ onCreated }: { onCreated: () => void }) {
  const [name, setName] = useState("");
  const [open, setOpen] = useState(false);
  const create = async () => {
    if (!name.trim()) return;
    await api.createCollection(name.trim());
    setName("");
    setOpen(false);
    onCreated();
  };
  return (
    <div className="relative">
      <button className="btn-ghost text-xs flex items-center gap-1.5" onClick={() => setOpen((o) => !o)}>
        <Icon name="plus" size={14} /> New Collection
      </button>
      {open && (
        <div className="absolute top-full mt-1 right-0 z-10 panel p-2 w-56 shadow-xl flex gap-1.5">
          <input
            className="input text-xs py-1"
            placeholder="Collection name"
            value={name}
            autoFocus
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && create()}
          />
          <button className="btn-primary text-xs px-2" onClick={create}>
            Add
          </button>
        </div>
      )}
    </div>
  );
}

function Shell({
  title,
  subtitle,
  actions,
  children,
}: {
  title: string;
  subtitle: string;
  actions?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex-1 flex min-h-0">
      <div className="flex-1 flex flex-col min-w-0">
        <div className="h-12 shrink-0 border-b border-line flex items-center gap-3 px-6">
          <div className="min-w-0">
            <div className="text-sm font-semibold text-white truncate">{title}</div>
            <div className="text-[11px] text-muted truncate">{subtitle}</div>
          </div>
          <div className="ml-auto flex items-center gap-2">{actions}</div>
        </div>
        {children}
      </div>
      <Inspector>
        <Placeholder />
      </Inspector>
    </div>
  );
}

function GridWrap({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex-1 overflow-y-auto p-4">
      <div className="grid gap-3" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(160px, 1fr))" }}>
        {children}
      </div>
    </div>
  );
}

function Inspector({ children }: { children: React.ReactNode }) {
  return (
    <aside className="w-72 shrink-0 border-l border-line bg-ink-850 hidden xl:flex flex-col">
      <div className="h-12 border-b border-line flex items-center px-4 text-sm font-semibold text-slate-200">
        Inspector
      </div>
      <div className="flex-1 min-h-0">{children}</div>
    </aside>
  );
}

function Placeholder() {
  return (
    <div className="h-full flex items-center justify-center text-center px-6">
      <p className="text-xs text-muted">
        Select an item to see its maps, metadata, tags, favorites, and collections.
      </p>
    </div>
  );
}
