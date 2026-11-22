import { expect, test } from "bun:test";
import {
  deckTextFromFile,
  isDeckLink,
  textFromCockatriceDeck,
  textFromMtgoDeck,
} from "./deck-files";
import { readDeckIntake } from "./deck-text";

test("a Magic Online deck reads as the same list pasted", () => {
  const xml = `<?xml version="1.0" encoding="utf-8"?>
<Deck xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <NetDeckID>0</NetDeckID>
  <Cards CatID="1" Quantity="4" Sideboard="false" Name="Lightning Bolt" Annotation="0" />
  <Cards CatID="2" Quantity="20" Sideboard="false" Name="Mountain" />
  <Cards CatID="3" Quantity="2" Sideboard="true" Name="Krenko&apos;s Command" />
  <Cards CatID="4" Quantity="x" Sideboard="false" Name="Broken" />
</Deck>`;
  expect(textFromMtgoDeck(xml)).toBe(
    "4 Lightning Bolt\n20 Mountain\n\nSideboard\n2 Krenko's Command",
  );
  const { lines } = readDeckIntake(textFromMtgoDeck(xml) ?? "");
  expect(lines.map((line) => [line.quantity, line.name, line.section])).toEqual(
    [
      ["4", "Lightning Bolt", "main"],
      ["20", "Mountain", "main"],
      ["2", "Krenko's Command", "sideboard"],
    ],
  );
});

test("a Cockatrice deck keeps its zones and leaves out tokens", () => {
  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<cockatrice_deck version="1">
  <deckname>Goblins</deckname>
  <zone name="main">
    <card number="4" name="Goblin Guide"/>
    <card number='1' name='Krenko, Mob Boss'/>
  </zone>
  <zone name="side"><card number="3" name="Shock"/></zone>
  <zone name="tokens"><card number="1" name="Goblin"/></zone>
</cockatrice_deck>`;
  expect(textFromCockatriceDeck(xml)).toBe(
    "4 Goblin Guide\n1 Krenko, Mob Boss\n\nSideboard\n3 Shock",
  );
});

test("a file is read by its kind, and anything else is refused", () => {
  expect(deckTextFromFile("list.TXT", "4 Shock")).toEqual({
    status: "read",
    text: "4 Shock",
  });
  expect(deckTextFromFile("deck.dek", "not a deck").status).toBe("unsupported");
  expect(deckTextFromFile("deck.cod", "<Deck/>").status).toBe("unsupported");
  expect(deckTextFromFile("deck.json", "{}").status).toBe("unsupported");
  expect(deckTextFromFile("noextension", "4 Shock").status).toBe("unsupported");
});

test("only public Moxfield and TappedOut deck addresses are accepted", () => {
  expect(isDeckLink("https://moxfield.com/decks/abc123")).toBe(true);
  expect(isDeckLink(" https://www.moxfield.com/decks/abc123 ")).toBe(true);
  expect(isDeckLink("https://tappedout.net/mtg-decks/goblins/")).toBe(true);
  expect(isDeckLink("https://example.com/deck")).toBe(false);
  expect(isDeckLink("http://moxfield.com/decks/abc123")).toBe(false);
  expect(isDeckLink("https://moxfield.com/users/someone")).toBe(false);
  expect(isDeckLink("https://user:pw@moxfield.com/decks/abc")).toBe(false);
  expect(isDeckLink("moxfield.com/decks/abc")).toBe(false);
});
