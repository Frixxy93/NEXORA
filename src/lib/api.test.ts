import { describe, it, expect, beforeAll } from "vitest";
import { api } from "./api";

// Outside Tauri, `api` is backed by the in-memory browser mock, so these tests
// exercise the real mock/store logic that the preview + tests depend on.

async function seed(names: string[]): Promise<string[]> {
  await api.importPaths(names.map((n) => `/lib/${n}`));
  const all = await api.listTextures(null);
  return names
    .map((n) => n.replace(/\.[^.]+$/, ""))
    .map((base) => all.find((t) => t.name === base)?.id)
    .filter((x): x is string => !!x);
}

describe("api mock — settings", () => {
  it("returns coherent default settings", async () => {
    const s = await api.getSettings();
    expect(s.library.storage_mode).toBe("managed");
    expect(s.appearance.grid_size).toBeGreaterThan(0);
    expect(s.discover.source_polyhaven).toBe(true);
  });
});

describe("api mock — favorites, rename, tags", () => {
  let id = "";
  beforeAll(async () => {
    [id] = await seed(["Alpha_BaseColor.png"]);
  });

  it("toggles favorite and reflects it in listFavorites", async () => {
    await api.setFavorite(id, true);
    let favs = await api.listFavorites();
    expect(favs.textures.some((t) => t.id === id)).toBe(true);

    await api.setFavorite(id, false);
    favs = await api.listFavorites();
    expect(favs.textures.some((t) => t.id === id)).toBe(false);
  });

  it("renames an asset", async () => {
    await api.renameAsset(id, "Alpha Renamed");
    const got = await api.getTexture(id);
    expect(got?.name).toBe("Alpha Renamed");
  });

  it("adds a tag (stripping the leading #)", async () => {
    await api.addTag(id, "#weathered");
    const tags = await api.tagsForAsset(id);
    expect(tags.some((t) => t.name === "weathered")).toBe(true);
  });
});

describe("api mock — bulk operations", () => {
  it("bulk-favorites then bulk-removes a set", async () => {
    const ids = await seed(["Bulk1_BaseColor.png", "Bulk2_Roughness.png"]);
    expect(ids.length).toBe(2);

    await api.setFavoriteMany(ids, true);
    const favs = await api.listFavorites();
    for (const id of ids) expect(favs.textures.some((t) => t.id === id)).toBe(true);

    await api.removeAssets(ids);
    const after = await api.listTextures(null);
    for (const id of ids) expect(after.some((t) => t.id === id)).toBe(false);
  });
});

describe("api mock — discover browse", () => {
  it("returns a catalog with thumbnail + synced flags", async () => {
    const list = await api.discoverBrowse("polyhaven");
    expect(list.length).toBeGreaterThan(0);
    expect(list.every((a) => a.source === "polyhaven")).toBe(true);
    expect(list[0]).toHaveProperty("thumbnail_url");
    expect(list[0]).toHaveProperty("categories");
    expect(list.some((a) => a.synced)).toBe(true);
  });
});
