import { useEffect, useState } from "react";
import type { TextureDto, UdimInfo } from "../lib/types";
import { formatBytes, mapLabel, resolutionLabel } from "../lib/format";
import { api, copyToClipboard, dirname, openPath, revealInExplorer } from "../lib/api";
import {
  CollectionMenu,
  FavoriteStar,
  RemoveButton,
  SendToMayaButton,
  TagEditor,
} from "./LibraryControls";
import { Icon } from "./Icon";

// Right-side inspector for a selected texture (spec §23). When the texture is a
// UDIM set, `udim` carries its tile coverage (spec §12).
export function TextureInspector({
  texture,
  udim,
}: {
  texture: TextureDto;
  udim?: UdimInfo | null;
}) {
  const res = resolutionLabel(texture.width, texture.height);
  const [thumb, setThumb] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    setThumb(null);
    api.getThumbnail(texture.id).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [texture.id]);

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 space-y-4 overflow-y-auto flex-1">
        <div className="rounded-lg overflow-hidden border border-line bg-ink-900 h-44 flex items-center justify-center">
          {thumb ? (
            <img src={thumb} alt={texture.name} className="w-full h-full object-contain" />
          ) : (
            <span className="text-ink-600">
              <Icon name="texture" size={30} />
            </span>
          )}
        </div>
        <div className="flex items-center justify-between">
          <span className="text-[11px] uppercase tracking-wider text-muted">Texture</span>
          <FavoriteStar id={texture.id} favorite={texture.favorite} />
        </div>
        <Row label="Name" value={texture.name} />
        <Row label="Type" value={mapLabel(texture.map_type)} />
        <Row
          label="Resolution"
          value={
            texture.width && texture.height
              ? `${texture.width} × ${texture.height}${res && res !== `${texture.width} × ${texture.height}` ? ` (${res})` : ""}`
              : "—"
          }
        />
        <Row label="Format" value={texture.format ? texture.format.toUpperCase() : "—"} />
        <Row label="Size" value={formatBytes(texture.file_size)} />
        <Row label="Color Space" value={texture.color_space ?? "—"} />
        <Row label="Channels" value={texture.channels != null ? String(texture.channels) : "—"} />
        <Row label="UDIM" value={texture.is_udim ? "Yes" : "No"} />
        <Row
          label="Tileable"
          value={texture.tileable == null ? "Unknown" : texture.tileable ? "Yes" : "No"}
        />
        <Row label="Storage" value={texture.managed ? "Managed" : "Referenced"} />
        {texture.is_udim && udim && udim.tile_count > 0 && (
          <div>
            <div className="flex items-center justify-between mb-2">
              <div className="field-label mb-0">UDIM Tiles</div>
              <span className="text-[11px] text-muted">{udim.tile_count} tiles</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {(() => {
                const present = new Set(udim.tiles);
                const min = Math.min(...udim.tiles);
                const max = Math.max(...udim.tiles);
                const range: number[] = [];
                for (let t = min; t <= max; t++) range.push(t);
                return range.map((t) => (
                  <span
                    key={t}
                    className={`text-[10px] font-mono px-1.5 py-0.5 rounded ${
                      present.has(t)
                        ? "bg-ink-700 text-slate-300"
                        : "bg-bad/20 text-bad line-through"
                    }`}
                  >
                    {t}
                  </span>
                ));
              })()}
            </div>
            {udim.missing.length > 0 && (
              <div className="text-[11px] text-bad mt-1.5">
                Missing: {udim.missing.join(", ")}
              </div>
            )}
          </div>
        )}

        <div>
          <div className="field-label">Path</div>
          <div className="text-[11px] font-mono text-slate-400 break-all bg-ink-900 border border-line rounded-md p-2">
            {texture.file_path}
          </div>
        </div>

        <TagEditor assetId={texture.id} />
      </div>

      <div className="p-3 border-t border-line space-y-2">
        <CollectionMenu assetId={texture.id} />
        <div className="grid grid-cols-2 gap-2">
          <button className="btn-ghost text-xs" onClick={() => copyToClipboard(texture.file_path)}>
            Copy Path
          </button>
        <button className="btn-ghost text-xs" onClick={() => openPath(dirname(texture.file_path))}>
          Open Folder
        </button>
        <button className="btn-ghost text-xs" onClick={() => revealInExplorer(texture.file_path)}>
          Reveal
        </button>
          <SendToMayaButton id={texture.id} kind="texture" className="btn-ghost text-xs" />
        </div>
        <RemoveButton id={texture.id} />
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="field-label">{label}</div>
      <div className="text-sm text-slate-200 break-words">{value}</div>
    </div>
  );
}
