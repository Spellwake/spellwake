import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { CircleUserRound } from "lucide-react";
import type {
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import {
  CompactSpellStack,
  resolveFramePalette,
  type CardFrameAccent,
  type StackEntry,
} from "@spellwake/ui";
import { useAnchors } from "./anchors";
import type { CardMark, SeatMark } from "./combat";
import * as s from "./match-table.css";

type WireEntry = PlayerSnapshot["stack"][number];
type StackTargetValue = WireEntry["targets"][number];
/** The pile's name for a spell that is being cast and is not yet on the stack. */
const PENDING = "cast:pending";

/** The pile's own card size, which every pose inside it is measured in. */
const CARD = { width: 178, height: 224 };
/** How far each older card shows above and to the left of the one in front. */
const PEEK = { x: 21, y: 20, most: 5 };

/** Map a known object's printed characteristics to a stack trigger frame accent. */
function stackFrameAccent(
  characteristics:
    | { name: string; typeLine: string; rulesText: string; manaCost: string | null }
    | undefined,
): CardFrameAccent {
  if (!characteristics) return "gold";
  const key = resolveFramePalette(
    {
      name: characteristics.name,
      typeLine: characteristics.typeLine,
      oracleText: characteristics.rulesText,
      manaCost: characteristics.manaCost,
    },
    "auto",
  ).key;
  if (
    key === "W" ||
    key === "U" ||
    key === "B" ||
    key === "R" ||
    key === "G" ||
    key === "gold" ||
    key === "artifact" ||
    key === "colorless" ||
    key === "land"
  ) {
    return key;
  }
  return "gold";
}

const nameOf = (object: PlayerObject | undefined): string | null =>
  object?.identity.kind === "known"
    ? object.identity.characteristics.name
    : null;

/** A spell is its own card on the stack; anything else is an ability of its source. */
function abilityType(label: string): string {
  if (/trigger/i.test(label)) return "Triggered ability";
  if (/activat/i.test(label)) return "Activated ability";
  return "Ability";
}

/**
 * Which stack item is being read, and what that marks on the table: the
 * player or card it targets. Reading never casts, retargets or passes.
 */
export function useStackReading(snapshot: PlayerSnapshot): {
  selectedId: string | null;
  select: (id: string | null) => void;
  marks: ReadonlyMap<string, CardMark>;
  seatMarks: ReadonlyMap<string, SeatMark>;
} {
  const [chosen, select] = useState<string | null>(null);
  // An item that has left the stack is no longer being read.
  const selected = snapshot.stack.find((entry) => entry.handle === chosen);
  const target = selected?.targets[0];
  return useMemo(
    () => ({
      selectedId: selected?.handle ?? null,
      select,
      marks:
        target?.kind === "object"
          ? new Map([[target.handle, "ivory" as const]])
          : new Map(),
      seatMarks:
        target?.kind === "seat"
          ? new Map([[target.seatId, "marked" as const]])
          : new Map(),
    }),
    [selected, target],
  );
}

interface Place {
  x: number;
  y: number;
  scale: number;
  /** Placed at the left edge, away from the turn panel and the pile. */
  apart?: boolean;
}

/**
 * Where the pile sits: under the turn panel on a wide screen, and under the
 * players along the right edge when the panel is docked below. It keeps clear
 * of the hand and shrinks before it would leave the table.
 */
export function usePilePlace(
  host: RefObject<HTMLDivElement | null>,
  count: number,
  clearBottom: number,
  /** What is being placed, in the pile's design pixels: the pile's own card when absent. */
  card: { width: number; height: number } = CARD,
  /**
   * Where it goes with no room under the turn panel, as on a phone held
   * sideways: beside the panel, or at the table's left edge, clear of the
   * cards. Two things share that edge as its upper and lower parts.
   */
  crowded: "beside" | "left" | "left-upper" | "left-lower" = "beside",
): Place | null {
  const [place, setPlace] = useState<Place | null>(null);
  useLayoutEffect(() => {
    const measure = () => {
      const stage = host.current?.parentElement?.getBoundingClientRect();
      if (!stage || stage.width === 0) return;
      const rect = (name: string) =>
        document
          .querySelector(`[data-frame="${name}"]`)
          ?.getBoundingClientRect();
      const side = rect("hud-side");
      const players = rect("hud-players");
      const density =
        parseFloat(getComputedStyle(document.documentElement).fontSize) / 16;
      const peeks = Math.min(Math.max(count - 1, 0), PEEK.most);
      const under = (players?.bottom ?? stage.top) - stage.top;
      const floor = stage.height - clearBottom - 10;
      const fit = (top: number, right: number, most: number): Place => {
        // The oldest card peeks above the front one, so the room is shared.
        const room = Math.max(floor - top, 60);
        // On a short screen the pile never takes more than half the height.
        const tall = card.height + peeks * PEEK.y;
        const scale = Math.max(
          0.42,
          Math.min(most, room / tall, (stage.height * 0.5) / tall),
        );
        return {
          scale,
          x: right - card.width * scale,
          y: top + peeks * PEEK.y * scale,
        };
      };
      const aside = (top: number): Place => {
        const column = Math.max(floor - top, 60);
        const tall = card.height + peeks * PEEK.y;
        if (crowded === "left-upper") {
          const scale = Math.min(density, (column * 0.56) / tall);
          return { scale, x: 12, y: top, apart: true };
        }
        if (crowded === "left-lower") {
          const scale = Math.min(density, (column * 0.4) / tall);
          return {
            scale,
            // Room for the older cards, which show more while offered as targets.
            x: 12 + (peeks > 0 ? Math.max(peeks * PEEK.x, 50) : 0) * scale,
            y: floor - card.height * scale,
            apart: true,
          };
        }
        return { ...fit(top, 0, density), x: 12, apart: true };
      };
      const docked = !side || side.top - stage.top > stage.height * 0.45;
      let next: Place;
      if (docked) {
        // A phone: small, along the right edge under the players.
        const most = Math.min(density, (stage.width * 0.27) / card.width);
        next = fit(under + 14, stage.width - 14, most);
      } else {
        const below = fit(
          side.bottom - stage.top + 28 * density,
          side.right - stage.left - 46 * density,
          density,
        );
        // With no room under the panel, the pile stands beside it instead.
        next =
          below.scale >= density * 0.8
            ? below
            : crowded === "beside"
              ? fit(under + 16, side.left - stage.left - 20, density)
              : aside(under + 12);
      }
      setPlace((current) =>
        current &&
        Math.abs(current.x - next.x) < 0.5 &&
        Math.abs(current.y - next.y) < 0.5 &&
        Math.abs(current.scale - next.scale) < 0.005 &&
        current.apart === next.apart
          ? current
          : next,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    const side = document.querySelector('[data-frame="hud-side"]');
    if (side) observer.observe(side);
    if (host.current?.parentElement)
      observer.observe(host.current.parentElement);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [host, count, clearBottom, card.width, card.height, crowded]);
  return place;
}

/**
 * The spells and abilities waiting to resolve, as approved: a compact pile of
 * printed faces, the newest in front. Clicking a face reads it in place and
 * draws one line to what it targets; Escape or the table puts it back.
 */
export function MatchStack({
  snapshot,
  seatNames,
  faceOf,
  selectedId,
  onSelect,
  pending = null,
  targets = null,
  locked = false,
  aside = false,
  follow,
  clearBottom = 0,
}: {
  snapshot: PlayerSnapshot;
  seatNames: ReadonlyMap<string, string>;
  faceOf: (object: PlayerObject) => string | undefined;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  /**
   * A spell being cast: shown in front of the pile until the game accepts
   * it, with a line to its target once one is chosen. It is not on the stack.
   */
  pending?: { object: PlayerObject; target: StackTargetValue | null } | null;
  /**
   * While a spell being cast may target a spell here: pressing an offered
   * item chooses it as that target instead of reading it.
   */
  targets?: {
    ids: ReadonlySet<string>;
    choose: (handle: string) => void;
  } | null;
  /** A cast is being drafted: the pile is not read until it ends. */
  locked?: boolean;
  /**
   * The spell being cast stands on its own face: where there is no room
   * beside the turn panel, the pile shares the left edge with it.
   */
  aside?: boolean;
  /** Changes whenever the cards may have moved. */
  follow: unknown;
  clearBottom?: number;
}): ReactElement {
  const host = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();
  const stack = useMemo((): readonly WireEntry[] => {
    if (!pending) return snapshot.stack;
    return [
      ...snapshot.stack,
      {
        handle: PENDING,
        controllerSeatId: snapshot.viewerSeatId,
        sourceObject: pending.object.handle,
        label: nameOf(pending.object) ?? "Spell",
        targets: pending.target ? [pending.target] : [],
        unavailableTargets: "0",
      },
    ];
  }, [snapshot.stack, snapshot.viewerSeatId, pending]);
  // A pending spell with its target chosen is read as a matter of course.
  const reading = pending?.target ? PENDING : locked ? null : selectedId;
  // The pile fades out with what it last held.
  const last = useRef(stack);
  useEffect(() => {
    if (stack.length > 0) last.current = stack;
  }, [stack]);
  const shown = stack.length > 0 ? stack : last.current;
  const place = usePilePlace(
    host,
    shown.length,
    clearBottom,
    CARD,
    aside ? "left-lower" : "beside",
  );

  const objects = useMemo(
    () => new Map(snapshot.visibleObjects.map((o) => [o.handle, o] as const)),
    [snapshot.visibleObjects],
  );
  const stackZone = snapshot.zones.find((zone) => zone.zoneType === "stack");
  const spells = new Set(
    stackZone?.visibility === "visible" ? stackZone.objects : [],
  );

  const selected = shown.find((entry) => entry.handle === reading);
  const target = selected?.targets[0];
  const anchorName =
    target?.kind === "seat"
      ? `seat:${target.seatId}`
      : target?.kind === "object"
        ? `card:${target.handle}`
        : null;
  const { rects } = useAnchors(
    host,
    anchorName ? [anchorName] : [],
    `${String(follow)}:${place?.x}:${place?.y}:${place?.scale}`,
  );

  const boardTarget = (): StackEntry["target"] => {
    const rect = anchorName ? rects.get(anchorName) : undefined;
    if (!rect || !place || !target || target.kind === "stack") return undefined;
    const scale = place.scale;
    if (target.kind === "seat")
      return {
        kind: "board",
        label: seatNames.get(target.seatId) ?? "Player",
        // The portrait is the round picture at the head of the seat's chip.
        anchor: {
          x: (rect.left + rect.height * 0.5) / scale,
          y: (rect.bottom - 2) / scale,
        },
      };
    // A card is met at the point of its edge nearest the pile.
    const from = {
      x: (CARD.width / 2) * scale,
      y: (CARD.height / 2) * scale,
    };
    return {
      kind: "board",
      label: nameOf(objects.get(target.handle)) ?? "Card",
      anchor: {
        x: Math.min(Math.max(from.x, rect.left + 4), rect.right - 4) / scale,
        y: Math.min(Math.max(from.y, rect.top + 4), rect.bottom - 4) / scale,
      },
    };
  };

  const items = shown.map((entry: WireEntry): StackEntry => {
    const source = entry.sourceObject
      ? objects.get(entry.sourceObject)
      : undefined;
    const face = source ? faceOf(source) : undefined;
    const name = nameOf(source);
    const first = entry.targets[0];
    const targeted: StackEntry["target"] =
      entry.handle !== reading || !first
        ? undefined
        : first.kind === "stack"
          ? { kind: "item", id: first.handle }
          : boardTarget();
    const base = {
      id: entry.handle,
      controller: seatNames.get(entry.controllerSeatId) ?? "Player",
      portrait: <CircleUserRound aria-hidden="true" />,
      ...(targeted ? { target: targeted } : {}),
    };
    const isSpell =
      entry.handle === PENDING ||
      (entry.sourceObject !== null && spells.has(entry.sourceObject));
    if (isSpell && face && name)
      return {
        ...base,
        kind: "spell",
        image: { src: face, name, width: 500, height: 700 },
      };
    const known = source?.identity.kind === "known" ? source.identity : null;
    const frameAccent = stackFrameAccent(known?.characteristics);
    if (isSpell && known)
      // A spell with no picture: its printed words on the unified stack frame.
      return {
        ...base,
        kind: "ability",
        name: known.characteristics.name,
        typeLabel: known.characteristics.typeLine,
        effect: known.characteristics.rulesText,
        frameAccent,
      };
    const typeLabel = abilityType(entry.label);
    return {
      ...base,
      kind: "ability",
      name: name ?? entry.label,
      ...(face ? { artUrl: face, artSourceKind: "full_card" as const } : {}),
      frameAccent,
      typeLabel,
      effect: entry.label === typeLabel ? "" : entry.label,
    };
  });

  return (
    <AnimatePresence>
      {stack.length > 0 && (
        <motion.div
          ref={host}
          key="pile"
          className={s.pileHost}
          data-match-stack={stack.length}
          style={{
            x: place?.x ?? 0,
            y: place?.y ?? 0,
            scale: place?.scale ?? 1,
            visibility: place ? "visible" : "hidden",
          }}
          initial={reduced ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{
            duration: reduced ? 0 : 0.28,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <CompactSpellStack
            items={items}
            selectedId={reading}
            offeredIds={targets ? [...targets.ids] : []}
            // Choosing a target comes before reading; the pending spell is
            // not a stack item to read or put back.
            onSelect={(id) => {
              if (id !== null && targets?.ids.has(id)) targets.choose(id);
              else if (!pending && !locked && id !== PENDING) onSelect(id);
            }}
          />
        </motion.div>
      )}
    </AnimatePresence>
  );
}
