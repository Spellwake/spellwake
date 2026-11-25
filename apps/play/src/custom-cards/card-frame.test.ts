import { describe, expect, test } from "bun:test";
import {
  CARD_HEIGHT,
  CARD_WIDTH,
  parseManaTokens,
  renderCardFrameSvg,
  resolveFramePalette,
} from "../../../../packages/ui/src/cards/card-frame-svg";
import {
  lookupManaSymbolSvg,
  SCRYFALL_SYMBOL_SVGS,
} from "../../../../packages/ui/src/cards/mana-symbols";

describe("unified 300-DPI SVG card frame engine", () => {
  test("bundles all 84 clean Scryfall vector symbols", () => {
    expect(Object.keys(SCRYFALL_SYMBOL_SVGS).length).toBe(84);
    expect(lookupManaSymbolSvg("{U}")).toContain("<svg");
    expect(lookupManaSymbolSvg("{R}")).toContain("<svg");
    expect(lookupManaSymbolSvg("{T}")).toContain("<svg");
    expect(lookupManaSymbolSvg("{W/U}")).toContain("<svg");
  });

  test("resolves dual-color and explicit accent override palettes cleanly", () => {
    const izzet = resolveFramePalette(
      {
        name: "Aetherblade Vanguard",
        manaCost: "{2}{U}{R}",
        typeLine: "Creature \u2014 Human Artificer",
        oracleText: "Flying, haste",
      },
      "auto",
    );
    expect(izzet.key).toBe("UR");

    const forcedGold = resolveFramePalette(
      {
        name: "Aetherblade Vanguard",
        manaCost: "{2}{U}{R}",
        typeLine: "Creature \u2014 Human Artificer",
        oracleText: "Flying, haste",
      },
      "gold",
    );
    expect(forcedGold.key).toBe("gold");
  });

  test("renders 1062x1482 normal, planeswalker, saga, and stack_trigger SVGs safely", () => {
    expect(parseManaTokens("{2}{U}{R}")).toEqual(["{2}", "{U}", "{R}"]);

    const creatureSvg = renderCardFrameSvg({
      kind: "card_face",
      layout: "normal",
      rarity: "rare",
      face: {
        name: "Aetherblade Vanguard",
        manaCost: "{2}{U}{R}",
        typeLine: "Creature \u2014 Human Artificer",
        oracleText:
          "Flying, haste\nWhen Aetherblade Vanguard enters, draw a card.",
        flavorText: "Forged in the quiet storm.",
        power: "3",
        toughness: "2",
        artist: "Elena Vance",
      },
    });
    expect(creatureSvg).toContain(`viewBox="0 0 ${CARD_WIDTH} ${CARD_HEIGHT}"`);
    expect(creatureSvg).toContain("Aetherblade Vanguard");
    expect(creatureSvg).toContain("3/2");
    expect(creatureSvg).toContain("Elena Vance");

    const pwSvg = renderCardFrameSvg({
      kind: "card_face",
      layout: "planeswalker",
      rarity: "mythic",
      face: {
        name: "Kael, Rift Weaver",
        manaCost: "{3}{U}",
        typeLine: "Legendary Planeswalker \u2014 Kael",
        oracleText:
          "+1: Draw a card.\n\u22122: Return target creature to its owner's hand.",
        loyalty: "4",
      },
    });
    expect(pwSvg).toContain("Kael, Rift Weaver");
    expect(pwSvg).toContain(">4<");

    const sagaSvg = renderCardFrameSvg({
      kind: "card_face",
      layout: "saga",
      rarity: "rare",
      face: {
        name: "Chronicle of the Spire",
        manaCost: "{2}{W}",
        typeLine: "Enchantment \u2014 Saga",
        oracleText:
          "I \u2014 Create a 1/1 Soldier token.\nII \u2014 Draw a card.",
      },
    });
    expect(sagaSvg).toContain("Chronicle of the Spire");
    expect(sagaSvg).toContain(">II<");

    const triggerSvg = renderCardFrameSvg({
      kind: "stack_trigger",
      name: "Aetherblade Vanguard",
      typeLabel: "Triggered ability",
      effect: "When Aetherblade Vanguard enters, draw a card.",
      illustrated: false,
    });
    expect(triggerSvg).toContain("Triggered ability");
    expect(triggerSvg).toContain("Aetherblade Vanguard");

    // XML escapes untrusted angle brackets so no injection is possible.
    const xssSvg = renderCardFrameSvg({
      kind: "card_face",
      layout: "normal",
      rarity: "common",
      face: {
        name: "<script>alert(1)</script>",
        typeLine: "Instant",
        oracleText: "Draw a card.",
      },
    });
    expect(xssSvg).not.toContain("<script>");
    expect(xssSvg).toContain("&lt;script&gt;");
  });
});
