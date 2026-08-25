import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { CollectionDto, TagDto } from "../lib/types";
import { Icon } from "./Icon";

// Inline-editable asset name (spec §23/§24). Shows the name with a pencil; click
// to edit, Enter/blur saves, Escape cancels. The backend emits library:changed
// so the grid + inspector refresh with the canonical value.
export function EditableName({ id, name }: { id: string; name: string }) {
  const [editing, setEditing] = useState(false);
  const [val, setVal] = useState(name);
  useEffect(() => setVal(name), [name, id]);

  const save = async () => {
    setEditing(false);
    const next = val.trim();
    if (!next || next === name) {
      setVal(name);
      return;
    }
    try {
      await api.renameAsset(id, next);
    } catch (err) {
      console.error(err);
      setVal(name);
    }
  };

  if (editing) {
    return (
      <input
        autoFocus
        className="input text-sm py-1"
        value={val}
        onChange={(e) => setVal(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => {
          if (e.key === "Enter") save();
          if (e.key === "Escape") {
            setVal(name);
            setEditing(false);
          }
        }}
      />
    );
  }
  return (
    <div className="group/name flex items-center gap-1.5">
      <span className="text-sm text-slate-200 break-words">{name}</span>
      <button
        className="shrink-0 text-muted hover:text-slate-200 opacity-0 group-hover/name:opacity-100 transition-opacity"
        onClick={() => setEditing(true)}
        title="Rename"
      >
        <Icon name="edit" size={13} />
      </button>
    </div>
  );
}

// Inline dropdown for a metadata field (map type, category). Saves on change.
export function EditableSelect({
  value,
  options,
  onSave,
}: {
  value: string;
  options: { value: string; label: string }[];
  onSave: (value: string) => Promise<void>;
}) {
  const [val, setVal] = useState(value);
  const [busy, setBusy] = useState(false);
  useEffect(() => setVal(value), [value]);

  const change = async (next: string) => {
    setVal(next);
    setBusy(true);
    try {
      await onSave(next);
    } catch (err) {
      console.error(err);
      setVal(value);
    } finally {
      setBusy(false);
    }
  };

  // Always render the current value, even if it isn't one of the known options
  // (e.g. a custom map type), so the dropdown never silently misrepresents it.
  const opts = options.some((o) => o.value === val)
    ? options
    : [{ value: val, label: val }, ...options];

  return (
    <select
      className="input text-sm py-1 disabled:opacity-60"
      value={val}
      disabled={busy}
      onChange={(e) => change(e.target.value)}
    >
      {opts.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

// Favorite toggle star (spec §21). Optimistic; the backend emits library:changed
// so grids refresh on their own.
export function FavoriteStar({ id, favorite }: { id: string; favorite: boolean }) {
  const [fav, setFav] = useState(favorite);
  useEffect(() => setFav(favorite), [favorite, id]);
  const toggle = async () => {
    const next = !fav;
    setFav(next);
    try {
      await api.setFavorite(id, next);
    } catch {
      setFav(!next);
    }
  };
  return (
    <button
      onClick={toggle}
      title={fav ? "Remove from favorites" : "Add to favorites"}
      className={fav ? "text-accent" : "text-muted hover:text-slate-300"}
    >
      <Icon name="star" size={18} />
    </button>
  );
}

// Tag chips with add/remove (spec §19).
export function TagEditor({ assetId }: { assetId: string }) {
  const [tags, setTags] = useState<TagDto[]>([]);
  const [input, setInput] = useState("");

  const load = () => api.tagsForAsset(assetId).then(setTags).catch(() => setTags([]));
  useEffect(() => {
    load();
  }, [assetId]); // eslint-disable-line react-hooks/exhaustive-deps

  const add = async () => {
    const name = input.trim();
    if (!name) return;
    setInput("");
    await api.addTag(assetId, name);
    load();
  };
  const remove = async (tagId: number) => {
    await api.removeTag(assetId, tagId);
    load();
  };

  return (
    <div>
      <div className="field-label mb-1.5">Tags</div>
      <div className="flex flex-wrap gap-1.5 mb-2">
        {tags.length === 0 && <span className="text-[11px] text-muted">No tags yet</span>}
        {tags.map((t) => (
          <span
            key={t.id}
            className="group flex items-center gap-1 text-[11px] px-2 py-0.5 rounded bg-ink-750 text-slate-300 border border-line"
          >
            #{t.name}
            <button
              onClick={() => remove(t.id)}
              className="text-muted hover:text-bad"
              title="Remove tag"
            >
              ×
            </button>
          </span>
        ))}
      </div>
      <div className="flex gap-1.5">
        <input
          className="input text-xs py-1"
          placeholder="Add a tag…"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <button className="btn-ghost text-xs px-2" onClick={add}>
          Add
        </button>
      </div>
    </div>
  );
}

// Remove an asset from the library (spec §26 — record only, file kept). Uses an
// inline two-step confirm rather than a native dialog (blocked in the webview).
export function RemoveButton({ id, onRemoved }: { id: string; onRemoved?: () => void }) {
  const [confirm, setConfirm] = useState(false);
  const remove = async () => {
    await api.removeAsset(id);
    setConfirm(false);
    onRemoved?.();
  };
  if (confirm) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-[11px] text-muted flex-1">Remove from library? (file kept)</span>
        <button className="btn text-xs bg-bad/20 text-bad hover:bg-bad/30 px-2 py-1" onClick={remove}>
          Remove
        </button>
        <button className="btn-ghost text-xs px-2 py-1" onClick={() => setConfirm(false)}>
          Cancel
        </button>
      </div>
    );
  }
  return (
    <button
      className="btn-ghost text-xs w-full text-bad/90 hover:text-bad"
      onClick={() => setConfirm(true)}
      title="Removes the library record only — your file on disk is not deleted"
    >
      Remove from Library
    </button>
  );
}

// Queue an asset to send into Maya (spec §34/§35). The plug-in picks it up on
// its next poll; the button briefly confirms.
export function SendToMayaButton({
  id,
  kind,
  className = "btn-ghost text-xs w-full",
}: {
  id: string;
  kind: "material" | "texture";
  className?: string;
}) {
  const [sent, setSent] = useState(false);
  const send = async () => {
    try {
      await api.sendToMaya(id, kind);
      setSent(true);
      window.setTimeout(() => setSent(false), 2500);
    } catch {
      /* ignore */
    }
  };
  return (
    <button className={className} onClick={send} title="Queue for the NEXORA Maya plug-in">
      {sent ? "Queued for Maya ✓" : "Send to Maya"}
    </button>
  );
}

// Add-to-collection menu (spec §22), with inline collection creation.
export function CollectionMenu({ assetId }: { assetId: string }) {
  const [open, setOpen] = useState(false);
  const [cols, setCols] = useState<CollectionDto[]>([]);
  const [newName, setNewName] = useState("");

  const load = () => api.listCollections().then(setCols).catch(() => setCols([]));
  useEffect(() => {
    if (open) load();
  }, [open]);

  const addTo = async (cid: number) => {
    await api.addToCollection(cid, assetId);
    setOpen(false);
  };
  const createAndAdd = async () => {
    const name = newName.trim();
    if (!name) return;
    const col = await api.createCollection(name);
    await api.addToCollection(col.id, assetId);
    setNewName("");
    setOpen(false);
  };

  return (
    <div className="relative">
      <button className="btn-ghost text-xs w-full" onClick={() => setOpen((o) => !o)}>
        Add to Collection
      </button>
      {open && (
        <div className="absolute bottom-full mb-1 left-0 right-0 z-10 panel p-2 max-h-64 overflow-y-auto shadow-xl">
          {cols.map((c) => (
            <button
              key={c.id}
              onClick={() => addTo(c.id)}
              className="w-full text-left px-2 py-1.5 rounded text-xs text-slate-300 hover:bg-ink-750 flex items-center gap-2"
            >
              <Icon name="folder" size={13} /> {c.name}
              <span className="ml-auto text-muted">{c.count}</span>
            </button>
          ))}
          <div className="flex gap-1.5 mt-1.5 pt-1.5 border-t border-line">
            <input
              className="input text-xs py-1"
              placeholder="New collection…"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && createAndAdd()}
            />
            <button className="btn-ghost text-xs px-2" onClick={createAndAdd}>
              +
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
