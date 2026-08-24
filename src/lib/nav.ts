// Declarative sidebar structure (spec §3). Each leaf is a `View` id the app
// routes on. Type/smart/collection views all render the shared Library page in
// Phase 1 (empty states); Home and Settings have dedicated pages.

import type { IconName } from "../components/Icon";

export type View = string;

export interface NavLeaf {
  id: View;
  label: string;
  icon: IconName;
}

export interface NavGroup {
  label?: string;
  items: NavLeaf[];
}

export const NAV: NavGroup[] = [
  {
    items: [
      { id: "home", label: "Home", icon: "home" },
      { id: "discover", label: "Discover", icon: "download" },
    ],
  },
  {
    label: "Library",
    items: [
      { id: "lib.materials", label: "Materials", icon: "material" },
      { id: "lib.textures", label: "Textures", icon: "texture" },
    ],
  },
  {
    label: "Material Types",
    items: [
      { id: "mtype.pbr", label: "PBR", icon: "layers" },
      { id: "mtype.vray", label: "V-Ray", icon: "cube" },
      { id: "mtype.arnold", label: "Arnold", icon: "cube" },
      { id: "mtype.udim", label: "UDIM", icon: "grid" },
    ],
  },
  {
    label: "Texture Types",
    items: [
      { id: "ttype.base_color", label: "Base Color", icon: "texture" },
      { id: "ttype.roughness", label: "Roughness", icon: "texture" },
      { id: "ttype.metallic", label: "Metallic", icon: "texture" },
      { id: "ttype.normal", label: "Normal", icon: "texture" },
      { id: "ttype.height", label: "Height", icon: "texture" },
      { id: "ttype.displacement", label: "Displacement", icon: "texture" },
      { id: "ttype.ao", label: "Ambient Occlusion", icon: "texture" },
      { id: "ttype.opacity", label: "Opacity", icon: "texture" },
      { id: "ttype.emission", label: "Emission", icon: "texture" },
      { id: "ttype.mask", label: "Mask", icon: "texture" },
      { id: "ttype.other", label: "Other", icon: "texture" },
    ],
  },
  {
    label: "Smart",
    items: [
      { id: "smart.favorites", label: "Favorites", icon: "star" },
      { id: "smart.recent_used", label: "Recently Used", icon: "clock" },
      { id: "smart.recent_added", label: "Recently Added", icon: "plus" },
      { id: "smart.missing", label: "Missing Maps", icon: "warning" },
      { id: "smart.duplicates", label: "Duplicates", icon: "copy" },
    ],
  },
  {
    items: [
      { id: "collections", label: "Collections", icon: "folder" },
      { id: "settings", label: "Settings", icon: "settings" },
    ],
  },
];

/** Human title + subtitle for the shared Library page, keyed by view id. */
export const VIEW_META: Record<string, { title: string; subtitle: string }> = {
  "lib.materials": { title: "Materials", subtitle: "Complete material assets" },
  "lib.textures": { title: "Textures", subtitle: "Individual texture assets" },
  "mtype.pbr": { title: "PBR Materials", subtitle: "Physically based materials" },
  "mtype.vray": { title: "V-Ray Materials", subtitle: "VRayMtl networks" },
  "mtype.arnold": { title: "Arnold Materials", subtitle: "aiStandardSurface networks" },
  "mtype.udim": { title: "UDIM", subtitle: "Multi-tile texture sets" },
  "ttype.base_color": { title: "Base Color", subtitle: "Albedo / diffuse maps" },
  "ttype.roughness": { title: "Roughness", subtitle: "Roughness / glossiness maps" },
  "ttype.metallic": { title: "Metallic", subtitle: "Metalness maps" },
  "ttype.normal": { title: "Normal", subtitle: "Tangent-space normal maps" },
  "ttype.height": { title: "Height", subtitle: "Height maps" },
  "ttype.displacement": { title: "Displacement", subtitle: "Displacement maps" },
  "ttype.ao": { title: "Ambient Occlusion", subtitle: "AO maps" },
  "ttype.opacity": { title: "Opacity", subtitle: "Opacity / alpha maps" },
  "ttype.emission": { title: "Emission", subtitle: "Emissive maps" },
  "ttype.mask": { title: "Mask", subtitle: "Mask / ID maps" },
  "ttype.other": { title: "Other", subtitle: "Unclassified textures" },
  "smart.favorites": { title: "Favorites", subtitle: "Your starred assets" },
  "smart.recent_used": { title: "Recently Used", subtitle: "Sent to Maya or applied" },
  "smart.recent_added": { title: "Recently Added", subtitle: "Newest imports" },
  "smart.missing": { title: "Missing Maps", subtitle: "Materials with incomplete map sets" },
  "smart.duplicates": { title: "Duplicates", subtitle: "Assets with identical content hashes" },
  collections: { title: "Collections", subtitle: "Virtual groups of assets" },
};
