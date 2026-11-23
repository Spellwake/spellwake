import type {
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";

/**
 * The match table: what sits on each seat's board, and where every card is
 * drawn for a camera view. Pure, so the layout can be tested without a
 * renderer. The approved views are the viewer's own board, another seat's
 * board, and the whole table from above (NP-01 to NP-03).
 */

export type TableView = { kind: "seat"; seatId: string } | { kind: "overview" };

/** One seat's public table: permanents by row, piles and hand. */
export interface SeatBoard {
  seatId: string;
  /** Creatures, and anything that is not a land. Auras and equipment follow their host. */
  front: PlayerObject[];
  lands: PlayerObject[];
  /** Noncreature, nonland permanents: artifacts, enchantments, planeswalkers. */
  others: PlayerObject[];
  command: PlayerObject[];
  graveyard: PlayerObject[];
  libraryCount: number;
  /** Cards the viewer may see in this hand; the rest are counted. */
  hand: PlayerObject[];
  handCount: number;
}

const count = (text: string): number => {
  const value = Number(text);
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
};
const types = (object: PlayerObject): readonly string[] =>
  object.publicProperties?.cardTypes ?? [];

/** Everything public about one seat's side of the table. */
export function seatBoard(snapshot: PlayerSnapshot, seatId: string): SeatBoard {
  const byHandle = new Map(
    snapshot.visibleObjects.map((object) => [object.handle, object]),
  );
  const objectsOf = (zoneType: string, owner: string | null) => {
    const zone = snapshot.zones.find(
      (entry) => entry.zoneType === zoneType && entry.ownerSeatId === owner,
    );
    return zone && zone.visibility !== "hidden"
      ? zone.objects.flatMap((handle) => {
          const object = byHandle.get(handle);
          return object ? [object] : [];
        })
      : [];
  };
  const permanents = objectsOf("battlefield", null).filter(
    (object) => (object.controllerSeatId ?? object.ownerSeatId) === seatId,
  );
  const lands = permanents.filter((object) => types(object).includes("land"));
  const front = permanents.filter(
    (object) =>
      types(object).includes("creature") && !types(object).includes("land"),
  );
  const others = permanents.filter(
    (object) => !lands.includes(object) && !front.includes(object),
  );
  const seat = snapshot.players.find((player) => player.seatId === seatId);
  return {
    seatId,
    front,
    lands,
    others,
    command: objectsOf("command", null).filter(
      (object) => object.ownerSeatId === seatId,
    ),
    graveyard: objectsOf("graveyard", seatId),
    libraryCount: count(seat?.libraryCount ?? "0"),
    hand: objectsOf("hand", seatId),
    handCount: count(seat?.handCount ?? "0"),
  };
}

/** Where one thing is drawn, in stage pixels from the stage's top-left. */
export interface Placement {
  /** Stable across views, so a card travels instead of being redrawn. */
  key: string;
  kind: "card" | "back" | "library" | "graveyard";
  seatId: string;
  object: PlayerObject | null;
  /** Centre of the card. */
  x: number;
  y: number;
  /** Card width; height follows the card's proportions. */
  width: number;
  /** Degrees. A tapped permanent lies on its side; a hand card leans in its fan. */
  rotate: number;
  zone: "battlefield" | "hand" | "pile";
  /** Cards in a pile, for its count. */
  count?: number;
  /** Later placements are drawn over earlier ones. */
  order: number;
}

export const CARD_RATIO = 1.4;

/** Centres for a row of slots, squeezed together when they outgrow the room. */
export function rowCentres(
  widths: readonly number[],
  centre: number,
  room: number,
  gap: number,
): number[] {
  if (widths.length === 0) return [];
  const natural =
    widths.reduce((sum, width) => sum + width, 0) + gap * (widths.length - 1);
  const last = widths.at(-1)!;
  // When squeezed, each card advances by a share of its width; the last is whole.
  const squeeze =
    natural <= room || widths.length === 1
      ? 1
      : Math.max(0.18, (room - last) / (natural - last));
  const advances = widths.map((width, index) =>
    index === widths.length - 1 ? width : (width + gap) * squeeze,
  );
  const total = advances.reduce((sum, advance) => sum + advance, 0);
  let cursor = centre - total / 2;
  return widths.map((width, index) => {
    const at = cursor + width / 2;
    cursor += advances[index]!;
    return at;
  });
}

/** Seats around the whole table, clockwise from the viewer at the lower left. */
export function overviewCells(
  seats: number,
): { column: number; row: "top" | "bottom" }[] {
  const layouts: Record<number, [number, "top" | "bottom"][]> = {
    1: [[1, "bottom"]],
    2: [
      [1, "bottom"],
      [1, "top"],
    ],
    3: [
      [1, "bottom"],
      [0, "top"],
      [2, "top"],
    ],
    4: [
      [0, "bottom"],
      [0, "top"],
      [2, "top"],
      [2, "bottom"],
    ],
    5: [
      [0, "bottom"],
      [0, "top"],
      [1, "top"],
      [2, "top"],
      [2, "bottom"],
    ],
    6: [
      [0, "bottom"],
      [0, "top"],
      [1, "top"],
      [2, "top"],
      [2, "bottom"],
      [1, "bottom"],
    ],
  };
  return (layouts[Math.min(6, Math.max(1, seats))] ?? []).map(
    ([column, row]) => ({ column, row }),
  );
}

interface BoardBox {
  /** Centre of the board, across. */
  centre: number;
  room: number;
  cardWidth: number;
  /** Centre line of the row nearest the board's owner, and of the row beyond it. */
  nearY: number;
  farY: number;
}

/** One seat's permanents and piles inside a box, lands nearest their owner. */
function placeBoard(
  board: SeatBoard,
  box: BoardBox,
  order: { next: number },
): Placement[] {
  const { cardWidth: width } = box;
  const slot = (object: PlayerObject) =>
    object.tapped ? width * CARD_RATIO : width;
  const gap = width * 0.07;
  const placed: Placement[] = [];
  const card = (object: PlayerObject, x: number, y: number): Placement => ({
    key: object.handle,
    kind: object.identity.kind === "concealed" ? "back" : "card",
    seatId: board.seatId,
    object,
    x,
    y,
    width,
    rotate: object.tapped ? 90 : 0,
    zone: "battlefield",
    order: order.next++,
  });

  // The far row: the command zone, then creatures.
  const far = [...board.command, ...board.front];
  rowCentres(far.map(slot), box.centre, box.room, gap).forEach((x, index) =>
    placed.push(card(far[index]!, x, box.farY)),
  );

  // The near row: graveyard and library, lands, then other permanents.
  type Slot =
    | { pile: "graveyard" | "library" }
    | { object: PlayerObject }
    | { space: true };
  const near: Slot[] = [
    ...(board.graveyard.length > 0 ? [{ pile: "graveyard" as const }] : []),
    ...(board.libraryCount > 0 ? [{ pile: "library" as const }] : []),
    ...(board.lands.length > 0 ? [{ space: true as const }] : []),
    ...board.lands.map((object) => ({ object })),
    ...(board.others.length > 0 ? [{ space: true as const }] : []),
    ...board.others.map((object) => ({ object })),
  ];
  const widths = near.map((entry) =>
    "object" in entry ? slot(entry.object) : "space" in entry ? gap : width,
  );
  rowCentres(widths, box.centre, box.room, gap).forEach((x, index) => {
    const entry = near[index]!;
    if ("object" in entry) placed.push(card(entry.object, x, box.nearY));
    else if ("pile" in entry) {
      const isLibrary = entry.pile === "library";
      placed.push({
        key: `${board.seatId}:${entry.pile}`,
        kind: entry.pile,
        seatId: board.seatId,
        object: isLibrary ? null : board.graveyard.at(-1)!,
        x,
        y: box.nearY,
        width,
        rotate: 0,
        zone: "pile",
        count: isLibrary ? board.libraryCount : board.graveyard.length,
        order: order.next++,
      });
    }
  });
  return placed;
}

/** A hand as a fan along the stage's lower edge. */
function placeHand(
  board: SeatBoard,
  stage: { width: number; height: number },
  cardWidth: number,
  order: { next: number },
): Placement[] {
  const known = board.hand;
  const hidden = Math.max(0, board.handCount - known.length);
  // A large concealed hand shows as a dozen backs at most.
  const backs = Math.min(hidden, 12);
  const total = known.length + backs;
  if (total === 0) return [];
  const width = cardWidth;
  const room = Math.min(stage.width * 0.86, width * 0.78 * (total + 0.4));
  const centres = rowCentres(
    Array.from({ length: total }, () => width),
    stage.width / 2,
    room,
    -width * 0.22,
  );
  const lean = Math.min(5, 34 / Math.max(1, total));
  // Two fifths of each card stays below the edge, as a held hand does.
  const baseline = stage.height - width * CARD_RATIO * 0.1;
  return centres.map((x, index) => {
    const offset = index - (total - 1) / 2;
    const object = known[index] ?? null;
    return {
      key: object ? object.handle : `${board.seatId}:hand:${index}`,
      kind: object && object.identity.kind === "known" ? "card" : "back",
      seatId: board.seatId,
      object,
      x,
      y: baseline + Math.abs(offset) ** 1.6 * width * 0.035,
      width,
      rotate: offset * lean,
      zone: "hand",
      order: order.next++,
    };
  });
}

/** Card sizes for a seat view: permanents, and the larger cards held in hand. */
export function seatViewSizes(stage: { width: number; height: number }): {
  cardWidth: number;
  handWidth: number;
} {
  // A narrower stage gives each card a larger share of its width.
  const share = Math.max(
    stage.width * 0.082,
    Math.min(stage.width * 0.115, 96),
  );
  const cardWidth = Math.max(54, Math.min(share, stage.height * 0.17, 132));
  return { cardWidth, handWidth: cardWidth * 1.42 };
}
/** How much of the fanned hand shows above the stage's lower edge. */
export const handShownHeight = (stage: {
  width: number;
  height: number;
}): number => seatViewSizes(stage).handWidth * CARD_RATIO * 0.5;

export interface TableLayout {
  placements: Placement[];
  /** Seat names drawn on the whole-table view. */
  labels: { seatId: string; x: number; y: number }[];
  /** The focused board is drawn tilted away from the viewer. */
  tilted: boolean;
}

/**
 * Every card's place for a view. A seat view shows that seat's board with its
 * hand fanned along the lower edge; at a two-player table the other board
 * sits across from it. The whole-table view gives each seat a fixed cell.
 */
export function layoutTable(
  snapshot: PlayerSnapshot,
  view: TableView,
  stage: { width: number; height: number },
  /** Height along the lower edge that controls cover, which the board stays above. */
  clearBottom = 0,
): TableLayout {
  const order = { next: 0 };
  const seats = snapshot.players.map((player) => player.seatId);
  // Seats run clockwise from the viewer.
  const from = Math.max(0, seats.indexOf(snapshot.viewerSeatId));
  const around = [...seats.slice(from), ...seats.slice(0, from)];
  const { width: w, height: h } = stage;

  if (view.kind === "overview") {
    const cells = overviewCells(around.length);
    const columns = around.length <= 2 ? 1 : 3;
    const cellWidth = w / (around.length <= 2 ? 1.6 : columns);
    const cardWidth = Math.max(
      26,
      Math.min(cellWidth / 9.2, ((h - clearBottom) * 0.5) / (CARD_RATIO * 2.9)),
    );
    const cardHeight = cardWidth * CARD_RATIO;
    const placements: Placement[] = [];
    const labels: TableLayout["labels"] = [];
    const middle = (h - clearBottom) * 0.56;
    around.forEach((seatId, index) => {
      const cell = cells[index];
      if (!cell) return;
      const centre = columns === 1 ? w / 2 : cellWidth * (cell.column + 0.5);
      const top = cell.row === "top";
      // Lands sit nearest each seat's own edge of the table.
      const nearY = top
        ? middle - cardHeight * 2.05
        : middle + cardHeight * 2.05;
      const farY = top ? middle - cardHeight * 0.9 : middle + cardHeight * 0.9;
      placements.push(
        ...placeBoard(
          seatBoard(snapshot, seatId),
          { centre, room: cellWidth * 0.9, cardWidth, nearY, farY },
          order,
        ),
      );
      labels.push({
        seatId,
        x: centre,
        y: top ? nearY - cardHeight * 0.95 : nearY + cardHeight * 0.95,
      });
    });
    return { placements, labels, tilted: false };
  }

  const { cardWidth, handWidth } = seatViewSizes(stage);
  const cardHeight = cardWidth * CARD_RATIO;
  const focused = seatBoard(snapshot, view.seatId);
  const nearY =
    h - Math.max(handShownHeight(stage), clearBottom) - cardHeight * 0.62;
  const farY = nearY - cardHeight * 1.16;
  const placements = placeBoard(
    focused,
    { centre: w / 2, room: w * 0.9, cardWidth, nearY, farY },
    order,
  );
  // Across a two-player table, the other board faces this one.
  if (around.length === 2) {
    const other = around.find((seatId) => seatId !== view.seatId);
    if (other) {
      const width = cardWidth * 0.8;
      const height = width * CARD_RATIO;
      const acrossFar = farY - cardHeight * 0.62 - height * 0.72;
      placements.push(
        ...placeBoard(
          seatBoard(snapshot, other),
          {
            centre: w / 2,
            room: w * 0.8,
            cardWidth: width,
            nearY: acrossFar - height * 1.14,
            farY: acrossFar,
          },
          order,
        ),
      );
    }
  }
  placements.push(...placeHand(focused, stage, handWidth, order));
  return { placements, labels: [], tilted: true };
}
