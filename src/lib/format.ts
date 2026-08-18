// Small display-formatting helpers shared across texture UI.

export function formatBytes(bytes: number | null | undefined): string {
  if (!bytes && bytes !== 0) return "—";
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const val = bytes / Math.pow(1024, i);
  return `${val.toFixed(val >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}

/** A friendly resolution label: 4096×4096 → "4K", non-square → "2048 × 1024". */
export function resolutionLabel(
  w: number | null | undefined,
  h: number | null | undefined
): string | null {
  if (!w || !h) return null;
  if (w === h) {
    const k: Record<number, string> = { 512: "512", 1024: "1K", 2048: "2K", 4096: "4K", 8192: "8K" };
    if (k[w]) return k[w];
  }
  return `${w} × ${h}`;
}

const MAP_LABELS: Record<string, string> = {
  base_color: "Base Color",
  roughness: "Roughness",
  glossiness: "Glossiness",
  metallic: "Metallic",
  normal: "Normal",
  height: "Height",
  displacement: "Displacement",
  bump: "Bump",
  ao: "Ambient Occlusion",
  specular: "Specular",
  opacity: "Opacity",
  emission: "Emission",
  transmission: "Transmission",
  thickness: "Thickness",
  mask: "Mask",
  id: "ID",
};

export function mapLabel(slug: string | null | undefined): string {
  if (!slug) return "Unclassified";
  return MAP_LABELS[slug] ?? slug;
}
