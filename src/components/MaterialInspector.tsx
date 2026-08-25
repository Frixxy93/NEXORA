import { useState } from "react";
import type { MaterialDto } from "../lib/types";
import { mapLabel } from "../lib/format";
import { Icon } from "./Icon";
import { api } from "../lib/api";
import {
  CollectionMenu,
  EditableName,
  EditableSelect,
  FavoriteStar,
  RemoveButton,
  SendToMayaButton,
  TagEditor,
} from "./LibraryControls";

const CATEGORY_OPTIONS = [
  "Concrete", "Wood", "Metal", "Stone", "Brick", "Plaster", "Tile", "Fabric",
  "Leather", "Plastic", "Glass", "Rubber", "Ground", "Organic", "Sci-Fi", "Other",
].map((c) => ({ value: c, label: c }));
import { MaterialPreview } from "./MaterialPreview";
import { MaterialPreviewModal } from "./MaterialPreviewModal";
import { useMaterialMaps } from "../lib/useMaterialMaps";

const EXPECTED = ["base_color", "roughness", "normal", "height", "ao"];
const RENDERER_LABELS: Record<string, string> = {
  generic_pbr: "Generic PBR",
  vray: "V-Ray",
  arnold: "Arnold",
};

// Inspector for a selected material (spec §24).
export function MaterialInspector({ material }: { material: MaterialDto }) {
  const bySlot = new Map(material.maps.map((m) => [m.slot, m]));
  const extra = material.maps.map((m) => m.slot).filter((s) => !EXPECTED.includes(s));
  const slots = [...EXPECTED, ...extra];
  const maps = useMaterialMaps(material);
  const [previewOpen, setPreviewOpen] = useState(false);
  const sig = `${material.id}:${Object.keys(maps).length}`;

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 space-y-4 overflow-y-auto flex-1">
        <div className="relative rounded-lg overflow-hidden border border-line bg-ink-900 h-44">
          <MaterialPreview maps={maps} signature={sig} className="w-full h-full" autoRotate />
          <button
            onClick={() => setPreviewOpen(true)}
            className="absolute bottom-2 right-2 text-[11px] px-2 py-1 rounded bg-ink-900/80 text-slate-200 hover:bg-ink-800 border border-line"
            title="Open full preview"
          >
            Expand
          </button>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-[11px] uppercase tracking-wider text-muted">Material</span>
          <FavoriteStar id={material.id} favorite={material.favorite} />
        </div>
        <div>
          <div className="field-label">Name</div>
          <EditableName id={material.id} name={material.name} />
        </div>
        <Row label="Type" value="Material" />
        <div>
          <div className="field-label">Category</div>
          <EditableSelect
            value={material.category ?? "Other"}
            options={CATEGORY_OPTIONS}
            onSave={(v) => api.setAssetCategory(material.id, v)}
          />
        </div>
        <Row label="Resolution" value={material.resolution ?? "—"} />
        <Row label="PBR" value={material.is_pbr ? "Yes" : "No"} />
        <Row label="UDIM" value={material.is_udim ? "Yes" : "No"} />
        <Row
          label="Tileable"
          value={material.tileable == null ? "Unknown" : material.tileable ? "Yes" : "No"}
        />

        <div>
          <div className="field-label mb-2">Maps</div>
          <div className="space-y-1">
            {slots.map((slot) => {
              const m = bySlot.get(slot);
              return (
                <div
                  key={slot}
                  className="flex items-center justify-between py-1 border-b border-line/50 last:border-0"
                >
                  <span className="flex items-center gap-2 text-sm">
                    {m ? (
                      <span className="text-good">
                        <Icon name="check" size={14} />
                      </span>
                    ) : (
                      <span className="text-ink-600 text-xs w-3.5 text-center">✕</span>
                    )}
                    <span className={m ? "text-slate-200" : "text-muted"}>{mapLabel(slot)}</span>
                  </span>
                  {m && (
                    <span className="text-[11px] text-muted truncate max-w-[120px]">{m.name}</span>
                  )}
                </div>
              );
            })}
          </div>
        </div>

        <div>
          <div className="field-label mb-1.5">Renderers</div>
          <div className="flex flex-wrap gap-1.5">
            {material.renderers.map((r) => (
              <span
                key={r}
                className="text-[11px] px-2 py-0.5 rounded bg-ink-750 text-slate-300 border border-line"
              >
                {RENDERER_LABELS[r] ?? r}
              </span>
            ))}
          </div>
        </div>

        <TagEditor assetId={material.id} />
      </div>

      <div className="p-3 border-t border-line">
        <div className="flex items-center justify-between mb-2 text-xs">
          <span className="text-muted">Health · {material.status}</span>
          <span className="text-slate-200 tabular-nums">{material.health}%</span>
        </div>
        <div className="h-1.5 bg-ink-700 rounded overflow-hidden mb-3">
          <div
            className={`h-full ${
              material.health >= 80 ? "bg-good" : material.health >= 50 ? "bg-warn" : "bg-bad"
            }`}
            style={{ width: `${material.health}%` }}
          />
        </div>
        <div className="mb-2">
          <CollectionMenu assetId={material.id} />
        </div>
        <div className="mb-2">
          <SendToMayaButton id={material.id} kind="material" />
        </div>
        <RemoveButton id={material.id} />
      </div>

      {previewOpen && (
        <MaterialPreviewModal material={material} onClose={() => setPreviewOpen(false)} />
      )}
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
