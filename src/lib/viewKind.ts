// Maps a sidebar `View` id to the kind of content the Library page should show.
// Extracted from Library.tsx so the routing logic is unit-testable in isolation
// (without pulling in the whole component tree / three.js).

import type { View } from "./nav";

export type Kind =
  | { t: "library" }
  | { t: "textures"; slug: string }
  | { t: "udim" }
  | { t: "materials" }
  | { t: "materials-pbr" }
  | { t: "materials-renderer"; renderer: string }
  | { t: "mixed"; source: "favorites" | "recent_added" | "recent_used" }
  | { t: "duplicates" }
  | { t: "missing_files" }
  | { t: "collections" }
  | null;

export function viewKind(view: View): Kind {
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
  if (view === "smart.missing_files") return { t: "missing_files" };
  if (view === "collections") return { t: "collections" };
  return null;
}
