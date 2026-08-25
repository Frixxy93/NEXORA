import { describe, it, expect } from "vitest";
import { formatBytes, resolutionLabel, mapLabel } from "./format";

describe("formatBytes", () => {
  it("handles empty values", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(0)).toBe("0 B");
  });

  it("scales through units", () => {
    expect(formatBytes(500)).toBe("500 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(15 * 1024)).toBe("15 KB"); // >= 10 drops the decimal
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe("3.0 GB");
  });
});

describe("resolutionLabel", () => {
  it("names standard square resolutions", () => {
    expect(resolutionLabel(1024, 1024)).toBe("1K");
    expect(resolutionLabel(2048, 2048)).toBe("2K");
    expect(resolutionLabel(4096, 4096)).toBe("4K");
    expect(resolutionLabel(8192, 8192)).toBe("8K");
  });

  it("shows dimensions for non-standard or non-square", () => {
    expect(resolutionLabel(2048, 1024)).toBe("2048 × 1024");
    expect(resolutionLabel(3000, 3000)).toBe("3000 × 3000");
  });

  it("returns null when a dimension is missing", () => {
    expect(resolutionLabel(null, 1024)).toBeNull();
    expect(resolutionLabel(1024, undefined)).toBeNull();
  });
});

describe("mapLabel", () => {
  it("prettifies known slugs", () => {
    expect(mapLabel("base_color")).toBe("Base Color");
    expect(mapLabel("ao")).toBe("Ambient Occlusion");
    expect(mapLabel("normal")).toBe("Normal");
  });

  it("falls back for unknown/empty", () => {
    expect(mapLabel(null)).toBe("Unclassified");
    expect(mapLabel(undefined)).toBe("Unclassified");
    expect(mapLabel("custom:foo")).toBe("custom:foo");
  });
});
