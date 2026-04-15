import { describe, it, expect } from "vitest";
import { getDisplayName, formatList, sum } from "../src/index.js";

describe("mixed-project TypeScript", () => {
  it("getDisplayName returns name field", () => {
    expect(getDisplayName({ name: "alice" })).toBe("alice");
  });

  it("formatList produces bullet points", () => {
    const result = formatList(["a", "b", "c"]);
    expect(result).toContain("•");
    expect(result.split("\n")).toHaveLength(3);
  });

  it("sum adds numbers correctly", () => {
    expect(sum([1, 2, 3, 4])).toBe(10);
    expect(sum([])).toBe(0);
  });

  // INTENTIONALLY FAILING — .skip() so CI does not run it.
  // This test represents a "known broken" scenario the fixture documents.
  it.skip("intentionally failing — fixture defect demonstration", () => {
    throw new Error(
      "This test is intentionally failing. The mixed-project fixture contains broken tests by design.",
    );
  });
});
