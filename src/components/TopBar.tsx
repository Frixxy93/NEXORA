import { Icon } from "./Icon";
import type { MayaStatus } from "../lib/types";
import { runningInTauri } from "../lib/api";

export function TopBar({
  query,
  onQuery,
  maya,
}: {
  query: string;
  onQuery: (q: string) => void;
  maya: MayaStatus | null;
}) {
  return (
    <header className="h-14 shrink-0 border-b border-line flex items-center gap-4 px-4 bg-ink-900/60 backdrop-blur">
      <div className="relative flex-1 max-w-xl">
        <span className="absolute left-3 top-1/2 -translate-y-1/2 text-muted">
          <Icon name="search" size={16} />
        </span>
        <input
          className="input pl-9"
          placeholder="Search materials, textures, tags, map types…"
          value={query}
          onChange={(e) => onQuery(e.target.value)}
        />
      </div>

      <div className="ml-auto flex items-center gap-3 text-xs">
        <div className="flex items-center gap-1.5 text-muted">
          <Icon name="maya" size={16} />
          <span
            className={`w-2 h-2 rounded-full ${
              maya?.connected ? "bg-good" : "bg-ink-600"
            }`}
          />
          <span>{maya?.connected ? "Maya connected" : "Maya offline"}</span>
        </div>
        <span className="text-muted/70">
          {runningInTauri ? "Desktop" : "Preview"}
        </span>
      </div>
    </header>
  );
}
