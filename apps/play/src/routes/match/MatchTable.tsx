import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type {
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import {
  layoutTable,
  type Placement,
  type TableView,
} from "../../match/table-model";
import type { CardMark } from "./combat";
import * as s from "./match-table.css";

const SLEEVE = `${import.meta.env.BASE_URL}cards/spellwake-sleeve-midnight.svg`;
/** One curve and one base time for every card and camera move. */
const EASE = [0.22, 1, 0.36, 1] as const;
const TRAVEL = 0.56;

export const cardName = (object: PlayerObject | null): string =>
  object?.identity.kind === "known"
    ? object.identity.characteristics.name
    : "Hidden card";

/** Power and toughness, when they are no longer what the card prints. */
function changedStats(object: PlayerObject): string | null {
  if (object.identity.kind !== "known" || !object.publicProperties) return null;
  const { power, toughness } = object.publicProperties;
  if (power === null || toughness === null) return null;
  const printed = object.identity.characteristics;
  const damaged = object.damageMarked !== "0";
  if (
    !damaged &&
    printed.printedPower === power &&
    printed.printedToughness === toughness
  )
    return null;
  return `${power}/${toughness}`;
}

function Face({
  placement,
  faceOf,
}: {
  placement: Placement;
  faceOf: (object: PlayerObject) => string | undefined;
}): ReactElement {
  const { object, kind } = placement;
  if (kind === "back" || kind === "library" || !object)
    return (
      <img
        className={kind === "library" ? `${s.face} ${s.pile}` : s.face}
        src={SLEEVE}
        alt=""
        draggable={false}
      />
    );
  const url = faceOf(object);
  if (url)
    return (
      <img
        className={s.face}
        src={url}
        alt=""
        draggable={false}
        loading="lazy"
      />
    );
  // No picture for this printing: its printed text, on a plain frame.
  const printed =
    object.identity.kind === "known" ? object.identity.characteristics : null;
  return (
    <span className={s.textCard}>
      <span className={s.textName}>{cardName(object)}</span>
      <span className={s.textType}>{printed?.typeLine}</span>
      <span className={s.textRules}>{printed?.rulesText}</span>
      {printed?.printedPower != null && (
        <span className={s.textStats}>
          {printed.printedPower}/{printed.printedToughness}
        </span>
      )}
    </span>
  );
}

/**
 * The table for one camera view. Every card keeps its identity across views,
 * so changing the view moves the cards rather than redrawing them: that
 * travel is the camera. With reduced motion the cards are simply in place.
 */
export function MatchTable({
  snapshot,
  view,
  seatNames,
  faceOf,
  isPlayable,
  isOffered,
  marks,
  handTucked = false,
  clearBottom = 0,
  onCard,
  onPile,
}: {
  snapshot: PlayerSnapshot;
  view: TableView;
  seatNames: ReadonlyMap<string, string>;
  faceOf: (object: PlayerObject) => string | undefined;
  /** A card in hand the viewer may play now. */
  isPlayable?: (object: PlayerObject) => boolean;
  /** Rims for a declaration in progress, by card handle. */
  marks?: ReadonlyMap<string, CardMark> | undefined;
  /** While a spell waits for its target, the hand sinks to show the board. */
  handTucked?: boolean;
  /** Height along the lower edge that floating controls cover, in pixels. */
  clearBottom?: number;
  /** A card offered as a choice for the action in progress. */
  isOffered?: (object: PlayerObject) => boolean;
  onCard: (object: PlayerObject, zone: "hand" | "battlefield") => void;
  onPile: (pile: "library" | "graveyard", seatId: string) => void;
}): ReactElement {
  const root = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState({ width: 0, height: 0 });
  const reduced = useReducedMotion();
  useLayoutEffect(() => {
    const node = root.current;
    if (!node) return;
    const measure = () =>
      setStage({ width: node.clientWidth, height: node.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  const layout = useMemo(
    () =>
      stage.width > 0
        ? layoutTable(snapshot, view, stage, clearBottom)
        : { placements: [], labels: [], tilted: view.kind === "seat" },
    [snapshot, view, stage, clearBottom],
  );
  const transition = reduced
    ? { duration: 0 }
    : { duration: TRAVEL, ease: EASE };
  const viewerSeat = snapshot.viewerSeatId;

  const piece = (placement: Placement): ReactElement => {
    const { object, kind, zone } = placement;
    const scale = placement.width / s.BASE_WIDTH;
    // A tucked hand keeps only its top edge in view.
    const y =
      placement.y +
      (handTucked && zone === "hand" ? placement.width * 1.4 * 0.3 : 0);
    const name = cardName(object);
    const owner = seatNames.get(placement.seatId) ?? "Player";
    const mine = placement.seatId === viewerSeat;
    const label =
      kind === "library"
        ? `${mine ? "Your" : `${owner}’s`} library, ${placement.count} cards`
        : kind === "graveyard"
          ? `${mine ? "Your" : `${owner}’s`} graveyard, ${placement.count} cards, top card ${name}`
          : zone === "hand"
            ? object
              ? `${name}, in your hand`
              : `${owner}’s hand card`
            : `${mine ? "" : `${owner}’s `}${name}${object?.tapped ? ", tapped" : ""}`;
    const stats = object && kind === "card" ? changedStats(object) : null;
    const counters =
      object && kind === "card"
        ? object.counters.map((entry) => `${entry.count} ${entry.name}`)
        : [];
    const inert = kind === "back" && !object;
    // On a small whole-table view a card is too small to press: it is shown, not offered.
    const tooSmall = placement.width < 44;
    return (
      <motion.div
        key={placement.key}
        className={s.piece}
        data-anchor={object ? `card:${object.handle}` : undefined}
        style={{ zIndex: placement.order }}
        initial={
          reduced
            ? false
            : {
                x: placement.x,
                y: y + 26,
                scale: scale * 0.94,
                rotate: placement.rotate,
                opacity: 0,
              }
        }
        animate={{
          x: placement.x,
          y,
          scale,
          rotate: placement.rotate,
          opacity: 1,
        }}
        exit={reduced ? { opacity: 0 } : { opacity: 0, scale: scale * 0.9 }}
        transition={transition}
      >
        {tooSmall ? (
          <span
            className={s.cardButton}
            role="img"
            aria-label={label}
            data-mark={object ? marks?.get(object.handle) : undefined}
          >
            <Face placement={placement} faceOf={faceOf} />
            {(kind === "library" || kind === "graveyard") && (
              <span className={s.countBadge} aria-hidden="true">
                {placement.count}
              </span>
            )}
            {(stats || counters.length > 0) && (
              <span
                className={object?.tapped ? `${s.badge} ${s.upright}` : s.badge}
                aria-hidden="true"
              >
                {[stats, ...counters].filter(Boolean).join(" · ")}
              </span>
            )}
          </span>
        ) : (
          <button
            type="button"
            className={s.cardButton}
            data-zone={zone === "pile" ? "battlefield" : zone}
            data-playable={
              object && zone === "hand" && isPlayable?.(object)
                ? "true"
                : undefined
            }
            data-offered={object && isOffered?.(object) ? "true" : undefined}
            data-mark={object ? marks?.get(object.handle) : undefined}
            aria-label={label}
            disabled={inert}
            onClick={() => {
              if (kind === "library" || kind === "graveyard")
                onPile(kind, placement.seatId);
              else if (object)
                onCard(object, zone === "hand" ? "hand" : "battlefield");
            }}
          >
            <Face placement={placement} faceOf={faceOf} />
            {(kind === "library" || kind === "graveyard") && (
              <span className={s.countBadge} aria-hidden="true">
                {placement.count}
              </span>
            )}
            {(stats || counters.length > 0) && (
              <span
                className={object?.tapped ? `${s.badge} ${s.upright}` : s.badge}
                aria-hidden="true"
              >
                {[stats, ...counters].filter(Boolean).join(" · ")}
              </span>
            )}
          </button>
        )}
      </motion.div>
    );
  };

  const board = layout.placements.filter(
    (placement) => placement.zone !== "hand",
  );
  const hand = layout.placements.filter(
    (placement) => placement.zone === "hand",
  );
  return (
    <div
      ref={root}
      className={s.stage}
      data-match-table={view.kind}
      role="group"
      aria-label={
        view.kind === "overview"
          ? "Whole table"
          : view.seatId === viewerSeat
            ? "Your board"
            : `${seatNames.get(view.seatId) ?? "Player"}’s board`
      }
    >
      <div className={s.light} />
      <motion.div
        className={s.layer}
        initial={false}
        animate={{
          rotateX: layout.tilted ? 16 : 0,
          transformPerspective: 1500,
        }}
        transition={transition}
      >
        <AnimatePresence initial={false}>{board.map(piece)}</AnimatePresence>
        {layout.labels.map((label) => (
          <motion.span
            key={label.seatId}
            className={s.seatLabel}
            style={{ left: label.x, top: label.y }}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ ...transition, delay: reduced ? 0 : 0.2 }}
          >
            {seatNames.get(label.seatId) ?? "Player"}
          </motion.span>
        ))}
      </motion.div>
      <div className={s.handLayer}>
        <AnimatePresence initial={false}>{hand.map(piece)}</AnimatePresence>
      </div>
    </div>
  );
}
