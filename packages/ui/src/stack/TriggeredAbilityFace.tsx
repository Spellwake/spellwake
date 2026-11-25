import { isValidElement, useMemo, type ReactNode } from "react";
import { CustomCardFrame } from "../cards/CustomCardFrame";
import type {
  CardFrameAccent,
  StackTriggerFrameInput,
} from "../cards/card-frame-svg";
import * as s from "./stack.css";

export interface TriggeredAbilityContent {
  name: string;
  /** Optional artwork URL rendered inside the unified 1062x1482 stack trigger frame. */
  artUrl?: string | null;
  /** Whether `artUrl` is pre-cropped artwork (`"art"`) or a full 5:7 printed card face (`"full_card"`). */
  artSourceKind?: "art" | "full_card";
  /** Optional color-identity pinline accent for the stack trigger frame. */
  frameAccent?: CardFrameAccent;
  /** Legacy artwork element; if `artUrl` is omitted and `art` is an `<img>`, its `src` is used. */
  art?: ReactNode;
  typeLabel: string;
  effect: string;
}

function extractImageSrc(node: ReactNode): string | null {
  if (!isValidElement(node)) return null;
  const props = node.props as { src?: unknown } | null;
  return typeof props?.src === "string" && props.src.trim().length > 0
    ? props.src
    : null;
}

/**
 * Stack ability face consolidated onto Spellwake's unified 1062x1482 procedural
 * `CustomCardFrame` (`kind: "stack_trigger"`). Short effects with art render in
 * illustrated mode; longer effects automatically expand into full-height text mode.
 */
export function TriggeredAbilityFace({
  name,
  art,
  artUrl,
  artSourceKind,
  frameAccent,
  typeLabel,
  effect,
}: TriggeredAbilityContent) {
  const resolvedArtUrl = artUrl ?? extractImageSrc(art);
  const illustrated = Boolean(resolvedArtUrl) && effect.trim().length <= 100;

  const frameInput = useMemo<StackTriggerFrameInput>(
    () => ({
      kind: "stack_trigger",
      name,
      typeLabel,
      effect,
      artUrl: resolvedArtUrl,
      illustrated,
      ...(artSourceKind ? { artSourceKind } : {}),
      ...(frameAccent ? { frameAccent } : {}),
    }),
    [name, typeLabel, effect, resolvedArtUrl, illustrated, artSourceKind, frameAccent],
  );

  return (
    <div className={s.ability} data-illustrated={illustrated}>
      <CustomCardFrame input={frameInput} />
    </div>
  );
}
