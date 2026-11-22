import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type KeyboardEvent,
} from "react";
import { motion } from "motion/react";
import { OriginalCardFace, type CardImage } from "../cards/OriginalCardFace";
import { PlayerAvatar } from "../identity/PlayerAvatar";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { TargetConnection, type AnchorPoint } from "./TargetConnection";
import {
  TriggeredAbilityFace,
  type TriggeredAbilityContent,
} from "./TriggeredAbilityFace";
import * as s from "./stack.css";

type StackTarget =
  | { kind: "item"; id: string }
  | { kind: "board"; label: string; anchor: AnchorPoint };
export type StackEntry = {
  id: string;
  controller: string;
  portrait: ReactNode;
  target?: StackTarget;
} & (
  | { kind: "spell"; image: CardImage }
  | ({ kind: "ability" } & TriggeredAbilityContent)
);
export interface CompactSpellStackProps {
  /** Stable object handles, oldest first. Board endpoints are relative to the front card's top left. */
  items: readonly StackEntry[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
}
const width = 178,
  height = 224;
const title = (item: StackEntry) =>
  item.kind === "spell" ? item.image.name : item.name;

function pose(depth: number, count: number, reading: boolean, raised: boolean) {
  if (raised && depth === 0) return { x: 4, y: 13, rotate: 0, scaleY: 0.89 };
  // Keep the remaining peeks spread across the same footprint while the reader sits in front.
  const spacing = raised
    ? ((depth - 1) * Math.min(count - 1, 5)) / Math.min(count - 2, 4)
    : depth;
  const step = Math.min(spacing, 5);
  if (count === 2 && depth === 1)
    return { x: reading ? -50 : -28, y: -21, rotate: -7, scaleY: 1 };
  return {
    x: -21 * step,
    y: -20 * step,
    rotate: -Math.min(7, step * 2),
    scaleY: 1,
  };
}

/** Project a point on the printed face through the same centered transform as its visual slot. */
function facePoint(
  position: ReturnType<typeof pose>,
  x: number,
  y: number,
): AnchorPoint {
  const angle = (position.rotate * Math.PI) / 180;
  const u = x - width / 2,
    v = (y - height / 2) * position.scaleY;
  return {
    x: position.x + width / 2 + u * Math.cos(angle) - v * Math.sin(angle),
    y: position.y + height / 2 + u * Math.sin(angle) + v * Math.cos(angle),
  };
}

/** Selection raises a reading copy within the pile. Only the host changes the canonical item list. */
export function CompactSpellStack({
  items,
  selectedId,
  onSelect,
}: CompactSpellStackProps) {
  const root = useRef<HTMLDivElement>(null),
    lastWheel = useRef(-Infinity),
    optPrefix = useId();
  const animate = useAllowsControlMotion();
  const selected = items.find((entry) => entry.id === selectedId);
  const paint = selected
    ? [...items.filter((entry) => entry !== selected), selected]
    : items;
  const raised = Boolean(
    selected && selected !== items.at(-1) && items.length > 2,
  );
  const poses = new Map(
    paint.map((entry, i) => [
      entry.id,
      {
        position: pose(
          paint.length - 1 - i,
          paint.length,
          Boolean(selected),
          raised,
        ),
        z: i + 1,
      },
    ]),
  );
  const selectedIndex = selected ? items.indexOf(selected) : -1;
  function step(direction: number) {
    if (!items.length) return;
    const index =
      selectedIndex < 0
        ? direction < 0
          ? items.length - 1
          : 0
        : Math.max(0, Math.min(items.length - 1, selectedIndex + direction));
    onSelect(items[index]!.id);
  }
  const handleWheel = useRef<(event: WheelEvent) => void>(() => {});
  useEffect(() => {
    handleWheel.current = (event) => {
      if (event.ctrlKey || Math.abs(event.deltaY) < 2) return;
      const elapsed = event.timeStamp - lastWheel.current;
      event.preventDefault();
      if (elapsed < 150) return;
      lastWheel.current = event.timeStamp;
      step(event.deltaY > 0 ? -1 : 1);
    };
  });
  const present = items.length > 0;
  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const wheel = (e: WheelEvent) => handleWheel.current(e);
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [present]);
  useEffect(() => {
    if (!selected) return;
    const clear = (e: PointerEvent) => {
      if (e.target instanceof Node && !root.current?.contains(e.target))
        onSelect(null);
    };
    const escape = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) onSelect(null);
    };
    document.addEventListener("pointerdown", clear);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", clear);
      document.removeEventListener("keydown", escape);
    };
  }, [selected, onSelect]);
  function keydown(event: KeyboardEvent) {
    if (!items.length) return;
    switch (event.key) {
      case "ArrowLeft":
      case "ArrowUp":
        step(-1);
        break;
      case "ArrowRight":
      case "ArrowDown":
        step(1);
        break;
      case "Home":
        onSelect(items[0]!.id);
        break;
      case "End":
        onSelect(items[items.length - 1]!.id);
        break;
      case "Enter":
      case " ":
        onSelect(selected ? null : items[items.length - 1]!.id);
        break;
      case "Escape":
        onSelect(null);
        break;
      default:
        return;
    }
    event.preventDefault();
  }
  if (!present) return null;
  const target = selected?.target;
  const targetItem =
    target?.kind === "item"
      ? items.find((item) => item.id === target.id)
      : undefined;
  const targetPose = targetItem
    ? poses.get(targetItem.id)?.position
    : undefined;
  const endpoint =
    target?.kind === "board"
      ? target.anchor
      : targetPose
        ? facePoint(targetPose, 10, height * 0.39)
        : undefined;
  const readingPose = selected ? poses.get(selected.id)?.position : undefined;
  const rightSide = endpoint && endpoint.x > width / 2;
  const origin = readingPose
    ? facePoint(
        readingPose,
        rightSide ? width + 2 : -2,
        target?.kind === "item" ? height * 0.46 : height * 0.105,
      )
    : undefined;
  const targetName =
    target?.kind === "board"
      ? target.label
      : targetItem
        ? title(targetItem)
        : undefined;
  return (
    <div
      ref={root}
      role="listbox"
      aria-label="Inspect stack"
      tabIndex={0}
      className={s.pile}
      style={{ width, height }}
      aria-activedescendant={
        selected ? `${optPrefix}-${selected.id}` : undefined
      }
      onKeyDown={keydown}
    >
      {items.map((item, i) => {
        const active = selected === item,
          { position, z } = poses.get(item.id)!;
        return (
          <motion.div
            key={item.id}
            role="option"
            id={`${optPrefix}-${item.id}`}
            aria-selected={active}
            aria-description={
              item.kind === "ability"
                ? item.typeLabel + ". " + item.effect
                : undefined
            }
            data-ability={item.kind === "ability" || undefined}
            aria-posinset={i + 1}
            aria-setsize={items.length}
            aria-label={`${title(item)}, ${item.controller}${active && targetName ? ", targeting " + targetName : ""}`}
            data-front={i === items.length - 1 || undefined}
            data-item={item.id}
            className={s.card}
            style={{ zIndex: z }}
            initial={false}
            animate={position}
            transition={{
              duration: animate ? controlMotion.settle : 0,
              ease: controlMotion.ease,
            }}
            onClick={() => {
              root.current?.focus({ preventScroll: true });
              onSelect(active ? null : item.id);
            }}
          >
            <div className={s.surface} aria-hidden>
              {item.kind === "spell" ? (
                <div
                  style={{
                    transform: `scaleY(${(height * item.image.width) / (width * item.image.height)})`,
                    transformOrigin: "top left",
                  }}
                >
                  <OriginalCardFace image={item.image} />
                </div>
              ) : (
                <TriggeredAbilityFace {...item} />
              )}
            </div>
            {active && (
              <span className={s.caster} aria-hidden>
                <PlayerAvatar diameter={31}>{item.portrait}</PlayerAvatar>
              </span>
            )}
          </motion.div>
        );
      })}
      {endpoint && origin && (
        <TargetConnection
          key={selected?.id}
          from={origin}
          to={endpoint}
          bend={
            target?.kind === "item"
              ? 4
              : (rightSide ? 1 : -1) *
                Math.min(
                  192,
                  Math.hypot(endpoint.x - origin.x, endpoint.y - origin.y) *
                    0.18,
                )
          }
        />
      )}
    </div>
  );
}
