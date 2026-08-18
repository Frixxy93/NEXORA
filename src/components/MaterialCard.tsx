import { useEffect, useRef, useState } from "react";
import type { MaterialDto } from "../lib/types";
import { api } from "../lib/api";
import { Icon } from "./Icon";
import { mapLabel } from "../lib/format";

const EXPECTED = ["base_color", "roughness", "normal", "height", "ao"];

// A material tile (spec §16). Preview is the base-color map thumbnail; a full 3D
// sphere render arrives with the preview engine (Phase 6).
export function MaterialCard({
  material,
  selected,
  onSelect,
}: {
  material: MaterialDto;
  selected: boolean;
  onSelect: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState<string | null>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          io.disconnect();
        }
      },
      { rootMargin: "200px" }
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible || !material.preview_texture_id) return;
    let alive = true;
    api
      .getThumbnail(material.preview_texture_id)
      .then((t) => alive && setThumb(t))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [visible, material.preview_texture_id]);

  const present = new Set(material.maps.map((m) => m.slot));

  return (
    <div
      ref={ref}
      onClick={onSelect}
      className={`rounded-lg overflow-hidden bg-ink-800 border cursor-pointer transition-colors ${
        selected ? "border-accent" : "border-line hover:border-ink-600"
      }`}
    >
      <div className="aspect-square bg-ink-900 flex items-center justify-center overflow-hidden relative">
        {thumb ? (
          <img src={thumb} alt={material.name} className="w-full h-full object-cover" loading="lazy" />
        ) : (
          <span className="text-ink-600">
            <Icon name="material" size={28} />
          </span>
        )}
        {material.is_pbr && (
          <span className="absolute top-1.5 left-1.5 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-ink-900/80 text-good">
            PBR
          </span>
        )}
        {material.resolution && (
          <span className="absolute top-1.5 right-1.5 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-ink-900/80 text-slate-300">
            {material.resolution}
          </span>
        )}
      </div>
      <div className="px-2.5 py-2">
        <div className="text-xs font-medium text-slate-200 truncate">{material.name}</div>
        <div className="text-[10px] text-muted mb-1.5 truncate">
          {material.category ?? "Other"} · {material.maps.length} maps
        </div>
        <div className="flex gap-1">
          {EXPECTED.map((slot) => (
            <span
              key={slot}
              title={`${mapLabel(slot)}: ${present.has(slot) ? "present" : "missing"}`}
              className={`w-2 h-2 rounded-full ${present.has(slot) ? "bg-good" : "bg-ink-700"}`}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
