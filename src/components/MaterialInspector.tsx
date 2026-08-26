import { useEffect, useMemo, useState } from "react";
import type { MaterialDto, TextureDto } from "../lib/types";
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
// Every slot a material can carry (the extras beyond EXPECTED can be added too).
const ALL_SLOTS = [
  "base_color", "roughness", "metallic", "normal", "height", "displacement",
  "ao", "opacity", "emission", "glossiness", "mask",
];
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
          <div className="space-y-0.5">
            {slots.map((slot) => (
              <MapSlotRow key={slot} materialId={material.id} slot={slot} map={bySlot.get(slot)} />
            ))}
          </div>
          <AddMapControl material={material} />
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

// One editable map slot: filled slots can be swapped or removed; empty EXPECTED
// slots can be filled. Opening the picker expands an inline texture chooser.
function MapSlotRow({
  materialId,
  slot,
  map,
}: {
  materialId: string;
  slot: string;
  map: { texture_id: string; name: string } | undefined;
}) {
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);

  async function set(textureId: string | null) {
    setBusy(true);
    try {
      await api.setMaterialMap(materialId, slot, textureId);
    } finally {
      setBusy(false);
      setPicking(false);
    }
  }

  return (
    <div className="border-b border-line/50 last:border-0">
      <div className="group flex items-center justify-between py-1 gap-2">
        <span className="flex items-center gap-2 text-sm min-w-0">
          {map ? (
            <span className="text-good shrink-0">
              <Icon name="check" size={14} />
            </span>
          ) : (
            <span className="text-ink-600 text-xs w-3.5 text-center shrink-0">✕</span>
          )}
          <span className={map ? "text-slate-200 shrink-0" : "text-muted shrink-0"}>
            {mapLabel(slot)}
          </span>
          {map && (
            <span className="text-[11px] text-muted truncate">{map.name}</span>
          )}
        </span>
        <span className="flex items-center gap-1 shrink-0">
          {map ? (
            <>
              <button
                onClick={() => setPicking((v) => !v)}
                disabled={busy}
                className="text-[11px] px-1.5 py-0.5 rounded text-muted hover:text-slate-100 hover:bg-ink-750 disabled:opacity-40"
                title="Replace this map"
              >
                Swap
              </button>
              <button
                onClick={() => set(null)}
                disabled={busy}
                className="text-[11px] px-1.5 py-0.5 rounded text-muted hover:text-bad hover:bg-ink-750 disabled:opacity-40"
                title="Remove this map"
              >
                Remove
              </button>
            </>
          ) : (
            <button
              onClick={() => setPicking((v) => !v)}
              disabled={busy}
              className="text-[11px] px-1.5 py-0.5 rounded text-accent hover:bg-ink-750 disabled:opacity-40 opacity-0 group-hover:opacity-100 focus:opacity-100"
              title="Add a map for this slot"
            >
              + Add
            </button>
          )}
        </span>
      </div>
      {picking && (
        <TexturePicker
          slot={slot}
          currentId={map?.texture_id}
          onPick={(id) => set(id)}
          onClose={() => setPicking(false)}
        />
      )}
    </div>
  );
}

// Lets the user add a slot that isn't one of the always-shown EXPECTED slots
// (e.g. metallic, displacement, emission) and immediately fill it.
function AddMapControl({ material }: { material: MaterialDto }) {
  const present = new Set(material.maps.map((m) => m.slot));
  const shown = new Set([...EXPECTED, ...material.maps.map((m) => m.slot)]);
  const addable = ALL_SLOTS.filter((s) => !shown.has(s) && !present.has(s));
  const [slot, setSlot] = useState<string | null>(null);

  if (addable.length === 0) return null;

  return (
    <div className="mt-2">
      {slot ? (
        <div className="rounded border border-line bg-ink-850 p-2">
          <div className="flex items-center justify-between mb-1.5">
            <span className="text-[11px] text-slate-200">Add {mapLabel(slot)}</span>
            <button
              onClick={() => setSlot(null)}
              className="text-[11px] text-muted hover:text-slate-100"
            >
              Cancel
            </button>
          </div>
          <TexturePicker
            slot={slot}
            onPick={async (id) => {
              await api.setMaterialMap(material.id, slot, id);
              setSlot(null);
            }}
            onClose={() => setSlot(null)}
            embedded
          />
        </div>
      ) : (
        <select
          value=""
          onChange={(e) => e.target.value && setSlot(e.target.value)}
          className="w-full text-[11px] bg-ink-850 border border-line rounded px-2 py-1 text-muted hover:text-slate-200"
        >
          <option value="">+ Add another map…</option>
          {addable.map((s) => (
            <option key={s} value={s}>
              {mapLabel(s)}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}

// Inline texture chooser: lists library textures, matching-slot ones first, with
// a search box. Picking one calls onPick(textureId).
function TexturePicker({
  slot,
  currentId,
  onPick,
  onClose,
  embedded,
}: {
  slot: string;
  currentId?: string;
  onPick: (id: string) => void;
  onClose: () => void;
  embedded?: boolean;
}) {
  const [all, setAll] = useState<TextureDto[] | null>(null);
  const [q, setQ] = useState("");

  useEffect(() => {
    let alive = true;
    api.listTextures(null).then((list) => {
      if (alive) setAll(list);
    });
    return () => {
      alive = false;
    };
  }, []);

  const results = useMemo(() => {
    if (!all) return [];
    const needle = q.trim().toLowerCase();
    const filtered = needle
      ? all.filter((t) => t.name.toLowerCase().includes(needle))
      : all;
    // Textures whose map_type matches this slot float to the top.
    return [...filtered].sort((a, b) => {
      const am = a.map_type === slot ? 0 : 1;
      const bm = b.map_type === slot ? 0 : 1;
      if (am !== bm) return am - bm;
      return a.name.localeCompare(b.name);
    });
  }, [all, q, slot]);

  return (
    <div className={embedded ? "" : "pb-2 pl-6 pr-1"}>
      <input
        autoFocus
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        placeholder="Search textures…"
        className="w-full text-[11px] bg-ink-900 border border-line rounded px-2 py-1 mb-1 text-slate-200 placeholder:text-ink-600"
      />
      <div className="max-h-44 overflow-y-auto rounded border border-line bg-ink-900">
        {all === null ? (
          <div className="text-[11px] text-muted px-2 py-2">Loading…</div>
        ) : results.length === 0 ? (
          <div className="text-[11px] text-muted px-2 py-2">No textures found.</div>
        ) : (
          results.map((t) => (
            <button
              key={t.id}
              onClick={() => onPick(t.id)}
              disabled={t.id === currentId}
              className="w-full flex items-center justify-between gap-2 px-2 py-1 text-left hover:bg-ink-800 disabled:opacity-40"
            >
              <span className="text-[11px] text-slate-200 truncate">{t.name}</span>
              <span className="text-[10px] text-muted shrink-0">
                {t.id === currentId ? "current" : t.map_type ? mapLabel(t.map_type) : "—"}
              </span>
            </button>
          ))
        )}
      </div>
    </div>
  );
}
