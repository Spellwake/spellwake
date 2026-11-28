import {
  useEffect,
  useId,
  useRef,
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
  Droplets,
  GalleryVerticalEnd,
  Layers,
  PawPrint,
  RectangleVertical,
  RotateCcw,
  Scroll,
  Shuffle,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { LoadingShape, LoadingState } from "@spellwake/ui";
import {
  paintPlaymatCanvas,
  resolvePlaymat,
} from "spellwake-renderer-benchmark/playmats";
import type { PlaymatEffectId } from "spellwake-renderer-benchmark/playmat-effects";
import {
  paintCardWearPreview,
  paintSleeveWearPreview,
  type WearTierId,
} from "spellwake-renderer-benchmark/wear-effects";
import {
  KIND_LABEL,
  type CosmeticItem,
  type CosmeticKind,
} from "../../cosmetics/cosmetic-sources";
import { usePlaymatEffectSeeds } from "../../cosmetics/playmat-effects";
import { useActivePlaymat } from "../../cosmetics/use-active-playmat";
import { useWearSeeds, type WearTarget } from "../../cosmetics/wear-effects";
import { motionLanguage as m } from "../../lib/motion";
import * as s from "./cosmetics.css";

const KIND_ICON: Readonly<Record<CosmeticKind, LucideIcon>> = {
  sleeve: GalleryVerticalEnd,
  back: RectangleVertical,
  dice: Box,
  playmat: Scroll,
  card_effect: Sparkles,
  sleeve_effect: Layers,
  playmat_effect: Droplets,
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

function WearLookCanvas({
  target,
  tier,
  size,
}: {
  target: WearTarget;
  tier: WearTierId;
  size: "tile" | "preview";
}): ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { seedFor } = useWearSeeds();
  const seed = seedFor(target, tier);
  const width = size === "preview" ? 200 : 120;
  const height = size === "preview" ? 280 : 168;

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    if (target === "card") {
      paintCardWearPreview(ctx, width, height, tier, seed);
    } else {
      paintSleeveWearPreview(ctx, width, height, tier, seed);
    }
  }, [target, tier, seed, width, height]);

  return (
    <span className={s.wearFrame} aria-hidden="true">
      <canvas
        ref={canvasRef}
        width={width}
        height={height}
        className={s.wearCanvas}
      />
    </span>
  );
}

/** Pattern seed display + Randomize / Reset buttons for wear and playmat effects. */
export function WearSeedControls({
  item,
}: {
  item: CosmeticItem | null;
}): ReactElement | null {
  const wearSeeds = useWearSeeds();
  const playmatSeeds = usePlaymatEffectSeeds();

  if (item?.look && "playmatEffect" in item.look) {
    const effectId = item.look.playmatEffect.id;
    if (effectId === "clean") return null;
    const seed = playmatSeeds.seedFor(effectId);
    const customized = playmatSeeds.isCustomSeed(effectId);
    return (
      <div className={s.seedControls}>
        <span className={s.seedTag}>
          Pattern #{seed}
          {customized ? " · Custom" : " · Default"}
        </span>
        <div className={s.seedActions}>
          <button
            type="button"
            className={s.seedButton}
            onClick={() => playmatSeeds.randomizeSeed(effectId)}
          >
            <Shuffle aria-hidden="true" />
            <span>Randomize</span>
          </button>
          {customized && (
            <button
              type="button"
              className={s.seedButton}
              onClick={() => playmatSeeds.resetSeed(effectId)}
            >
              <RotateCcw aria-hidden="true" />
              <span>Reset</span>
            </button>
          )}
        </div>
      </div>
    );
  }

  if (!item?.look || !("wear" in item.look)) return null;
  const { target, tier } = item.look.wear;
  if (tier === "nm") return null;
  const seed = wearSeeds.seedFor(target, tier);
  const customized = wearSeeds.isCustomSeed(target, tier);
  return (
    <div className={s.seedControls}>
      <span className={s.seedTag}>
        Pattern #{seed}
        {customized ? " · Custom" : " · Default"}
      </span>
      <div className={s.seedActions}>
        <button
          type="button"
          className={s.seedButton}
          onClick={() => wearSeeds.randomizeSeed(target, tier)}
        >
          <Shuffle aria-hidden="true" />
          <span>Randomize</span>
        </button>
        {customized && (
          <button
            type="button"
            className={s.seedButton}
            onClick={() => wearSeeds.resetSeed(target, tier)}
          >
            <RotateCcw aria-hidden="true" />
            <span>Reset</span>
          </button>
        )}
      </div>
    </div>
  );
}

const PLAYMAT_TILE_CACHE = new Map<string, string>();

function PlaymatLookCanvas({
  playmatId,
  size,
}: {
  playmatId: string;
  size: "tile" | "preview";
}): ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const spec = resolvePlaymat(playmatId);
  const width = size === "preview" ? 560 : 240;
  const height = size === "preview" ? 350 : 150;
  const cacheKey = `${playmatId}:${width}x${height}`;

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || !spec) return;

    if (size === "tile") {
      const cached = PLAYMAT_TILE_CACHE.get(cacheKey);
      if (cached) {
        const img = new Image();
        img.onload = () => {
          ctx.clearRect(0, 0, width, height);
          ctx.drawImage(img, 0, 0, width, height);
        };
        img.src = cached;
        return;
      }
    }

    return paintPlaymatCanvas(ctx, width, height, spec, {
      assetBaseUrl: import.meta.env.BASE_URL,
      onReady: () => {
        if (size === "tile") {
          try {
            PLAYMAT_TILE_CACHE.set(cacheKey, canvas.toDataURL("image/jpeg", 0.88));
          } catch {
            // Canvas may be tainted in non-standard preview hosts; skip caching.
          }
        }
      },
    });
  }, [spec, size, width, height, cacheKey]);

  return (
    <span className={s.playmatFrame} aria-hidden="true">
      <canvas
        ref={canvasRef}
        width={width}
        height={height}
        className={s.playmatCanvas}
      />
    </span>
  );
}

function PlaymatEffectLookCanvas({
  effectId,
  size,
}: {
  effectId: PlaymatEffectId;
  size: "tile" | "preview";
}): ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const equippedPlaymatId = useActivePlaymat();
  const { seedFor } = usePlaymatEffectSeeds();
  const seed = seedFor(effectId);
  const width = size === "preview" ? 560 : 240;
  const height = size === "preview" ? 350 : 150;
  // Preview on the player's equipped playmat when set; otherwise on a crisp neutral playmat.
  const baseSpec =
    resolvePlaymat(equippedPlaymatId) ??
    resolvePlaymat(effectId === "dusty" ? "obsidian-and-brass" : "woven-natural-linen");

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || !baseSpec) return;
    return paintPlaymatCanvas(ctx, width, height, baseSpec, {
      assetBaseUrl: import.meta.env.BASE_URL,
      effect: { id: effectId, seed },
    });
  }, [baseSpec, effectId, seed, width, height]);

  return (
    <span className={s.playmatFrame} aria-hidden="true">
      <canvas
        ref={canvasRef}
        width={width}
        height={height}
        className={s.playmatCanvas}
      />
    </span>
  );
}

const paintOf = (item: CosmeticItem): string | null =>
  !item.look ||
  "wear" in item.look ||
  "playmat" in item.look ||
  "playmatEffect" in item.look
    ? null
    : "imageUrl" in item.look
      ? `url("${item.look.imageUrl}")`
      : item.look.paint;

/**
 * One item drawn as what it is: a card for sleeves and backs, a die, a mat,
 * or a live procedural wear preview. An item with no artwork draws as an empty frame.
 */
export function ItemLook({
  item,
  kind,
  size = "tile",
}: {
  item: CosmeticItem | null;
  kind: CosmeticKind;
  size?: "tile" | "preview";
}): ReactElement {
  if (item?.look && "wear" in item.look) {
    return (
      <WearLookCanvas
        target={item.look.wear.target}
        tier={item.look.wear.tier}
        size={size}
      />
    );
  }
  if (item?.look && "playmat" in item.look) {
    return <PlaymatLookCanvas playmatId={item.look.playmat.id} size={size} />;
  }
  if (item?.look && "playmatEffect" in item.look) {
    return (
      <PlaymatEffectLookCanvas
        effectId={item.look.playmatEffect.id}
        size={size}
      />
    );
  }
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
  if (item?.look && "wear" in item.look) {
    shown = (
      <span className={s.single}>
        <ItemLook item={item} kind={kind} size="preview" />
      </span>
    );
  } else if (
    item?.look &&
    ("playmat" in item.look || "playmatEffect" in item.look)
  ) {
    shown = (
      <span className={s.bigMat}>
        <ItemLook item={item} kind={kind} size="preview" />
      </span>
    );
  } else if (item && paintOf(item)) {
    if (kind === "sleeve")
      shown = (
        <span className={s.fan}>
          {(["left", "right", "middle"] as const).map((side) => (
            <span key={side} className={s.fanned} data-side={side}>
              <ItemLook item={item} kind={kind} size="preview" />
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
          <ItemLook item={item} kind={kind} size="preview" />
        </span>
      );
  } else if (item)
    shown = (
      <span className={s.single}>
        <ItemLook item={item} kind={kind} size="preview" />
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
