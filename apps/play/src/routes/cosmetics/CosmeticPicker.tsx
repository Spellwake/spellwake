import {
  useId,
  type CSSProperties,
  type KeyboardEvent,
  type ReactElement,
  type ReactNode,
} from "react";
import { assignInlineVars } from "@vanilla-extract/dynamic";
import { motion } from "motion/react";
import {
  Ban,
  Box,
  Check,
  GalleryVerticalEnd,
  PawPrint,
  RectangleVertical,
  Scroll,
  type LucideIcon,
} from "lucide-react";
import { LoadingShape, LoadingState } from "@spellwake/ui";
import {
  KIND_LABEL,
  type CosmeticItem,
  type CosmeticKind,
} from "../../cosmetics/cosmetic-sources";
import { motionLanguage as m } from "../../lib/motion";
import * as s from "./cosmetics.css";

const KIND_ICON: Readonly<Record<CosmeticKind, LucideIcon>> = {
  sleeve: GalleryVerticalEnd,
  back: RectangleVertical,
  dice: Box,
  playmat: Scroll,
  pet: PawPrint,
};
/** Pip positions for the five face, which the approved dice show. */
const FIVE: readonly (readonly [number, number])[] = [
  [28, 28],
  [72, 28],
  [50, 50],
  [28, 72],
  [72, 72],
];

const paintOf = (item: CosmeticItem): string | null =>
  !item.look
    ? null
    : "imageUrl" in item.look
      ? `url("${item.look.imageUrl}")`
      : item.look.paint;

/**
 * One item drawn as what it is: a card for sleeves and backs, a die, or a
 * mat. An item with no artwork, and the None choice, draw as an empty frame.
 */
export function ItemLook({
  item,
  kind,
}: {
  item: CosmeticItem | null;
  kind: CosmeticKind;
}): ReactElement {
  const painted = item ? paintOf(item) : null;
  if (!item || !painted) {
    const Icon = item ? KIND_ICON[kind] : Ban;
    return (
      <span className={s.blank} aria-hidden="true">
        {/* None is an empty frame. A known item without artwork shows its kind. */}
        {item && <Icon />}
      </span>
    );
  }
  const style = assignInlineVars({ [s.look]: painted }) as CSSProperties;
  if (kind === "dice")
    return (
      <span className={s.die} style={style} aria-hidden="true">
        {FIVE.map(([left, top]) => (
          <span
            key={`${left}-${top}`}
            className={s.pip}
            style={{ left: `${left}%`, top: `${top}%` }}
          />
        ))}
      </span>
    );
  return (
    <span
      className={kind === "playmat" ? s.mat : s.card}
      style={style}
      aria-hidden="true"
    />
  );
}

/** The chosen item, large, in its own light. A sleeve is shown as a fan of three. */
export function ItemPreview({
  item,
  kind,
}: {
  item: CosmeticItem | null;
  kind: CosmeticKind;
}): ReactElement {
  let shown: ReactNode = null;
  if (item && paintOf(item)) {
    if (kind === "sleeve")
      shown = (
        <span className={s.fan}>
          {(["left", "right", "middle"] as const).map((side) => (
            <span key={side} className={s.fanned} data-side={side}>
              <ItemLook item={item} kind={kind} />
            </span>
          ))}
        </span>
      );
    else
      shown = (
        <span
          className={
            kind === "dice"
              ? s.bigDie
              : kind === "playmat"
                ? s.bigMat
                : s.single
          }
        >
          <ItemLook item={item} kind={kind} />
        </span>
      );
  } else if (item)
    shown = (
      <span className={s.single}>
        <ItemLook item={item} kind={kind} />
      </span>
    );
  return (
    <span className={s.stage} aria-hidden="true">
      <span className={s.floor} />
      {/* None previews as an empty stage. */}
      {shown && (
        <span key={`${kind}:${item?.id ?? "none"}`} className={s.shown}>
          {shown}
        </span>
      )}
    </span>
  );
}

/** The five categories, as a tab list that reads top to bottom or side to side. */
export function CategoryList({
  categories,
  current,
  onChange,
  panelId,
}: {
  categories: readonly CosmeticKind[];
  current: CosmeticKind;
  onChange: (kind: CosmeticKind) => void;
  panelId: string;
}): ReactElement {
  const id = useId();
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const index = categories.indexOf(current);
    const step =
      event.key === "ArrowDown" || event.key === "ArrowRight"
        ? 1
        : event.key === "ArrowUp" || event.key === "ArrowLeft"
          ? -1
          : 0;
    const next =
      event.key === "Home"
        ? categories[0]
        : event.key === "End"
          ? categories.at(-1)
          : step !== 0
            ? categories[(index + step + categories.length) % categories.length]
            : undefined;
    if (!next) return;
    event.preventDefault();
    onChange(next);
    event.currentTarget
      .querySelector<HTMLElement>(`[data-kind="${next}"]`)
      ?.focus();
  };
  return (
    <div
      role="tablist"
      aria-label="Categories"
      aria-orientation="vertical"
      className={s.categories}
      onKeyDown={onKeyDown}
    >
      {categories.map((kind) => {
        const Icon = KIND_ICON[kind];
        const isSelected = kind === current;
        return (
          <button
            key={kind}
            type="button"
            role="tab"
            data-kind={kind}
            className={s.category}
            aria-selected={isSelected}
            aria-controls={panelId}
            tabIndex={isSelected ? 0 : -1}
            onClick={() => onChange(kind)}
          >
            {isSelected && (
              <motion.span
                layoutId={`${id}-plate`}
                className={s.categoryPlate}
                transition={{
                  duration: m.duration.marker,
                  ease: m.ease.travel,
                }}
              />
            )}
            <Icon aria-hidden="true" />
            <span className={s.categoryLabel}>{KIND_LABEL[kind]}</span>
          </button>
        );
      })}
    </div>
  );
}

export type Tile = {
  /** Null is the None choice. */
  item: CosmeticItem | null;
  /** Shown under the name, such as a price. */
  detail?: string | null;
};

/** The choices of one category. The selected one is marked, and a tick shows the chosen default. */
export function TileGrid({
  kind,
  tiles,
  selectedId,
  showTick,
  onSelect,
  state,
  emptyText,
}: {
  kind: CosmeticKind;
  tiles: readonly Tile[];
  selectedId: string | null;
  /** True where the selection is a choice being made, as in Default settings. */
  showTick: boolean;
  onSelect: (itemId: string | null) => void;
  /** "failed" with nothing loaded shows an empty gallery, never "nothing here". */
  state: "loading" | "ready" | "failed";
  emptyText: string;
}): ReactElement {
  if (tiles.length === 0) {
    if (state === "loading")
      return (
        <LoadingState label="Loading cosmetics">
          <div className={s.grid}>
            {[0, 1, 2, 3, 4, 5].map((index) => (
              <LoadingShape key={index} width="100%" height="180px" />
            ))}
          </div>
        </LoadingState>
      );
    return <p className={s.empty}>{state === "ready" ? emptyText : ""}</p>;
  }
  return (
    <div className={s.grid}>
      {tiles.map(({ item, detail }) => {
        const itemId = item?.id ?? null;
        const isSelected = itemId === selectedId;
        return (
          <button
            key={itemId ?? "none"}
            type="button"
            className={s.tile}
            aria-pressed={isSelected}
            onClick={() => onSelect(itemId)}
          >
            <span className={s.tileArt}>
              <ItemLook item={item} kind={kind} />
              {showTick && isSelected && (
                <span className={s.tick} aria-hidden="true">
                  <Check />
                </span>
              )}
            </span>
            <span className={s.tileName}>{item?.name ?? "None"}</span>
            {detail && <span className={s.tilePrice}>{detail}</span>}
          </button>
        );
      })}
    </div>
  );
}
