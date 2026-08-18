import { Icon, type IconName } from "./Icon";

export function StatCard({
  label,
  value,
  icon,
}: {
  label: string;
  value: number | string;
  icon: IconName;
}) {
  return (
    <div className="panel px-4 py-3.5 flex items-center gap-3.5">
      <div className="w-9 h-9 rounded-lg bg-ink-750 text-accent flex items-center justify-center">
        <Icon name={icon} size={18} />
      </div>
      <div className="leading-tight">
        <div className="text-2xl font-semibold text-white tabular-nums">
          {typeof value === "number" ? value.toLocaleString() : value}
        </div>
        <div className="text-xs text-muted">{label}</div>
      </div>
    </div>
  );
}
