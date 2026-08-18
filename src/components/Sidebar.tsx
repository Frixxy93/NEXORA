import { NAV, type View } from "../lib/nav";
import { Icon } from "./Icon";

export function Sidebar({
  view,
  onNavigate,
}: {
  view: View;
  onNavigate: (v: View) => void;
}) {
  return (
    <nav className="w-60 shrink-0 h-full bg-ink-850 border-r border-line flex flex-col">
      <div className="px-4 h-14 flex items-center gap-2.5 border-b border-line">
        <div className="w-7 h-7 rounded-md bg-accent flex items-center justify-center">
          <span className="text-ink-900 font-bold text-sm">N</span>
        </div>
        <div className="leading-tight">
          <div className="text-sm font-semibold tracking-wide text-white">NEXORA</div>
          <div className="text-[10px] text-muted">Store · Organize · Use</div>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-2 pb-4">
        {NAV.map((group, gi) => (
          <div key={gi}>
            {group.label && <div className="nav-group-label">{group.label}</div>}
            {group.items.map((leaf) => (
              <div
                key={leaf.id}
                className={`nav-item ${view === leaf.id ? "active" : ""}`}
                onClick={() => onNavigate(leaf.id)}
              >
                <Icon name={leaf.icon} size={16} />
                <span className="truncate">{leaf.label}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </nav>
  );
}
