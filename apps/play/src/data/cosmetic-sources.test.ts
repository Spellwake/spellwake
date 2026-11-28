import { expect, test } from "bun:test";
import {
  firstChoice,
  matching,
  sameSelection,
  type CosmeticItem,
  type DefaultSelection,
} from "../cosmetics/cosmetic-sources";

const items: CosmeticItem[] = [
  { id: "s1", name: "Classic", kind: "sleeve", look: null },
  { id: "s2", name: "Midnight", kind: "sleeve", look: null },
  { id: "d1", name: "Midnight", kind: "dice", look: null },
];
const selection: DefaultSelection = {
  sleeve: "s1",
  back: "b1",
  dice: "d1",
  playmat: null,
  card_effect: "ce1",
  sleeve_effect: "se1",
  playmat_effect: "pe1",
  pet: null,
};

test("search matches names within one category, in the source's order", () => {
  expect(matching(items, "sleeve", "").map((item) => item.id)).toEqual([
    "s1",
    "s2",
  ]);
  expect(matching(items, "sleeve", "  MID ").map((item) => item.id)).toEqual([
    "s2",
  ]);
  expect(matching(items, "dice", "classic")).toEqual([]);
  expect(matching(items, "playmat", "")).toEqual([]);
});

test("the first choice of an empty category is None", () => {
  expect(firstChoice(items, "sleeve")).toBe("s1");
  expect(firstChoice(items, "pet")).toBeNull();
});

test("selections are compared only in the categories a source has", () => {
  const other = { ...selection, pet: "p1" };
  expect(sameSelection(selection, other, ["sleeve", "back", "dice"])).toBe(
    true,
  );
  expect(sameSelection(selection, other, ["sleeve", "pet"])).toBe(false);
  expect(
    sameSelection(selection, { ...selection, playmat: "m1" }, ["playmat"]),
  ).toBe(false);
});
