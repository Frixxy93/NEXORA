import { Icon, type IconName } from "./Icon";

export function EmptyState({
  icon = "grid",
  title,
  hint,
  action,
}: {
  icon?: IconName;
  title: string;
  hint?: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex-1 flex flex-col items-center justify-center text-center px-6 py-16">
      <div className="w-16 h-16 rounded-2xl bg-ink-800 border border-line flex items-center justify-center text-muted mb-4">
        <Icon name={icon} size={28} />
      </div>
      <div className="text-slate-200 font-medium">{title}</div>
      {hint && <div className="text-sm text-muted mt-1 max-w-sm">{hint}</div>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}
