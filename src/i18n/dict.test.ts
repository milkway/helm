import { describe, expect, it } from "vitest";
import { dict } from "./dict";

const languages = ["en", "pt", "fr", "es"] as const;

describe("translation dictionary", () => {
  it("has exactly the same keys in every supported language", () => {
    const referenceKeys = Object.keys(dict.en).sort();

    expect(Object.keys(dict).sort()).toEqual([...languages].sort());

    for (const code of languages) {
      expect(Object.keys(dict[code]).sort(), `translation keys for ${code}`).toEqual(referenceKeys);
    }
  });
});
