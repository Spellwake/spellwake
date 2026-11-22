import type { CSSProperties } from "react";
import { TargetOption } from "@spellwake/ui";
import { cardImage, cardName, type Card, sourceFor } from "./assets";
import sleeve from "../../../../../output/card-study-matte-approved-2026-10-31/preview/app/cards/spellwake-sleeve-midnight.svg?inline";

export type Piece = {
  id: string;
  face: Card;
  x: number;
  y: number;
  w: number;
  h: number;
  tapped?: boolean;
  back?: boolean;
  angle?: number;
  zone?: "hand";
  seat?: string;
};
type TableProps = {
  id: string;
  family: string;
  overview: boolean;
  camera: string;
  watching: boolean;
  selected: readonly string[];
  legal: (piece: Piece) => boolean;
  onChoose: (piece: Piece) => void;
};
export const center = (p: Piece) => ({ x: p.x + p.w / 2, y: p.y + p.h / 2 });
function homePieces(id: string, family: string, watching: boolean): Piece[] {
  const mixed = family === "MX",
    wide = mixed ? -220 : 0,
    action = sourceFor(family);
  const accepted = [
    "BC06",
    "BC07",
    "MC06",
    "MM07",
    "MT06",
    "MX08",
    "DC11",
  ].includes(id);
  const blue = family === "MM" || mixed,
    green = family === "MT";
  const landscape: Card = blue ? "island" : green ? "forest" : "mountain";
  const table: Piece[] = [
    {
      id: "far-aura",
      face: "pacifism",
      x: 240 + wide,
      y: 480,
      w: 115,
      h: 155,
      angle: 6,
    },
    { id: "elf", face: "elf", x: 575 + wide, y: 481, w: 125, h: 155, angle: 2 },
    { id: "bear", face: "bear", x: 717 + wide, y: 481, w: 125, h: 155 },
    {
      id: "other-angel",
      face: "angel",
      x: 855 + wide,
      y: 481,
      w: 125,
      h: 155,
      angle: -2,
    },
    {
      id: "chandra",
      face: "chandra",
      x: 1136 + wide,
      y: 481,
      w: 123,
      h: 155,
      angle: -5,
    },
    {
      id: "far-aura-2",
      face: "pacifism",
      x: 1480 + wide,
      y: 481,
      w: 118,
      h: 155,
      angle: -8,
    },
    {
      id: "angel",
      face: "angel",
      x: 190 + wide,
      y: 680,
      w: 138,
      h: 170,
      angle: 7,
    },
    {
      id: "library",
      face: "mountain",
      x: 338 + wide,
      y: 662,
      w: 147,
      h: 183,
      back: true,
      angle: 5,
    },
    {
      id: "ball",
      face: "ball",
      x: 1103 + wide,
      y: 680,
      w: 129,
      h: 173,
      angle: -4,
    },
    {
      id: "aura",
      face: "pacifism",
      x: 1250 + wide,
      y: 680,
      w: 134,
      h: 173,
      angle: -6,
    },
    {
      id: "thopter",
      face: "thopter",
      x: 1542 + wide,
      y: 680,
      w: 128,
      h: 173,
      angle: -6,
    },
  ];
  for (let i = 0; i < 5; i++) {
    const tapped =
      id === "BC08" ||
      (accepted &&
        i >=
          (id === "BC07" || family === "MM" || family === "MX"
            ? 1
            : family === "MT" || family === "DC"
              ? 2
              : family === "MC"
                ? 3
                : 4));
    table.push({
      id: "land" + i,
      face: green && i >= 3 ? "plains" : landscape,
      x: 546 + wide + i * 95,
      y: 680 + (i === 4 ? 12 : 0),
      w: 122,
      h: 166,
      tapped,
    });
  }
  if (id === "AT-08") table.find((p) => p.id === "bear")!.tapped = true;
  if (!watching) {
    const raised =
      family === "NP" ||
      family === "AS" ||
      family === "AT" ||
      family === "CM" ||
      family === "DA";
    const hand: Card[] = [
      "bolt",
      landscape,
      "bolt",
      landscape,
      "bolt",
      "bolt",
      action,
      "krenko",
      "bolt",
      "angel",
    ];
    if (accepted) hand.splice(id === "BC07" ? 7 : 6, 1);
    hand.forEach((face, i) => {
      const d = i - (hand.length - 1) / 2;
      table.push({
        id: "hand" + i,
        zone: "hand",
        face,
        x: 65 + i * 135,
        y: (raised ? 722 : 914) + d * d * 3.7,
        w: 213,
        h: 296,
        angle: d * 3.4,
      });
    });
  }
  return table;
}
function overviewPieces(watching: boolean, blocking: boolean): Piece[] {
  return ["Alex", "Morgan", "Sam", "You", "Jordan", "Riley"].flatMap(
    (seat, index) => {
      if (watching && seat === "You") return [];
      const upper = index < 3;
      const x = 45 + (index % 3) * 496;
      const y = upper ? 357 : 636;
      const base = [
        ["angel", 0, 0],
        ["mountain", 56, 0],
        ["mountain", 109, 0],
        ["mountain", 145, 0],
        ["mountain", 177, 0],
        ["mountain", 209, 0],
        ["mountain", 240, 0],
        ["bolt", 300, 0],
        ["pacifism", 365, 0],
        ["chandra", 23, 1],
        ["angel", 133, 1],
        ["bear", 194, 1],
        ["elf", 253, 1],
        ["pacifism", 359, 1],
      ] as const;
      return base.map(([face, off, row], n) => {
        const incoming = blocking && seat === "Alex" && n === 10;
        const nearFaces: Record<number, Card> = {
          1: "bolt",
          7: "mountain",
          9: "pacifism",
          13: "chandra",
        };
        const nearOffsets: Record<number, number> = { 10: 253, 12: 133 };
        return {
          id: seat + "-" + n,
          face: incoming ? "bear" : !upper ? (nearFaces[n] ?? face) : face,
          x: x + (upper ? off : (nearOffsets[n] ?? off)),
          y: y + 78 * (upper ? row : 1 - row),
          w: 49,
          h: 65,
          seat,
          back: upper ? n === 7 : n === 1,
          tapped: incoming,
        };
      });
    },
  );
}
/** An opponent's seat has a visible far battlefield and a partly visible concealed hand. */
function opponentPieces(watching: boolean): Piece[] {
  const pieces: Piece[] = [];
  const add = (
    id: string,
    face: Card,
    x: number,
    y: number,
    w: number,
    h: number,
    angle = 0,
  ) => pieces.push({ id, face, x, y, w, h, angle });
  const far = [
    ["bolt", 313],
    ["angel", 536],
    ["bear", 678],
    ["elf", 813],
    ["pacifism", 1039],
  ] as const;
  far.forEach(([face, x], i) => {
    if (i > 0 && i < 4)
      add("far-rear-" + i, "mountain", x + 3, 121, 116, 87, 3 - i * 2);
    add("far-" + i, face, x, 136, 116, 94, 10 - i * 5);
  });
  add("far-aura", "pacifism", 216, 362, 138, 174, 8);
  add("elf", "elf", 586, 362, 137, 174, 2);
  add("bear", "bear", 735, 362, 137, 174);
  add("other-angel", "angel", 882, 362, 137, 174, -3);
  add("chandra", "chandra", 1168, 362, 142, 174, -6);
  add("angel", "angel", 140, 562, 158, 187, 9);
  pieces.push({
    id: "library",
    face: "mountain",
    x: 310,
    y: 550,
    w: 159,
    h: 196,
    back: true,
    angle: 6,
  });
  for (let i = 0; i < 4; i++)
    add("land" + i, "mountain", 535 + 124 * i, 568, 147, 178, 3 - i * 2);
  add("ball", "ball", 1130, 562, 152, 188, -6);
  add("aura", "pacifism", 1290, 562, 152, 188, -7);
  if (!watching) {
    for (let i = 0; i < 7; i++) {
      const d = i - 3;
      pieces.push({
        id: "opponent-hand-" + i,
        face: i === 6 ? "krenko" : "mountain",
        zone: "hand",
        x: 305 + i * 112,
        y: 743 + d * d * 6.8,
        w: 213,
        h: 296,
        angle: d * 4.7,
        back: i < 6,
      });
    }
  }
  return pieces;
}

export function piecesFor(
  id: string,
  family: string,
  overview: boolean,
  watching: boolean,
  camera = "You",
) {
  if (overview) return overviewPieces(watching, family === "BL");
  if (camera !== "You") return opponentPieces(watching);
  return homePieces(id, family, watching);
}
export function Battlefield({
  id,
  family,
  overview,
  camera,
  watching,
  selected,
  legal,
  onChoose,
}: TableProps) {
  const pieces = piecesFor(id, family, overview, watching, camera);
  return (
    <div
      className="match-field"
      aria-label={overview ? "Table" : "Battlefield"}
    >
      {overview &&
        ["Alex", "Morgan", "Sam", "You", "Jordan", "Riley"].map((name, i) => (
          <span
            key={name}
            className="match-seat-label"
            style={{ left: 44 + (i % 3) * 496, top: i < 3 ? 306 : 820 }}
          >
            {watching && name === "You" ? "You · Conceded" : name}
          </span>
        ))}
      {pieces.map((p) => {
        const offered = legal(p),
          hand = p.zone === "hand",
          blocked = hand && camera !== "You";
        const style: CSSProperties = {
          left: p.x,
          top: p.y,
          width: p.w,
          height: p.h,
          transform: hand
            ? `rotate(${p.angle ?? 0}deg)`
            : `perspective(700px) rotateX(12deg) skewX(${-(p.angle ?? 0)}deg) rotate(${p.tapped ? 90 : 0}deg)`,
        };
        const art = (
          <img
            alt={p.back ? "Card back" : cardName(p.face)}
            src={p.back ? sleeve : cardImage(p.face).src}
            draggable={false}
          />
        );
        const label = `${p.seat ?? ""} ${cardName(p.face)}`.trim();
        return (
          <div
            className={
              "match-piece" +
              (hand ? " match-hand-card" : "") +
              (selected.includes(p.id) ? " match-linked" : "") +
              (p.back && !hand ? " match-deck" : "")
            }
            key={p.id}
            style={style}
          >
            {blocked || !offered ? (
              art
            ) : hand ? (
              <button
                type="button"
                aria-label={"Cast " + label}
                onDoubleClick={() => onChoose(p)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onChoose(p);
                  }
                }}
              >
                {art}
              </button>
            ) : (
              <TargetOption
                label={"Choose " + label}
                eligible
                onChoose={() => onChoose(p)}
              >
                {art}
              </TargetOption>
            )}
          </div>
        );
      })}
    </div>
  );
}
