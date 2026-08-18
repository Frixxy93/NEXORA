import { useState } from "react";
import type { MaterialDto } from "../lib/types";
import { useMaterialMaps } from "../lib/useMaterialMaps";
import { MaterialPreview, type PreviewObject } from "./MaterialPreview";
import { Icon } from "./Icon";

const OBJECTS: PreviewObject[] = ["sphere", "cube", "plane", "cylinder"];

// Full-screen interactive material preview (spec §14): object selection, HDRI
// (studio env) lighting with exposure, background toggle, orbit/zoom.
export function MaterialPreviewModal({
  material,
  onClose,
}: {
  material: MaterialDto;
  onClose: () => void;
}) {
  const maps = useMaterialMaps(material);
  const [object, setObject] = useState<PreviewObject>("sphere");
  const [exposure, setExposure] = useState(1);
  const [background, setBackground] = useState(false);
  const [autoRotate, setAutoRotate] = useState(true);
  const sig = `${material.id}:${Object.keys(maps).length}`;

  return (
    <div className="fixed inset-0 z-[60] bg-ink-900/90 backdrop-blur-sm flex flex-col">
      <div className="h-12 shrink-0 flex items-center px-5 border-b border-line">
        <div className="text-sm font-semibold text-white">{material.name}</div>
        <div className="text-xs text-muted ml-2">{material.category ?? "Other"} · Preview</div>
        <button className="ml-auto text-muted hover:text-white" onClick={onClose} title="Close">
          <Icon name="plus" size={20} className="rotate-45" />
        </button>
      </div>

      <div className="flex-1 flex min-h-0">
        <MaterialPreview
          maps={maps}
          signature={sig}
          object={object}
          exposure={exposure}
          background={background}
          autoRotate={autoRotate}
          className="flex-1 min-w-0"
        />

        <aside className="w-64 shrink-0 border-l border-line bg-ink-850 p-4 space-y-5 overflow-y-auto">
          <div>
            <div className="field-label mb-1.5">Preview Object</div>
            <div className="grid grid-cols-2 gap-1.5">
              {OBJECTS.map((o) => (
                <button
                  key={o}
                  onClick={() => setObject(o)}
                  className={`text-xs py-1.5 rounded border capitalize ${
                    object === o
                      ? "border-accent bg-accent/10 text-white"
                      : "border-line text-muted hover:border-ink-600"
                  }`}
                >
                  {o}
                </button>
              ))}
            </div>
          </div>

          <div>
            <div className="field-label mb-1.5">Exposure — {exposure.toFixed(2)}</div>
            <input
              type="range"
              min={0.2}
              max={2.5}
              step={0.05}
              value={exposure}
              onChange={(e) => setExposure(Number(e.target.value))}
              className="w-full accent-accent"
            />
          </div>

          <Toggle label="Show environment" checked={background} onChange={setBackground} />
          <Toggle label="Auto-rotate" checked={autoRotate} onChange={setAutoRotate} />

          <div className="pt-2 border-t border-line text-[11px] text-muted leading-relaxed">
            Drag to orbit · scroll to zoom. Lit by a generated studio environment — real HDRI
            presets and cached renders can slot in here later.
          </div>
        </aside>
      </div>
    </div>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between cursor-pointer">
      <span className="text-sm text-slate-300">{label}</span>
      <button
        type="button"
        onClick={() => onChange(!checked)}
        className={`w-10 h-6 rounded-full p-0.5 transition-colors ${
          checked ? "bg-accent" : "bg-ink-700"
        }`}
      >
        <span
          className={`block w-5 h-5 rounded-full bg-white transition-transform ${
            checked ? "translate-x-4" : ""
          }`}
        />
      </button>
    </label>
  );
}
