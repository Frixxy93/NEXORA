// Mirror of the serde types in `nexora-core`. Keep these in step with
// core/src/settings.rs and core/src/models.rs.

export type StorageMode = "managed" | "referenced";
export type ThemeMode = "dark" | "light" | "system";
export type Renderer = "generic_pbr" | "vray" | "arnold";

export interface LibrarySettings {
  location: string | null;
  storage_mode: StorageMode;
  auto_scan: boolean;
  scan_frequency_minutes: number;
}

export interface ImportSettings {
  auto_detect_maps: boolean;
  auto_generate_preview: boolean;
  auto_tag: boolean;
  auto_group_texture_sets: boolean;
  copy_files: boolean;
}

export interface AppearanceSettings {
  theme: ThemeMode;
  grid_size: number;
  preview_quality: number;
}

export interface UpdateSettings {
  automatic_updates: boolean;
  check_on_startup: boolean;
  channel: string;
}

export interface DiscoverSettings {
  auto_sync: boolean;
  resolution: string; // "1k" | "2k" | "4k"
  source_polyhaven: boolean;
}

export interface AppSettings {
  library: LibrarySettings;
  import: ImportSettings;
  appearance: AppearanceSettings;
  default_renderer: Renderer;
  updates: UpdateSettings;
  discover: DiscoverSettings;
}

export interface SyncProgress {
  running: boolean;
  total: number;
  done: number;
  imported: number;
  skipped: number;
  failed: number;
  current: string;
  bytes: number;
  finished: boolean;
  error: string | null;
}

export interface DiscoverStatus {
  running: boolean;
  synced: number;
  progress: SyncProgress;
}

export interface PluginInstallResult {
  installed: string[];
  skipped: string[];
}

export interface LibraryStats {
  materials: number;
  textures: number;
  texture_sets: number;
  favorites: number;
  recently_added: number;
}

export interface LibraryStatus {
  configured: boolean;
  location: string | null;
  reachable: boolean;
  storage_mode: string;
}

export interface LibraryHealth {
  assets: number;
  healthy: number;
  missing_files: number;
  duplicates: number;
  incomplete_materials: number;
  broken_references: number;
}

export interface MayaStatus {
  connected: boolean;
  version: string | null;
  bridge_port: number | null;
}

// --- Phase 2: textures ---------------------------------------------------

export interface TextureDto {
  id: string;
  name: string;
  map_type: string | null;
  category: string | null;
  width: number | null;
  height: number | null;
  format: string | null;
  channels: number | null;
  color_space: string | null;
  file_size: number | null;
  is_udim: boolean;
  tileable: boolean | null;
  favorite: boolean;
  managed: boolean;
  file_path: string;
  thumbnail_path: string | null;
  created_at: number;
}

export interface ImportReport {
  total: number;
  imported: number;
  duplicates: number;
  failed: number;
}

export interface ImportProgress {
  done: number;
  total: number;
  current: string;
}

// --- Phase 3: texture sets & UDIM ---------------------------------------

export interface TextureSetMap {
  slot: string;
  texture_id: string;
  name: string;
}

export interface TextureSetDto {
  id: string;
  name: string;
  resolution: string | null;
  is_pbr: boolean;
  tileable: boolean | null;
  maps: TextureSetMap[];
  missing_maps: string[];
  member_count: number;
}

export interface UdimInfo {
  tiles: number[];
  missing: number[];
  tile_count: number;
}

// --- Phase 4: materials --------------------------------------------------

export interface MaterialMapDto {
  slot: string;
  texture_id: string;
  name: string;
}

export interface MaterialDto {
  id: string;
  name: string;
  category: string | null;
  is_pbr: boolean;
  tileable: boolean | null;
  is_udim: boolean;
  resolution: string | null;
  health: number;
  status: string;
  favorite: boolean;
  maps: MaterialMapDto[];
  missing_maps: string[];
  renderers: string[];
  preview_texture_id: string | null;
}

// --- Phase 5: library ----------------------------------------------------

export interface MixedAssets {
  materials: MaterialDto[];
  textures: TextureDto[];
}

export interface SearchResults {
  materials: MaterialDto[];
  textures: TextureDto[];
  sets: TextureSetDto[];
}

export interface TagDto {
  id: number;
  name: string;
  count: number;
}

export interface CollectionDto {
  id: number;
  name: string;
  icon: string | null;
  count: number;
}

export interface DuplicateGroup {
  hash: string;
  textures: TextureDto[];
}

// --- Phase 7: Maya bridge ------------------------------------------------

export interface BridgeInfo {
  port: number;
  token: string;
  connected: boolean;
  maya_version: string | null;
}
