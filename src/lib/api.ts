// Typed bridge to the Rust backend.
//
// In Tauri, calls go over IPC via `invoke` and events via `listen`. In a plain
// browser, an in-memory mock backs the same API (including a small fake texture
// store) so the full UI — import, grid, inspector — is demoable without Rust.

import type {
  AppSettings,
  BridgeInfo,
  CollectionDto,
  DuplicateGroup,
  ImportProgress,
  ImportReport,
  LibraryHealth,
  LibraryStats,
  LibraryStatus,
  MaterialDto,
  MaterialMapDto,
  MayaStatus,
  MixedAssets,
  PluginInstallResult,
  SearchResults,
  TagDto,
  TextureDto,
  TextureSetDto,
  UdimInfo,
} from "./types";

const isTauri =
  typeof window !== "undefined" &&
  ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);

/** True when the UI is backed by the real Rust backend. */
export const runningInTauri = isTauri;

export type Unlisten = () => void;

// ===========================================================================
// Browser mock (dev / preview only)
// ===========================================================================
const defaultSettings = (): AppSettings => ({
  library: {
    location: null,
    storage_mode: "managed",
    auto_scan: false,
    scan_frequency_minutes: 30,
  },
  import: {
    auto_detect_maps: true,
    auto_generate_preview: true,
    auto_tag: true,
    auto_group_texture_sets: true,
    copy_files: true,
  },
  appearance: { theme: "dark", grid_size: 200, preview_quality: 2 },
  default_renderer: "generic_pbr",
  updates: { automatic_updates: true, check_on_startup: true, channel: "stable" },
});

let mockSettings = defaultSettings();
const mockTextures: TextureDto[] = [];
const mockMaterials: MaterialDto[] = [];
const doneListeners = new Set<(r: ImportReport) => void>();
const progressListeners = new Set<(p: ImportProgress) => void>();
const libraryChangeListeners = new Set<() => void>();
const materialImportedListeners = new Set<(name: string) => void>();

// Phase 5 mock stores.
let mockTagSeq = 1;
let mockColSeq = 1;
const mockTagsByAsset = new Map<string, { id: number; name: string }[]>();
const mockCollections: { id: number; name: string; icon: string | null; members: Set<string> }[] = [];
const mockUsage: string[] = []; // asset ids, most-recent last
function fireLibraryChanged() {
  libraryChangeListeners.forEach((cb) => cb());
}
function findAsset(id: unknown): { favorite: boolean; name: string } | undefined {
  return (
    mockTextures.find((t) => t.id === id) ?? mockMaterials.find((m) => m.id === id)
  );
}

const MAP_COLORS: Record<string, string> = {
  base_color: "#b5713c",
  roughness: "#8a8a8a",
  metallic: "#c9c9c9",
  normal: "#8080ff",
  height: "#5a5a5a",
  displacement: "#4a4a4a",
  ao: "#6b6b6b",
  opacity: "#dddddd",
  emission: "#e0a030",
  mask: "#909090",
};

function mockThumb(mapType: string | null): string {
  const c = (mapType && MAP_COLORS[mapType]) || "#5c6673";
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='256' height='256'><rect width='256' height='256' fill='${c}'/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function mockDetect(name: string): string | null {
  const n = name.toLowerCase();
  const table: [RegExp, string][] = [
    [/base ?color|albedo|diffuse|_col|_diff/, "base_color"],
    [/rough/, "roughness"],
    [/metal/, "metallic"],
    [/normal|_nrm|_nor/, "normal"],
    [/height|_hgt/, "height"],
    [/disp/, "displacement"],
    [/\bao\b|occlusion/, "ao"],
    [/opacity|alpha/, "opacity"],
    [/emiss|glow/, "emission"],
    [/mask/, "mask"],
  ];
  for (const [re, slug] of table) if (re.test(n)) return slug;
  return null;
}

const EXPECTED_PBR = ["base_color", "roughness", "normal", "height", "ao"];

// Group the mock texture store into sets by base name (first token).
function mockSets(): TextureSetDto[] {
  const groups = new Map<string, TextureDto[]>();
  for (const t of mockTextures) {
    const base = t.name.split(/[_\-.]/)[0].toLowerCase();
    if (!groups.has(base)) groups.set(base, []);
    groups.get(base)!.push(t);
  }
  const out: TextureSetDto[] = [];
  for (const members of groups.values()) {
    const slots = new Map<string, TextureDto>();
    for (const m of members) if (m.map_type && !slots.has(m.map_type)) slots.set(m.map_type, m);
    if (slots.size < 2) continue;
    const present = new Set(slots.keys());
    const maps = [...slots.entries()].map(([slot, tex]) => ({
      slot,
      texture_id: tex.id,
      name: tex.name,
    }));
    out.push({
      id: `NX-SET-${members[0].id.slice(-4)}`,
      name: members[0].name.split(/[_\-.]/)[0],
      resolution: "2K",
      is_pbr:
        present.has("base_color") &&
        present.has("normal") &&
        (present.has("roughness") || present.has("metallic")),
      tileable: null,
      maps,
      missing_maps: EXPECTED_PBR.filter((s) => !present.has(s)),
      member_count: maps.length,
    });
  }
  return out;
}

const CATEGORY_KEYWORDS = [
  "Concrete", "Wood", "Metal", "Stone", "Brick", "Plaster", "Tile", "Fabric",
  "Leather", "Plastic", "Glass", "Rubber", "Ground", "Organic", "Sci-Fi",
];
function mockCategory(name: string): string {
  const n = name.toLowerCase();
  return CATEGORY_KEYWORDS.find((c) => n.includes(c.toLowerCase())) ?? "Other";
}

function makeMockMaterial(name: string, maps: MaterialMapDto[]): MaterialDto {
  const present = new Set(maps.map((m) => m.slot));
  const is_pbr =
    present.has("base_color") &&
    present.has("normal") &&
    (present.has("roughness") || present.has("metallic"));
  const presentExpected = EXPECTED_PBR.filter((s) => present.has(s)).length;
  const health = Math.round((presentExpected / EXPECTED_PBR.length) * 100);
  return {
    id: `NX-MAT-${Math.random().toString(16).slice(2, 6).toUpperCase()}-${Math.random()
      .toString(16)
      .slice(2, 6)
      .toUpperCase()}`,
    name,
    category: mockCategory(name),
    is_pbr,
    tileable: null,
    is_udim: false,
    resolution: "2K",
    health,
    status: health >= 100 ? "healthy" : health === 0 ? "broken" : "incomplete",
    favorite: false,
    maps,
    missing_maps: EXPECTED_PBR.filter((s) => !present.has(s)),
    renderers: ["generic_pbr"],
    preview_texture_id:
      maps.find((m) => m.slot === "base_color")?.texture_id ?? maps[0]?.texture_id ?? null,
  };
}

async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "core_version":
      return "0.1.0" as unknown as T;
    case "get_app_settings":
      return mockSettings as unknown as T;
    case "save_app_settings":
      mockSettings = args?.settings as AppSettings;
      return undefined as unknown as T;
    case "init_library":
      mockSettings.library.location = String(args?.path ?? "");
      mockSettings.library.storage_mode = args?.managed ? "managed" : "referenced";
      return {
        configured: true,
        location: mockSettings.library.location,
        reachable: true,
        storage_mode: mockSettings.library.storage_mode,
      } as unknown as T;
    case "get_library_status":
      return {
        configured: mockSettings.library.location !== null,
        location: mockSettings.library.location,
        reachable: mockSettings.library.location !== null,
        storage_mode: mockSettings.library.storage_mode,
      } as unknown as T;
    case "get_library_stats":
      return {
        materials: 0,
        textures: mockTextures.length,
        texture_sets: 0,
        favorites: mockTextures.filter((t) => t.favorite).length,
        recently_added: mockTextures.length,
      } as unknown as T;
    case "get_library_health":
      return {
        assets: mockTextures.length,
        healthy: mockTextures.length,
        missing_files: 0,
        duplicates: 0,
        incomplete_materials: 0,
        broken_references: 0,
      } as unknown as T;
    case "get_maya_status":
      return { connected: false, version: null, bridge_port: null } as unknown as T;
    case "import_paths": {
      const paths = (args?.paths as string[]) ?? [];
      // Simulate async import with progress + done events.
      const total = paths.length;
      let done = 0;
      const report: ImportReport = { total, imported: 0, duplicates: 0, failed: 0 };
      for (const p of paths) {
        const name = p.split(/[\\/]/).pop() || p;
        progressListeners.forEach((cb) => cb({ done, total, current: name }));
        const mapType = mockDetect(name);
        const id = `NX-TEX-${Math.random().toString(16).slice(2, 6).toUpperCase()}-${Math.random()
          .toString(16)
          .slice(2, 6)
          .toUpperCase()}`;
        mockTextures.unshift({
          id,
          name: name.replace(/\.[^.]+$/, ""),
          map_type: mapType,
          category: mapType ?? "other",
          width: 2048,
          height: 2048,
          format: (name.split(".").pop() || "").toLowerCase(),
          channels: 3,
          color_space: mapType === "base_color" ? "srgb" : "linear",
          file_size: 4_200_000,
          is_udim: /\.\d{4}\./.test(name),
          tileable: null,
          favorite: false,
          managed: mockSettings.library.storage_mode === "managed",
          file_path: p,
          thumbnail_path: null,
          created_at: Math.floor(Date.now() / 1000),
        });
        report.imported += 1;
        done += 1;
      }
      doneListeners.forEach((cb) => cb(report));
      return undefined as unknown as T;
    }
    case "list_textures": {
      const mt = (args?.mapType as string | null) ?? null;
      let list = mockTextures;
      if (mt === "other") list = mockTextures.filter((t) => t.map_type === null);
      else if (mt) list = mockTextures.filter((t) => t.map_type === mt);
      return list as unknown as T;
    }
    case "get_texture":
      return (mockTextures.find((t) => t.id === args?.id) ?? null) as unknown as T;
    case "get_thumbnail": {
      const t = mockTextures.find((x) => x.id === args?.id);
      return (t ? mockThumb(t.map_type) : null) as unknown as T;
    }
    case "list_texture_sets":
      return mockSets() as unknown as T;
    case "get_texture_set":
      return (mockSets().find((s) => s.id === args?.id) ?? null) as unknown as T;
    case "rebuild_texture_sets":
      return mockSets().length as unknown as T;
    case "get_udim_info": {
      const t = mockTextures.find((x) => x.id === args?.id);
      const tiles = t?.is_udim ? [1001, 1002, 1003, 1004] : [];
      return { tiles, missing: [], tile_count: tiles.length } as unknown as T;
    }
    case "import_material": {
      const path = String(args?.path ?? "Material");
      const name = (path.split(/[\\/]/).pop() || "Material").replace(/\.[^.]+$/, "");
      // Reference the current textures (dedup by map type) as the material's maps.
      const slots = new Map<string, TextureDto>();
      for (const t of mockTextures) if (t.map_type && !slots.has(t.map_type)) slots.set(t.map_type, t);
      const maps: MaterialMapDto[] = [...slots.entries()].map(([slot, tex]) => ({
        slot,
        texture_id: tex.id,
        name: tex.name,
      }));
      const mat = makeMockMaterial(name, maps);
      mockMaterials.unshift(mat);
      materialImportedListeners.forEach((cb) => cb(mat.name));
      libraryChangeListeners.forEach((cb) => cb());
      return undefined as unknown as T;
    }
    case "create_material_from_set": {
      const set = mockSets().find((s) => s.id === args?.setId);
      const maps: MaterialMapDto[] = (set?.maps ?? []).map((m) => ({
        slot: m.slot,
        texture_id: m.texture_id,
        name: m.name,
      }));
      const mat = makeMockMaterial(String(args?.name ?? set?.name ?? "Material"), maps);
      mockMaterials.unshift(mat);
      libraryChangeListeners.forEach((cb) => cb());
      return mat.id as unknown as T;
    }
    case "list_materials": {
      const cat = (args?.category as string | null) ?? null;
      return (cat ? mockMaterials.filter((m) => m.category === cat) : mockMaterials) as unknown as T;
    }
    case "get_material":
      return (mockMaterials.find((m) => m.id === args?.id) ?? null) as unknown as T;
    case "search": {
      const q = String(args?.query ?? "").trim().toLowerCase();
      if (!q) return { materials: [], textures: [], sets: [] } as unknown as T;
      const hasTag = (id: string) =>
        (mockTagsByAsset.get(id) ?? []).some((t) => t.name.toLowerCase().includes(q));
      const match = (name: string, id: string) => name.toLowerCase().includes(q) || hasTag(id);
      return {
        materials: mockMaterials.filter((m) => match(m.name, m.id)),
        textures: mockTextures.filter((t) => match(t.name, t.id)),
        sets: mockSets().filter((s) => match(s.name, s.id)),
      } as unknown as T;
    }
    case "set_favorite": {
      const a = findAsset(args?.id);
      if (a) a.favorite = Boolean(args?.favorite);
      fireLibraryChanged();
      return undefined as unknown as T;
    }
    case "list_favorites":
      return {
        materials: mockMaterials.filter((m) => m.favorite),
        textures: mockTextures.filter((t) => t.favorite),
      } as unknown as T;
    case "list_recent_added":
      return {
        materials: mockMaterials.slice(0, 60),
        textures: mockTextures.slice(0, 60),
      } as unknown as T;
    case "list_recent_used": {
      const seen = new Set<string>();
      const ordered = [...mockUsage].reverse().filter((id) => !seen.has(id) && seen.add(id));
      return {
        materials: ordered.map((id) => mockMaterials.find((m) => m.id === id)).filter(Boolean),
        textures: ordered.map((id) => mockTextures.find((t) => t.id === id)).filter(Boolean),
      } as unknown as T;
    }
    case "record_usage":
      mockUsage.push(String(args?.id));
      return undefined as unknown as T;
    case "list_tags": {
      const counts = new Map<string, number>();
      for (const tags of mockTagsByAsset.values())
        for (const t of tags) counts.set(t.name, (counts.get(t.name) ?? 0) + 1);
      return [...counts.entries()]
        .map(([name, count], i) => ({ id: i + 1, name, count }))
        .sort((a, b) => a.name.localeCompare(b.name)) as unknown as T;
    }
    case "tags_for_asset":
      return (mockTagsByAsset.get(String(args?.id)) ?? []).map((t) => ({ ...t, count: 0 })) as unknown as T;
    case "add_tag": {
      const id = String(args?.id);
      const name = String(args?.name).trim().replace(/^#/, "").trim();
      const list = mockTagsByAsset.get(id) ?? [];
      let tag = list.find((t) => t.name.toLowerCase() === name.toLowerCase());
      if (!tag) {
        tag = { id: ++mockTagSeq, name };
        list.push(tag);
        mockTagsByAsset.set(id, list);
      }
      fireLibraryChanged();
      return { ...tag, count: 0 } as unknown as T;
    }
    case "remove_tag": {
      const id = String(args?.id);
      const list = (mockTagsByAsset.get(id) ?? []).filter((t) => t.id !== args?.tagId);
      mockTagsByAsset.set(id, list);
      fireLibraryChanged();
      return undefined as unknown as T;
    }
    case "create_collection": {
      const col = {
        id: ++mockColSeq,
        name: String(args?.name),
        icon: (args?.icon as string | null) ?? null,
        members: new Set<string>(),
      };
      mockCollections.push(col);
      fireLibraryChanged();
      return { id: col.id, name: col.name, icon: col.icon, count: 0 } as unknown as T;
    }
    case "list_collections":
      return mockCollections.map((c) => ({
        id: c.id,
        name: c.name,
        icon: c.icon,
        count: c.members.size,
      })) as unknown as T;
    case "delete_collection": {
      const i = mockCollections.findIndex((c) => c.id === args?.id);
      if (i >= 0) mockCollections.splice(i, 1);
      fireLibraryChanged();
      return undefined as unknown as T;
    }
    case "add_to_collection": {
      mockCollections.find((c) => c.id === args?.collectionId)?.members.add(String(args?.assetId));
      fireLibraryChanged();
      return undefined as unknown as T;
    }
    case "remove_from_collection": {
      mockCollections
        .find((c) => c.id === args?.collectionId)
        ?.members.delete(String(args?.assetId));
      fireLibraryChanged();
      return undefined as unknown as T;
    }
    case "collection_members": {
      const col = mockCollections.find((c) => c.id === args?.collectionId);
      const ids = col?.members ?? new Set<string>();
      return {
        materials: mockMaterials.filter((m) => ids.has(m.id)),
        textures: mockTextures.filter((t) => ids.has(t.id)),
      } as unknown as T;
    }
    case "list_duplicates":
      return [] as unknown as T;
    case "send_to_maya":
      return undefined as unknown as T;
    case "get_bridge_info":
      return {
        port: 48757,
        token: "preview-mode-no-token",
        connected: false,
        maya_version: null,
      } as unknown as T;
    case "install_maya_plugin":
      return {
        installed: ["Maya 2026", "Maya 2027"],
        skipped: [],
      } as unknown as T;
    case "remove_asset": {
      const id = String(args?.id);
      const ti = mockTextures.findIndex((t) => t.id === id);
      if (ti >= 0) mockTextures.splice(ti, 1);
      const mi = mockMaterials.findIndex((m) => m.id === id);
      if (mi >= 0) mockMaterials.splice(mi, 1);
      mockTagsByAsset.delete(id);
      for (const c of mockCollections) c.members.delete(id);
      fireLibraryChanged();
      return undefined as unknown as T;
    }
    default:
      throw new Error(`mockInvoke: unhandled command '${cmd}'`);
  }
}

// ===========================================================================
// Invoke dispatcher
// ===========================================================================
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockInvoke<T>(cmd, args);
}

// ===========================================================================
// Typed API surface
// ===========================================================================
export const api = {
  coreVersion: () => call<string>("core_version"),
  getSettings: () => call<AppSettings>("get_app_settings"),
  saveSettings: (settings: AppSettings) => call<void>("save_app_settings", { settings }),
  initLibrary: (path: string, managed: boolean) =>
    call<LibraryStatus>("init_library", { path, managed }),
  getLibraryStatus: () => call<LibraryStatus>("get_library_status"),
  getLibraryStats: () => call<LibraryStats>("get_library_stats"),
  getLibraryHealth: () => call<LibraryHealth>("get_library_health"),
  getMayaStatus: () => call<MayaStatus>("get_maya_status"),

  // Phase 2
  importPaths: (paths: string[]) => call<void>("import_paths", { paths }),
  listTextures: (mapType?: string | null) =>
    call<TextureDto[]>("list_textures", { mapType: mapType ?? null }),
  getTexture: (id: string) => call<TextureDto | null>("get_texture", { id }),
  getThumbnail: (id: string) => call<string | null>("get_thumbnail", { id }),

  // Phase 3
  listTextureSets: () => call<TextureSetDto[]>("list_texture_sets"),
  getTextureSet: (id: string) => call<TextureSetDto | null>("get_texture_set", { id }),
  rebuildTextureSets: () => call<number>("rebuild_texture_sets"),
  getUdimInfo: (id: string) => call<UdimInfo>("get_udim_info", { id }),

  // Phase 4
  importMaterial: (path: string) => call<void>("import_material", { path }),
  createMaterialFromSet: (setId: string, name?: string) =>
    call<string>("create_material_from_set", { setId, name: name ?? null }),
  listMaterials: (category?: string | null) =>
    call<MaterialDto[]>("list_materials", { category: category ?? null }),
  getMaterial: (id: string) => call<MaterialDto | null>("get_material", { id }),

  // Phase 5
  search: (query: string) => call<SearchResults>("search", { query }),
  setFavorite: (id: string, favorite: boolean) => call<void>("set_favorite", { id, favorite }),
  listFavorites: () => call<MixedAssets>("list_favorites"),
  listRecentAdded: () => call<MixedAssets>("list_recent_added"),
  listRecentUsed: () => call<MixedAssets>("list_recent_used"),
  recordUsage: (id: string, action = "viewed") => call<void>("record_usage", { id, action }),
  listTags: () => call<TagDto[]>("list_tags"),
  tagsForAsset: (id: string) => call<TagDto[]>("tags_for_asset", { id }),
  addTag: (id: string, name: string) => call<TagDto>("add_tag", { id, name }),
  removeTag: (id: string, tagId: number) => call<void>("remove_tag", { id, tagId }),
  createCollection: (name: string, icon?: string | null) =>
    call<CollectionDto>("create_collection", { name, icon: icon ?? null }),
  listCollections: () => call<CollectionDto[]>("list_collections"),
  deleteCollection: (id: number) => call<void>("delete_collection", { id }),
  addToCollection: (collectionId: number, assetId: string) =>
    call<void>("add_to_collection", { collectionId, assetId }),
  removeFromCollection: (collectionId: number, assetId: string) =>
    call<void>("remove_from_collection", { collectionId, assetId }),
  collectionMembers: (collectionId: number) =>
    call<MixedAssets>("collection_members", { collectionId }),
  listDuplicates: () => call<DuplicateGroup[]>("list_duplicates"),
  removeAsset: (id: string) => call<void>("remove_asset", { id }),

  // Phase 7 — Maya bridge
  sendToMaya: (id: string, kind: "material" | "texture") =>
    call<void>("send_to_maya", { id, kind }),
  getBridgeInfo: () => call<BridgeInfo>("get_bridge_info"),
  installMayaPlugin: () => call<PluginInstallResult>("install_maya_plugin"),
};

// ===========================================================================
// Events (import progress) and drag-drop
// ===========================================================================
export async function onImportProgress(cb: (p: ImportProgress) => void): Promise<Unlisten> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<ImportProgress>("import:progress", (e) => cb(e.payload));
  }
  progressListeners.add(cb);
  return () => progressListeners.delete(cb);
}

export async function onImportDone(cb: (r: ImportReport) => void): Promise<Unlisten> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<ImportReport>("import:done", (e) => cb(e.payload));
  }
  doneListeners.add(cb);
  return () => doneListeners.delete(cb);
}

export async function onLibraryChanged(cb: () => void): Promise<Unlisten> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen("library:changed", () => cb());
  }
  libraryChangeListeners.add(cb);
  return () => libraryChangeListeners.delete(cb);
}

export async function onMaterialImported(cb: (name: string) => void): Promise<Unlisten> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<string>("material:imported", (e) => cb(e.payload));
  }
  materialImportedListeners.add(cb);
  return () => materialImportedListeners.delete(cb);
}

/** Subscribe to OS file drops onto the window. Returns an unlisten function. */
export async function onFileDrop(cb: (paths: string[]) => void): Promise<Unlisten> {
  if (isTauri) {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    return getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") cb(event.payload.paths);
    });
  }
  return () => {};
}

// ===========================================================================
// File pickers
// ===========================================================================
const IMAGE_EXTS = ["jpg", "jpeg", "png", "tif", "tiff", "tga", "bmp", "exr", "hdr", "webp", "tx"];

/** Pick one or more texture files. */
export async function pickFiles(): Promise<string[]> {
  if (isTauri) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const result = await open({
      multiple: true,
      directory: false,
      filters: [{ name: "Textures", extensions: IMAGE_EXTS }],
    });
    if (!result) return [];
    return Array.isArray(result) ? result : [result];
  }
  // Browser preview: fabricate a realistic sample set.
  return [
    "Concrete_BaseColor_4K.jpg",
    "Concrete_Roughness_4K.jpg",
    "Concrete_Normal_4K.jpg",
    "Concrete_Height_4K.exr",
    "Concrete_AO_4K.jpg",
  ];
}

// ===========================================================================
// Opener / clipboard (inspector actions, spec §23)
// ===========================================================================

/** Reveal a file in the OS file manager (Explorer/Finder). */
export async function revealInExplorer(path: string): Promise<void> {
  if (isTauri) {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(path);
  } else {
    console.info("[preview] reveal:", path);
  }
}

/** Open a path with the OS default handler (a folder opens the file manager). */
export async function openPath(path: string): Promise<void> {
  if (isTauri) {
    const { openPath: open } = await import("@tauri-apps/plugin-opener");
    await open(path);
  } else {
    console.info("[preview] open:", path);
  }
}

/** Directory portion of a path (handles both separators). */
export function dirname(path: string): string {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx > 0 ? path.slice(0, idx) : path;
}

/** Copy text to the clipboard. */
export async function copyToClipboard(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    console.info("[clipboard] copy:", text);
  }
}

/** Pick a folder (import recursively / library location). */
export async function pickFolder(): Promise<string | null> {
  if (isTauri) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const result = await open({ directory: true, multiple: false });
    return typeof result === "string" ? result : null;
  }
  const entered = window.prompt("Folder path (e.g. D:\\NEXORA_LIBRARY)");
  return entered && entered.trim() ? entered.trim() : null;
}
