import { describe, expect, test } from "bun:test";
import { scryfallImage } from "./scryfall-images";

describe("scryfallImage", () => {
  const id = "0000579f-7b35-4ed3-b44c-db2a538066fe";
  test("builds Scryfall's image path from the card ID", () => {
    expect(scryfallImage(id, "normal")).toBe(
      `https://cards.scryfall.io/normal/front/0/0/${id}.jpg`,
    );
    expect(scryfallImage(id, "art_crop")).toBe(
      `https://cards.scryfall.io/art_crop/front/0/0/${id}.jpg`,
    );
  });
  test("refuses anything that is not a card ID", () => {
    expect(scryfallImage("../etc", "normal")).toBeNull();
    expect(scryfallImage(id.toUpperCase(), "normal")).toBeNull();
  });
});
