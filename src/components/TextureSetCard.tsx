import { useEffect, useRef, useState } from "react";
import type { TextureSetDto } from "../lib/types";
import { api } from "../lib/api";
import { Icon } from "./Icon";
import { mapLabel } from "../lib/format";

const EXPECTED = ["base_color", "roughness", "normal", "height", "ao"];

// A texture-set tile (spec §16). Preview comes from the set's base-color (or
// first) member; a dot strip shows which expected PBR maps are present.
export function TextureSetCard({
  set,
  selected,
  onSelect,
}: {
  set: TextureSetDto;
  selected: boolean;
  onSelect: (e: React.MouseEvent) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState<string | null>(null);
  const [visible, setVisible] = useState(false);

  const previewTexId =
    set.maps.find((m) => m.slot === "base_color")?.texture_id ?? set.maps[0]?.texture_id;

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
    if (!visible || !previewTexId) return;
    let alive = true;
    api.getThumbnail(previewTexId).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [visible, previewTexId]);

  const present = new Set(set.maps.map((m) => m.slot));

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
          <img src={thumb} alt={set.name} className="w-full h-full object-cover" loading="lazy" />
        ) : (
          <span className="text-ink-600">
            <Icon name="layers" size={26} />
          </span>
        )}
        {set.is_pbr && (
          <span className="absolute top-1.5 left-1.5 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-ink-900/80 text-good">
            PBR
          </span>
        )}
        {set.resolution && (
          <span className="absolute top-1.5 right-1.5 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-ink-900/80 text-slate-300">
            {set.resolution}
          </span>
        )}
      </div>
      <div className="px-2.5 py-2">
        <div className="text-xs font-medium text-slate-200 truncate">{set.name}</div>
        <div className="text-[10px] text-muted mb-1.5">{set.member_count} maps</div>
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
