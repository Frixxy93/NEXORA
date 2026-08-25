import { useEffect, useRef, useState } from "react";
import type { TextureDto } from "../lib/types";
import { api } from "../lib/api";
import { mapLabel, resolutionLabel } from "../lib/format";
import { Icon } from "./Icon";

// A single texture tile (spec §16). The thumbnail is fetched lazily via an
// IntersectionObserver so an off-screen card in a large library never loads its
// image into memory (spec §49).
export function TextureCard({
  texture,
  selected,
  onSelect,
}: {
  texture: TextureDto;
  selected: boolean;
  onSelect: (e: React.MouseEvent) => void;
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
    if (!visible) return;
    let alive = true;
    api.getThumbnail(texture.id).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [visible, texture.id]);

  const res = resolutionLabel(texture.width, texture.height);

  return (
    <div
      ref={ref}
      onClick={onSelect}
      className={`group rounded-lg overflow-hidden bg-ink-800 border cursor-pointer transition-colors ${
        selected ? "border-accent" : "border-line hover:border-ink-600"
      }`}
    >
      <div className="aspect-square bg-ink-900 flex items-center justify-center overflow-hidden relative">
        {thumb ? (
          <img src={thumb} alt={texture.name} className="w-full h-full object-cover" loading="lazy" />
        ) : (
          <span className="text-ink-600">
            <Icon name="texture" size={26} />
          </span>
        )}
        {texture.is_udim && (
          <span className="absolute top-1.5 left-1.5 text-[9px] font-semibold px-1.5 py-0.5 rounded bg-ink-900/80 text-accent-soft">
            UDIM
          </span>
        )}
        {texture.favorite && (
          <span className="absolute top-1.5 right-1.5 text-accent">
            <Icon name="star" size={14} />
          </span>
        )}
      </div>
      <div className="px-2.5 py-2">
        <div className="text-xs font-medium text-slate-200 truncate">{texture.name}</div>
        <div className="text-[10px] text-muted truncate">
          {mapLabel(texture.map_type)}
          {res ? ` · ${res}` : ""}
          {texture.format ? ` · ${texture.format.toUpperCase()}` : ""}
        </div>
      </div>
    </div>
  );
}
