import { useEffect, useState } from "react";
import { api } from "./api";
import type { MaterialDto } from "./types";
import type { PreviewMaps } from "../components/MaterialPreview";

const PREVIEW_SLOTS = ["base_color", "roughness", "metallic", "normal", "ao", "height"];

/** Load a material's map thumbnails as data URLs for the 3D preview. */
export function useMaterialMaps(material: MaterialDto | null): PreviewMaps {
  const [maps, setMaps] = useState<PreviewMaps>({});
  useEffect(() => {
    let alive = true;
    setMaps({});
    if (!material) return;
    const entries = material.maps.filter((m) => PREVIEW_SLOTS.includes(m.slot));
    Promise.all(
      entries.map(async (m) => [m.slot, await api.getThumbnail(m.texture_id)] as const)
    )
      .then((pairs) => {
        if (!alive) return;
        const out: PreviewMaps = {};
        for (const [slot, url] of pairs) if (url) out[slot] = url;
        setMaps(out);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [material?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  return maps;
}
