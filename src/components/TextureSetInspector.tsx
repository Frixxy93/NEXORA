import type { TextureSetDto } from "../lib/types";
import { mapLabel } from "../lib/format";
import { Icon } from "./Icon";

const EXPECTED = ["base_color", "roughness", "normal", "height", "ao"];

// Inspector for a selected texture set (spec §24/§31): map checklist with
// present ✓ / missing ✗, PBR status, a health percentage, and a "Create
// Material" action (Texture → Set → Material).
export function TextureSetInspector({
  set,
  onCreateMaterial,
}: {
  set: TextureSetDto;
  onCreateMaterial?: (set: TextureSetDto) => void;
}) {
  const bySlot = new Map(set.maps.map((m) => [m.slot, m]));
  // Show expected slots first (in order), then any extra slots the set has.
  const extra = set.maps.map((m) => m.slot).filter((s) => !EXPECTED.includes(s));
  const slots = [...EXPECTED, ...extra];
  const presentCount = slots.filter((s) => bySlot.has(s)).length;
  const health = Math.round((set.maps.length / Math.max(slots.length, 1)) * 100);

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 space-y-4 overflow-y-auto flex-1">
        <Row label="Name" value={set.name} />
        <Row label="Type" value="Texture Set" />
        <Row label="Resolution" value={set.resolution ?? "—"} />
        <Row label="PBR" value={set.is_pbr ? "Yes" : "No"} />
        <Row
          label="Tileable"
          value={set.tileable == null ? "Unknown" : set.tileable ? "Yes" : "No"}
        />

        <div>
          <div className="flex items-center justify-between mb-2">
            <div className="field-label mb-0">Maps</div>
            <span className="text-[11px] text-muted">
              {presentCount}/{slots.length}
            </span>
          </div>
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
                  {m && <span className="text-[11px] text-muted truncate max-w-[120px]">{m.name}</span>}
                </div>
              );
            })}
          </div>
        </div>

        {set.missing_maps.length > 0 && (
          <div className="text-[11px] text-warn">
            Missing: {set.missing_maps.map((s) => mapLabel(s)).join(", ")}
          </div>
        )}
      </div>

      <div className="p-3 border-t border-line">
        <div className="flex items-center justify-between mb-2 text-xs">
          <span className="text-muted">Set health</span>
          <span className="text-slate-200 tabular-nums">{health}%</span>
        </div>
        <div className="h-1.5 bg-ink-700 rounded overflow-hidden mb-3">
          <div
            className={`h-full ${health >= 80 ? "bg-good" : health >= 50 ? "bg-warn" : "bg-bad"}`}
            style={{ width: `${health}%` }}
          />
        </div>
        <button className="btn-primary text-xs w-full mb-2" onClick={() => onCreateMaterial?.(set)}>
          Create Material
        </button>
        <button
          className="btn-ghost text-xs w-full opacity-50 cursor-not-allowed"
          title="Available once the Maya Bridge lands (Phase 7)"
          disabled
        >
          Send to Maya
        </button>
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
