import { describe, expect, test } from "bun:test";
import { parseStrictJson } from "./raw-json.ts";
describe("original JSON before schema validation", () => {
  test("retains ordinary values without object-key normalization", () => {
    for (const value of [
      null,
      true,
      3,
      "🂡",
      { a: [{ b: '\\"' }], __protoName: "safe" },
    ]) {
      expect(parseStrictJson(JSON.stringify(value))).toEqual({
        success: true,
        value,
      });
    }
    expect(parseStrictJson('{"a":{"same":1},"b":{"same":2}}')).toEqual({
      success: true,
      value: { a: { same: 1 }, b: { same: 2 } },
    });
  });
  test("rejects equal, conflicting and escaped duplicate keys at every nesting level", () => {
    for (const text of [
      '{"a":1,"a":1}',
      '{"a":1,"\\u0061":2}',
      '{"action":{"kind":"pass","kind":"choose"}}',
      '{"items":[{"value":"0","value":"0"}]}',
      '{"cursor":{"after":"1","after":"2"}}',
    ]) {
      expect(parseStrictJson(text)).toEqual({ success: false });
    }
  });
  test("enforces JSON syntax, scalar Unicode, size and depth independently", () => {
    for (const text of [
      "",
      "{a:1}",
      '{"a":1,}',
      "[1,]",
      "01",
      "true false",
      '"\\ud800"',
      '"\\udfff"',
      '"\\ud800x"',
      "/*x*/null",
      '"\n"',
    ]) {
      expect(parseStrictJson(text)).toEqual({ success: false });
    }
    expect(parseStrictJson('"\\ud83c\\udca1"')).toEqual({
      success: true,
      value: "🂡",
    });
    expect(parseStrictJson('"🂡"', 5)).toEqual({ success: false });
    expect(parseStrictJson("[".repeat(66) + "0" + "]".repeat(66))).toEqual({
      success: false,
    });
  });
});
