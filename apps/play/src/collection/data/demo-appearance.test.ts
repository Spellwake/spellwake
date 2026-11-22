import { describe, expect, test } from "bun:test";
import { demoAppearanceSchema } from "./demo-model";

describe("demo deck appearance", () => {
  test("reads a save from before face cards, dropping the retired cover art", () => {
    expect(
      demoAppearanceSchema.parse({ colors: ["R"], artwork: "dragon" }),
    ).toEqual({ colors: ["R"], faceCard: null });
  });
  test("keeps a chosen face card", () => {
    expect(
      demoAppearanceSchema.parse({ colors: ["G"], faceCard: "Llanowar Elves" }),
    ).toEqual({ colors: ["G"], faceCard: "Llanowar Elves" });
  });
  test("rejects anything else", () => {
    expect(
      demoAppearanceSchema.safeParse({ colors: [], faceCard: null, cover: 1 })
        .success,
    ).toBe(false);
  });
});
