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

describe("api mock — cloud app lock", () => {
  it("registers, logs out, rejects bad login, and logs back in", async () => {
    // Fresh session: signed out.
    let status = await api.authStatus();
    expect(status.authenticated).toBe(false);

    // Registering creates the account and signs in.
    status = await api.authRegister("Artist@Studio.com", "supersecret");
    expect(status.authenticated).toBe(true);
    expect(status.email).toBe("artist@studio.com"); // normalized lowercase

    // Registering the same email again is refused.
    await expect(api.authRegister("artist@studio.com", "another1")).rejects.toThrow();

    // Log out clears the session.
    status = await api.authLogout();
    expect(status.authenticated).toBe(false);

    // Wrong password is rejected; correct password (case-insensitive email) works.
    await expect(api.authLogin("artist@studio.com", "nope")).rejects.toThrow();
    status = await api.authLogin("ARTIST@studio.com", "supersecret");
    expect(status.authenticated).toBe(true);

    // Change password: wrong current rejected, correct one rotates it.
    await expect(api.authChangePassword("wrong", "brandnew1")).rejects.toThrow();
    await api.authChangePassword("supersecret", "brandnew1");
    await api.authLogout();
    await expect(api.authLogin("artist@studio.com", "supersecret")).rejects.toThrow();
    status = await api.authLogin("artist@studio.com", "brandnew1");
    expect(status.authenticated).toBe(true);
  });

  it("rejects invalid emails and short passwords on register", async () => {
    await expect(api.authRegister("notanemail", "goodpass")).rejects.toThrow();
    await expect(api.authRegister("new@user.com", "abc")).rejects.toThrow();
  });

  it("password reset accepts a valid email and rejects a malformed one", async () => {
    // Succeeds for a well-formed email whether or not it's registered (no probing).
    await expect(api.authSendPasswordReset("anyone@example.com")).resolves.toBeUndefined();
    await expect(api.authSendPasswordReset("notanemail")).rejects.toThrow();
  });

  it("Google sign-in authenticates and logout clears it", async () => {
    await api.authLogout();
    let status = await api.authLoginGoogle();
    expect(status.authenticated).toBe(true);
    expect(status.email).toBeTruthy();
    status = await api.authLogout();
    expect(status.authenticated).toBe(false);
  });
});

describe("api mock — material map editing", () => {
  it("adds, swaps, and removes a material's map slots", async () => {
    // Build a material from JUST base_color + normal (no roughness yet), so we
    // can watch adding roughness complete the PBR set.
    const [bc, nrm] = await seed(["MatEdit_BaseColor.png", "MatEdit_Normal.png"]);
    expect(bc && nrm).toBeTruthy();

    await api.importMaterial("/lib/MatEdit_Material");
    const mats = await api.listMaterials(null);
    const mat = mats[0];
    expect(mat).toBeTruthy();
    expect(mat.is_pbr).toBe(false);
    const startHealth = mat.health;

    // Now bring in two loose roughness textures to add/swap with.
    const [rough, rough2] = await seed(["Loose_Roughness.png", "Other_Roughness.png"]);
    expect(rough && rough2).toBeTruthy();

    // ADD roughness → completes PBR, health rises, renderers gain vray/arnold.
    await api.setMaterialMap(mat.id, "roughness", rough);
    let got = (await api.getMaterial(mat.id))!;
    expect(got.is_pbr).toBe(true);
    expect(got.health).toBeGreaterThan(startHealth);
    expect(got.renderers).toContain("vray");
    expect(got.maps.some((m) => m.slot === "roughness" && m.texture_id === rough)).toBe(true);

    // SWAP roughness → same slot count, new texture id.
    await api.setMaterialMap(mat.id, "roughness", rough2);
    got = (await api.getMaterial(mat.id))!;
    expect(got.maps.filter((m) => m.slot === "roughness").length).toBe(1);
    expect(got.maps.some((m) => m.slot === "roughness" && m.texture_id === rough2)).toBe(true);

    // REMOVE base_color → drops PBR and the base-color-anchored renderers.
    await api.setMaterialMap(mat.id, "base_color", null);
    got = (await api.getMaterial(mat.id))!;
    expect(got.is_pbr).toBe(false);
    expect(got.maps.some((m) => m.slot === "base_color")).toBe(false);
    expect(got.renderers).not.toContain("vray");
    expect(got.renderers).toContain("generic_pbr");
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
