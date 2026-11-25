/**
 * Preview / offline demo store and deterministic client-side fallback standardizer &
 * live diagnostics engine for `useIsPreview()` mode in Spellwake Custom Card Studio.
 *
 * Connected sessions always compile, standardize, and persist against the authoritative
 * Rust backend (`services/application`).
 */

import type {
  CustomCardColor,
  CustomCardCompileResult,
  CustomCardFaceRules,
  CustomCardFrameAccent,
  CustomCardLayout,
  CustomCardRarity,
  CustomCardSummary,
  StandardizeResult,
} from "@spellwake/application-contracts";

export interface DemoVaultCard extends CustomCardSummary {
  /** Local preview art data URLs keyed by face index (`0 | 1`). */
  demoArtUrls?: Partial<Record<0 | 1, string>>;
}

function makeDemoArtDataUrl(
  hueA: string,
  hueB: string,
  accent: string,
  emblemPath: string,
): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 894 654">
    <defs>
      <linearGradient id="sky" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stop-color="${hueA}"/>
        <stop offset="55%" stop-color="${hueB}"/>
        <stop offset="100%" stop-color="#0b1019"/>
      </linearGradient>
      <radialGradient id="glow" cx="52%" cy="42%" r="48%">
        <stop offset="0%" stop-color="${accent}" stop-opacity="0.78"/>
        <stop offset="55%" stop-color="${accent}" stop-opacity="0.20"/>
        <stop offset="100%" stop-color="#090c12" stop-opacity="0"/>
      </radialGradient>
    </defs>
    <rect width="894" height="654" fill="url(#sky)"/>
    <rect width="894" height="654" fill="url(#glow)"/>
    <g stroke="${accent}" stroke-opacity="0.32" fill="none" stroke-width="2">
      <circle cx="447" cy="296" r="196"/>
      <circle cx="447" cy="296" r="138" stroke-dasharray="10 8"/>
      <line x1="90" y1="540" x2="804" y2="540"/>
    </g>
    <path d="${emblemPath}" fill="${accent}" fill-opacity="0.26" stroke="${accent}" stroke-width="4" stroke-linejoin="round"/>
  </svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

export const INITIAL_DEMO_CUSTOM_CARDS: DemoVaultCard[] = [
  {
    id: "10000000-0000-4000-8000-000000000101",
    archivedAt: null,
    createdAt: "2026-11-24T18:00:00Z",
    updatedAt: "2026-11-24T18:05:00Z",
    demoArtUrls: {
      0: makeDemoArtDataUrl(
        "#122b4a",
        "#4d1b18",
        "#f0b95c",
        "M 447 110 L 538 286 L 447 478 L 356 286 Z M 248 318 L 646 254 L 447 356 Z",
      ),
    },
    currentRevision: {
      revisionId: "20000000-0000-4000-8000-000000000101",
      customCardId: "10000000-0000-4000-8000-000000000101",
      revisionNumber: "3",
      rulesSha256:
        "a1b2c3d4e5f60718293a4b5c6d7e8f9011223344556677889900aabbccddeeff",
      name: "Aetherblade Vanguard",
      layout: "normal",
      rarity: "rare",
      faces: [
        {
          name: "Aetherblade Vanguard",
          manaCost: "{2}{U}{R}",
          typeLine: "Creature \u2014 Human Artificer",
          oracleText:
            "Flying, haste\nWhen Aetherblade Vanguard enters, draw a card.\nWhenever you cast an instant or sorcery spell, Aetherblade Vanguard gets +1/+0 until end of turn.",
          power: "3",
          toughness: "2",
          loyalty: null,
          defense: null,
          colorIndicator: null,
        },
      ],
      visualFrameAccents: ["auto"],
      flavorTexts: [
        "Every spark in the forge is a promise kept at the speed of lightning.",
      ],
      collectorNote: "014/250",
      stage1Parsed: true,
      stage2Playable: true,
      compilerRevision: "keyword-gameplay-current-v18-oracle-compiler-v1",
      createdAt: "2026-11-24T18:05:00Z",
      art: [
        {
          faceIndex: 0,
          mimeType: "image/webp",
          artSha256:
            "0000000000000000000000000000000000000000000000000000000000000101",
          hasArt: true,
          cropX: 0,
          cropY: -0.05,
          zoom: 1,
          artist: "Elena Vance",
        },
      ],
    },
  },
  {
    id: "10000000-0000-4000-8000-000000000102",
    archivedAt: null,
    createdAt: "2026-11-24T17:10:00Z",
    updatedAt: "2026-11-24T17:12:00Z",
    demoArtUrls: {
      0: makeDemoArtDataUrl(
        "#1b2d47",
        "#1f4a3c",
        "#91d6be",
        "M 447 132 L 560 330 L 447 458 L 334 330 Z",
      ),
      1: makeDemoArtDataUrl(
        "#3c1824",
        "#181226",
        "#df8464",
        "M 310 172 L 584 172 L 447 468 Z",
      ),
    },
    currentRevision: {
      revisionId: "20000000-0000-4000-8000-000000000102",
      customCardId: "10000000-0000-4000-8000-000000000102",
      revisionNumber: "2",
      rulesSha256:
        "b1b2c3d4e5f60718293a4b5c6d7e8f9011223344556677889900aabbccddeef2",
      name: "Vesper, Lantern Watcher // Vesper, Eclipse Wraith",
      layout: "transform",
      rarity: "mythic",
      faces: [
        {
          name: "Vesper, Lantern Watcher",
          manaCost: "{1}{W}{B}",
          typeLine: "Legendary Creature \u2014 Spirit Cleric",
          oracleText:
            "Vigilance, lifelink\nWhen Vesper, Lantern Watcher enters the battlefield, scry 2 and transform this card if you gained 3 or more life.",
          power: "2",
          toughness: "4",
          loyalty: null,
          defense: null,
          colorIndicator: null,
        },
        {
          name: "Vesper, Eclipse Wraith",
          manaCost: null,
          typeLine: "Legendary Creature \u2014 Wraith",
          oracleText:
            "Flying, deathtouch\nWhenever Vesper, Eclipse Wraith deals combat damage to a player, draw a card and lose 1 life.",
          power: "4",
          toughness: "4",
          loyalty: null,
          defense: null,
          colorIndicator: ["B"],
        },
      ],
      visualFrameAccents: ["auto", "B"],
      flavorTexts: [
        "She keeps the wick lit until the final bell.",
        "No lantern outlasts the night forever.",
      ],
      collectorNote: "019/250",
      stage1Parsed: true,
      stage2Playable: false,
      compilerRevision: "keyword-gameplay-current-v18-oracle-compiler-v1",
      createdAt: "2026-11-24T17:12:00Z",
      art: [
        {
          faceIndex: 0,
          mimeType: "image/webp",
          artSha256:
            "0000000000000000000000000000000000000000000000000000000000000102",
          hasArt: true,
          cropX: 0,
          cropY: 0,
          zoom: 1,
          artist: "Marek Solis",
        },
      ],
    },
  },
  {
    id: "10000000-0000-4000-8000-000000000103",
    archivedAt: null,
    createdAt: "2026-11-23T22:40:00Z",
    updatedAt: "2026-11-23T22:40:00Z",
    demoArtUrls: {
      0: makeDemoArtDataUrl(
        "#4d2515",
        "#25140f",
        "#f6a652",
        "M 447 120 L 502 274 L 620 274 L 520 356 L 560 490 L 447 408 L 334 490 L 374 356 L 274 274 L 392 274 Z",
      ),
    },
    currentRevision: {
      revisionId: "20000000-0000-4000-8000-000000000103",
      customCardId: "10000000-0000-4000-8000-000000000103",
      revisionNumber: "1",
      rulesSha256:
        "c1b2c3d4e5f60718293a4b5c6d7e8f9011223344556677889900aabbccddeef3",
      name: "Emberburst Relay",
      layout: "normal",
      rarity: "uncommon",
      faces: [
        {
          name: "Emberburst Relay",
          manaCost: "{1}{R}",
          typeLine: "Instant",
          oracleText: "Emberburst Relay deals 3 damage to any target. Scry 1.",
          power: null,
          toughness: null,
          loyalty: null,
          defense: null,
          colorIndicator: null,
        },
      ],
      visualFrameAccents: ["R"],
      flavorTexts: [
        "Three sparks to light the fuse, one glance to watch it fly.",
      ],
      collectorNote: "007/250",
      stage1Parsed: true,
      stage2Playable: true,
      compilerRevision: "keyword-gameplay-current-v18-oracle-compiler-v1",
      createdAt: "2026-11-23T22:40:00Z",
      art: [],
    },
  },
];

/** Deterministic offline standardizer mirroring `crates/oracle_compiler/src/standardize.rs`. */
export function demoStandardizeFaces(
  faces: CustomCardFaceRules[],
): StandardizeResult {
  const changes: StandardizeResult["changes"] = [];
  const nextFaces = faces.map((face, faceIndex) => {
    const next: CustomCardFaceRules = { ...face };
    if (next.manaCost) {
      const normalized = next.manaCost.replace(
        /\{([^}]+)\}/g,
        (_, sym: string) => `{${sym.trim().toUpperCase()}}`,
      );
      if (normalized !== next.manaCost) {
        changes.push({
          faceIndex,
          before: next.manaCost,
          after: normalized,
          reason: "Normalize mana symbol casing",
        });
        next.manaCost = normalized;
      }
    }
    if (next.typeLine.includes(" - ")) {
      const normalized = next.typeLine.replace(/\s+-\s+/g, " \u2014 ");
      changes.push({
        faceIndex,
        before: next.typeLine,
        after: normalized,
        reason: "Use standard Magic type-line separator",
      });
      next.typeLine = normalized;
    }
    const rules: Array<[RegExp, string, string]> = [
      [
        /enters the battlefield/gi,
        "enters",
        "Use modern Foundations 'enters' phrasing",
      ],
      [/to your mana pool/gi, "", "Remove legacy 'to your mana pool' wording"],
      [/shuffle your library/gi, "shuffle", "Use modern 'shuffle' wording"],
      [
        /to target creature or player/gi,
        "to any target",
        "Use modern 'any target' damage phrasing",
      ],
      [/\{t\}/g, "{T}", "Capitalize tap symbol {T}"],
      [/~/g, next.name || "this card", "Expand card-name placeholder"],
    ];
    let text = next.oracleText;
    for (const [pattern, replacement, reason] of rules) {
      if (pattern.test(text)) {
        const before = text;
        text = text
          .replace(pattern, replacement)
          .replace(/  +/g, " ")
          .replace(/ \./g, ".");
        if (text !== before) {
          changes.push({ faceIndex, before, after: text, reason });
        }
      }
    }
    next.oracleText = text.trim();
    return next;
  });
  return { faces: nextFaces, changes, changed: changes.length > 0 };
}

/** Deterministic offline preview compiler so `useIsPreview()` reflects real AST chips and warnings. */
export function demoCompileCard(input: {
  layout: CustomCardLayout;
  rarity: CustomCardRarity;
  faces: CustomCardFaceRules[];
}): CustomCardCompileResult {
  const diagnostics: CustomCardCompileResult["diagnostics"] = [];
  const astReading: CustomCardCompileResult["astReading"] = [];
  const colorSet = new Set<CustomCardColor>();
  let manaValue = 0;

  input.faces.forEach((face, faceIndex) => {
    if (!face.name.trim()) {
      diagnostics.push({
        severity: "error",
        stage: "validation",
        faceIndex,
        code: "missing-name",
        message: "Card name is required.",
        clauseText: null,
      });
    }
    if (!face.typeLine.trim()) {
      diagnostics.push({
        severity: "error",
        stage: "validation",
        faceIndex,
        code: "missing-type-line",
        message: "Type line is required.",
        clauseText: null,
      });
    }
    if (face.typeLine.includes(" - ")) {
      diagnostics.push({
        severity: "warning",
        stage: "stage1",
        faceIndex,
        code: "legacy-type-dash",
        message:
          "Type line uses a hyphen instead of the standard separator. Click Standardize phrasing.",
        clauseText: face.typeLine,
      });
    }
    if (/enters the battlefield/i.test(face.oracleText)) {
      diagnostics.push({
        severity: "warning",
        stage: "stage1",
        faceIndex,
        code: "legacy-etb-wording",
        message:
          "Legacy 'enters the battlefield' wording detected. Standardize to modern 'enters'.",
        clauseText: "enters the battlefield",
      });
    }
    const mana = face.manaCost ?? "";
    const tokRegex = /\{([^}]+)\}/g;
    let m: RegExpExecArray | null;
    while ((m = tokRegex.exec(mana)) !== null) {
      const t = (m[1] ?? "").toUpperCase();
      if (/^\d+$/.test(t)) {
        if (faceIndex === 0) manaValue += Number(t);
      } else if (t !== "X" && t !== "Y" && t !== "Z") {
        if (faceIndex === 0) manaValue += 1;
      }
      for (const p of t.split("/")) {
        if (p === "W" || p === "U" || p === "B" || p === "R" || p === "G") {
          colorSet.add(p);
        }
      }
    }
    for (const c of face.colorIndicator ?? []) {
      colorSet.add(c);
    }

    const lines = face.oracleText
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    for (const line of lines) {
      if (
        /^(flying|haste|vigilance|lifelink|deathtouch|trample|reach|first strike|flash|menace|hexproof|indestructible|defender)(,\s*(flying|haste|vigilance|lifelink|deathtouch|trample|reach|first strike|flash|menace|hexproof|indestructible|defender))*$/i.test(
          line,
        )
      ) {
        for (const kw of line.split(",")) {
          const clean = kw.trim();
          const titled = clean.charAt(0).toUpperCase() + clean.slice(1).toLowerCase();
          astReading.push({
            faceIndex,
            kind: "keyword",
            label: titled,
            summary: titled,
          });
        }
      } else if (/^(when|whenever|at the beginning)/i.test(line)) {
        astReading.push({
          faceIndex,
          kind: "triggered",
          label: "Triggered",
          summary: line,
        });
        if (/transform this card if you gained/i.test(line)) {
          diagnostics.push({
            severity: "error",
            stage: "stage2",
            faceIndex,
            code: "stage2-unsupported-conditional-transform",
            message:
              "Conditional life-threshold transform clause is not lowered by the current gameplay profile yet. Simplify or split into a separate triggered ability.",
            clauseText: line,
          });
        }
      } else if (/^[+\u2212-]?\d+:|^[+\u2212-]?X:/i.test(line)) {
        const prefix = line.split(":")[0];
        astReading.push({
          faceIndex,
          kind: "loyalty",
          label: `Loyalty ${prefix}`,
          summary: line,
        });
      } else if (line.includes(":")) {
        astReading.push({
          faceIndex,
          kind: "activated",
          label: "Activated",
          summary: line,
        });
      } else if (
        /deals \d+ damage|draw a card|scry \d+|destroy target|counter target/i.test(
          line,
        )
      ) {
        astReading.push({
          faceIndex,
          kind: "spell_effect",
          label: "Spell Effect",
          summary: line,
        });
      } else {
        diagnostics.push({
          severity: "error",
          stage: "stage2",
          faceIndex,
          code: "unlowered-clause",
          message: `Preview engine could not lower clause yet: "${line}"`,
          clauseText: line,
        });
      }
    }
  });

  const hasStage1Error = diagnostics.some(
    (d) => d.severity === "error" && d.stage !== "stage2",
  );
  const hasStage2Blocker = diagnostics.some((d) => d.stage === "stage2");
  const stage1Parsed = !hasStage1Error;
  const stage2Playable = stage1Parsed && !hasStage2Blocker;
  const orderedColors: CustomCardColor[] = ["W", "U", "B", "R", "G"];

  // Deterministic preview rules fingerprint (real SHA-256 is computed in Rust when connected).
  const canonicalText = JSON.stringify({ layout: input.layout, faces: input.faces });
  let h1 = 0x811c9dc5;
  let h2 = 0x9e3779b9;
  for (let i = 0; i < canonicalText.length; i += 1) {
    const ch = canonicalText.charCodeAt(i);
    h1 = Math.imul(h1 ^ ch, 0x01000193) >>> 0;
    h2 = Math.imul(h2 ^ (ch + i), 0x85ebca6b) >>> 0;
  }
  const previewFingerprint = `${h1.toString(16).padStart(8, "0")}${h2.toString(16).padStart(8, "0")}`.repeat(4);

  return {
    rulesSha256: previewFingerprint,
    compilerRevision: "keyword-gameplay-current-v18-oracle-compiler-v1",
    stage1Parsed,
    stage2Playable,
    diagnostics,
    astReading,
    colorIdentity: orderedColors.filter((c) => colorSet.has(c)),
    manaValue,
  };
}

export interface StudioDraftFace {
  name: string;
  manaCost: string | null;
  typeLine: string;
  oracleText: string;
  power: string | null;
  toughness: string | null;
  loyalty: string | null;
  defense: string | null;
  colorIndicator: CustomCardColor[] | null;
  flavorText: string;
  frameAccent: CustomCardFrameAccent;
  artist: string;
  artUrl: string | null;
  artMimeType: "image/webp" | "image/png" | null;
  artBase64: string | null;
  copyFromRevisionId: string | null;
  cropX: number;
  cropY: number;
  zoom: number;
}

export function createEmptyDraftFace(name = ""): StudioDraftFace {
  return {
    name,
    manaCost: "{1}{U}",
    typeLine: "Creature \u2014 Wizard",
    oracleText: "Flying",
    power: "2",
    toughness: "1",
    loyalty: null,
    defense: null,
    colorIndicator: null,
    flavorText: "",
    frameAccent: "auto",
    artist: "",
    artUrl: null,
    artMimeType: null,
    artBase64: null,
    copyFromRevisionId: null,
    cropX: 0,
    cropY: 0,
    zoom: 1,
  };
}
