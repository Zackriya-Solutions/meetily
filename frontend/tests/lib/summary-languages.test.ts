import { describe, expect, test } from "bun:test";

import {
  LANGUAGE_OPTIONS,
  labelForCode,
  normaliseLanguageCode,
} from "../../src/lib/summary-languages";

describe("summary language options", () => {
  test("rejects unknown languages instead of guessing", () => {
    expect(normaliseLanguageCode("xx")).toBeNull();
    expect(normaliseLanguageCode("urdu")).toBeNull();
  });

  test("returns null for empty input", () => {
    expect(normaliseLanguageCode("")).toBeNull();
    expect(normaliseLanguageCode(null)).toBeNull();
    expect(normaliseLanguageCode(undefined)).toBeNull();
  });

  test("offers Urdu in the summary language picker list", () => {
    const urdu = LANGUAGE_OPTIONS.find((option) => option.code === "ur");
    expect(urdu?.label).toBe("Urdu");
  });

  test("normalises Urdu variants to the ur code", () => {
    expect(normaliseLanguageCode("ur")).toBe("ur");
    expect(normaliseLanguageCode("UR")).toBe("ur");
    expect(normaliseLanguageCode("ur-PK")).toBe("ur");
    expect(normaliseLanguageCode("ur_PK")).toBe("ur");
  });

  test("labels the ur code as Urdu", () => {
    expect(labelForCode("ur")).toBe("Urdu");
  });

  test("every listed language normalises to itself", () => {
    for (const option of LANGUAGE_OPTIONS) {
      expect(normaliseLanguageCode(option.code)).toBe(option.code);
    }
  });
});
