import { describe, it, expect } from "vitest";
import { viewKind } from "./viewKind";

describe("viewKind", () => {
  it("routes the library + material roots", () => {
    expect(viewKind("lib.textures")).toEqual({ t: "library" });
    expect(viewKind("lib.materials")).toEqual({ t: "materials" });
    expect(viewKind("mtype.pbr")).toEqual({ t: "materials-pbr" });
    expect(viewKind("mtype.udim")).toEqual({ t: "udim" });
  });

  it("routes texture types by prefix, carrying the slug", () => {
    expect(viewKind("ttype.base_color")).toEqual({ t: "textures", slug: "base_color" });
    expect(viewKind("ttype.normal")).toEqual({ t: "textures", slug: "normal" });
    expect(viewKind("ttype.other")).toEqual({ t: "textures", slug: "other" });
  });

  it("routes renderer-scoped material views", () => {
    expect(viewKind("mtype.vray")).toEqual({ t: "materials-renderer", renderer: "vray" });
    expect(viewKind("mtype.arnold")).toEqual({ t: "materials-renderer", renderer: "arnold" });
  });

  it("routes smart views", () => {
    expect(viewKind("smart.favorites")).toEqual({ t: "mixed", source: "favorites" });
    expect(viewKind("smart.recent_added")).toEqual({ t: "mixed", source: "recent_added" });
    expect(viewKind("smart.recent_used")).toEqual({ t: "mixed", source: "recent_used" });
    expect(viewKind("smart.duplicates")).toEqual({ t: "duplicates" });
    expect(viewKind("smart.missing_files")).toEqual({ t: "missing_files" });
    expect(viewKind("collections")).toEqual({ t: "collections" });
  });

  it("returns null for unknown views (e.g. home/settings)", () => {
    expect(viewKind("home")).toBeNull();
    expect(viewKind("settings")).toBeNull();
    expect(viewKind("something.unknown")).toBeNull();
  });
});
