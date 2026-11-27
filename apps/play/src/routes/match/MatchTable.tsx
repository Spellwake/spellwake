import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { useReducedMotion } from "motion/react";
import type {
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import {
  mountLiveMatchScene,
  type LiveCardRim,
  type LiveMatchSceneHandle,
  type LiveSceneCard,
  type ProjectedCardRect,
} from "spellwake-renderer-benchmark/live-match-scene";
import {
  CARD_RATIO,
  layoutTable,
  type Placement,
  type TableView,
} from "../../match/table-model";
import { SPELLWAKE_CARD_BACK_URL } from "../../cosmetics/card-backs";
import { useCardBackUrl } from "../../cosmetics/use-card-back-url";
import type { CardMark } from "./combat";
import * as s from "./match-table.css";

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

function resolveRim(
  placement: Placement,
  marks: ReadonlyMap<string, CardMark> | undefined,
  isOffered: ((object: PlayerObject) => boolean) | undefined,
  isPlayable: ((object: PlayerObject) => boolean) | undefined,
): LiveCardRim | null {
  const object = placement.object;
  if (!object) return null;
  const mark = marks?.get(object.handle);
  if (mark) return mark;
  if (isOffered?.(object)) return "offered";
  if (placement.zone === "hand" && isPlayable?.(object)) return "playable";
  return null;
}

function rectsChanged(
  prev: ReadonlyMap<string, ProjectedCardRect>,
  next: ReadonlyMap<string, ProjectedCardRect>,
): boolean {
  if (prev.size !== next.size) return true;
  for (const [key, rect] of next) {
    const old = prev.get(key);
    if (
      !old ||
      Math.abs(old.left - rect.left) > 0.5 ||
      Math.abs(old.top - rect.top) > 0.5 ||
      Math.abs(old.width - rect.width) > 0.5 ||
      Math.abs(old.height - rect.height) > 0.5
    ) {
      return true;
    }
  }
  return false;
}

/**
 * The live Three.js 3D match tabletop.
 *
 * Every card, pressed Midnight sleeve, instanced library/graveyard stack, felt
 * surface, shadow, hover lift, tap rotation, and glowing status rim renders on
 * the WebGL2 canvas. Visually transparent accessible buttons track each 3D card's
 * projected screen rectangle so keyboard/touch input, screen readers, combat
 * lines, damage pills, and cast-target lines align pixel-accurately with the 3D
 * meshes across all viewport sizes.
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
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const sceneRef = useRef<LiveMatchSceneHandle | null>(null);
  const projectedRef = useRef<ReadonlyMap<string, ProjectedCardRect>>(
    new Map(),
  );
  const [projected, setProjected] = useState<
    ReadonlyMap<string, ProjectedCardRect>
  >(() => new Map());
  const [stage, setStage] = useState({ width: 0, height: 0 });
  const [hoveredKey, setHoveredKey] = useState<string | null>(null);
  const reduced = Boolean(useReducedMotion());
  const sleeveUrl = useCardBackUrl();

  const handleProject = useCallback(
    (next: ReadonlyMap<string, ProjectedCardRect>) => {
      if (!rectsChanged(projectedRef.current, next)) return;
      projectedRef.current = next;
      setProjected(next);
    },
    [],
  );

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

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const controller = new AbortController();
    const handle = mountLiveMatchScene(canvas, {
      sleeveUrl,
      sleeveFallbackUrl: SPELLWAKE_CARD_BACK_URL,
      onProject: handleProject,
      signal: controller.signal,
    });
    sceneRef.current = handle;
    return () => {
      controller.abort();
      handle.dispose();
      if (sceneRef.current === handle) sceneRef.current = null;
    };
  }, [handleProject, sleeveUrl]);

  const layout = useMemo(
    () =>
      stage.width > 0
        ? layoutTable(snapshot, view, stage, clearBottom)
        : { placements: [], labels: [], tilted: view.kind === "seat" },
    [snapshot, view, stage, clearBottom],
  );

  const sceneCards = useMemo((): LiveSceneCard[] => {
    return layout.placements.map((placement) => {
      const { object, kind, zone } = placement;
      const y =
        placement.y +
        (handTucked && zone === "hand" ? placement.width * CARD_RATIO * 0.3 : 0);
      const printed =
        object?.identity.kind === "known"
          ? object.identity.characteristics
          : null;
      const faceUrl =
        object && kind !== "back" && kind !== "library"
          ? faceOf(object)
          : undefined;
      return {
        key: placement.key,
        kind,
        zone,
        x: placement.x,
        y,
        width: placement.width,
        rotateDeg: placement.rotate,
        order: placement.order,
        count: placement.count,
        faceUrl,
        fallback: printed
          ? {
              name: printed.name,
              typeLine: printed.typeLine,
              rulesText: printed.rulesText,
              stats:
                printed.printedPower != null && printed.printedToughness != null
                  ? `${printed.printedPower}/${printed.printedToughness}`
                  : undefined,
            }
          : undefined,
        rim: resolveRim(placement, marks, isOffered, isPlayable),
      };
    });
  }, [layout.placements, handTucked, faceOf, marks, isOffered, isPlayable]);

  useEffect(() => {
    if (!sceneRef.current || stage.width <= 0 || stage.height <= 0) return;
    sceneRef.current.update({
      stage,
      tilted: layout.tilted,
      cards: sceneCards,
      hoveredKey,
      reducedMotion: reduced,
    });
  }, [stage, layout.tilted, sceneCards, hoveredKey, reduced, sleeveUrl]);

  const viewerSeat = snapshot.viewerSeatId;

  const hitItem = (placement: Placement): ReactElement => {
    const { object, kind, zone } = placement;
    const rect = projected.get(placement.key);
    const fallbackH = placement.width * CARD_RATIO;
    const y =
      placement.y +
      (handTucked && zone === "hand" ? fallbackH * 0.3 : 0);
    const left = rect ? rect.left : placement.x - placement.width / 2;
    const top = rect ? rect.top : y - fallbackH / 2;
    const width = rect ? rect.width : placement.width;
    const height = rect ? rect.height : fallbackH;

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
    // On a small whole-table view a card is too small to press: shown, not offered.
    const tooSmall = placement.width < 44;
    const badgeText = [stats, ...counters].filter(Boolean).join(" · ");

    return (
      <div
        key={placement.key}
        className={s.hitAnchor}
        data-anchor={object ? `card:${object.handle}` : undefined}
        style={{
          left,
          top,
          width,
          height,
          zIndex:
            hoveredKey === placement.key
              ? 500 + placement.order
              : placement.zone === "hand"
                ? 200 + placement.order
                : placement.order,
        }}
      >
        {tooSmall ? (
          <span
            className={s.hitButton}
            role="img"
            aria-label={label}
            data-mark={object ? marks?.get(object.handle) : undefined}
          >
            {(kind === "library" || kind === "graveyard") && (
              <span className={s.countBadge} aria-hidden="true">
                {placement.count}
              </span>
            )}
            {badgeText && (
              <span className={s.badge} aria-hidden="true">
                {badgeText}
              </span>
            )}
          </span>
        ) : (
          <button
            type="button"
            className={s.hitButton}
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
            onPointerEnter={(event) => {
              if (event.pointerType === "mouse") setHoveredKey(placement.key);
            }}
            onPointerLeave={() => {
              setHoveredKey((current) =>
                current === placement.key ? null : current,
              );
            }}
            onFocus={() => setHoveredKey(placement.key)}
            onBlur={() => {
              setHoveredKey((current) =>
                current === placement.key ? null : current,
              );
            }}
            onClick={() => {
              if (kind === "library" || kind === "graveyard")
                onPile(kind, placement.seatId);
              else if (object)
                onCard(object, zone === "hand" ? "hand" : "battlefield");
            }}
          >
            {(kind === "library" || kind === "graveyard") && (
              <span className={s.countBadge} aria-hidden="true">
                {placement.count}
              </span>
            )}
            {badgeText && (
              <span className={s.badge} aria-hidden="true">
                {badgeText}
              </span>
            )}
          </button>
        )}
      </div>
    );
  };

  return (
    <div
      ref={root}
      className={s.stage}
      data-match-table={view.kind}
      data-renderer="three-webgl2"
      role="group"
      aria-label={
        view.kind === "overview"
          ? "Whole table"
          : view.seatId === viewerSeat
            ? "Your board"
            : `${seatNames.get(view.seatId) ?? "Player"}’s board`
      }
    >
      <canvas
        ref={canvasRef}
        className={s.canvas}
        aria-hidden="true"
      />
      <div className={s.hitLayer}>
        {layout.placements.map(hitItem)}
        {layout.labels.map((label) => (
          <span
            key={label.seatId}
            className={s.seatLabel}
            style={{ left: label.x, top: label.y }}
          >
            {seatNames.get(label.seatId) ?? "Player"}
          </span>
        ))}
      </div>
    </div>
  );
}
