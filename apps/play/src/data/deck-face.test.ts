import { describe, expect, test } from "bun:test";
import { faceCardOf } from "./deck-face";

describe("faceCardOf", () => {
  const cards = ["Goblin Guide", "Krenko, Mob Boss", "Lightning Bolt"];
  test("shows the chosen face card while it is in the deck", () => {
    expect(faceCardOf("Krenko, Mob Boss", cards)).toBe("Krenko, Mob Boss");
  });
  test("falls back to the first card added when none is chosen", () => {
    expect(faceCardOf(null, cards)).toBe("Goblin Guide");
  });
  test("falls back when the chosen card has left the deck", () => {
    expect(faceCardOf("Shock", cards)).toBe("Goblin Guide");
  });
  test("an empty deck has no face card", () => {
    expect(faceCardOf(null, [])).toBeNull();
  });
});
