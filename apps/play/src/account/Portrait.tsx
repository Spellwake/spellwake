import { useState, type CSSProperties, type ReactElement } from "react";
import { assignInlineVars } from "@vanilla-extract/dynamic";
import { CircleUserRound } from "lucide-react";
import { designPixels } from "@spellwake/ui";
import type { BadgeChoice, Portrait as PortraitData } from "./account-sources";
import { coveringCrop, type PortraitCrop } from "./portrait-crop";
import * as s from "./portrait.css";

/** Card artwork is wider than tall. Used until the image reports its own shape. */
const USUAL_ASPECT = 626 / 457;

/** How artwork is placed so that `crop` sits under a circle the size of its box. */
export function artPlacement(
  crop: PortraitCrop,
  aspect: number,
): CSSProperties {
  const placed = coveringCrop(crop, aspect);
  return {
    [aspect >= 1 ? "height" : "width"]: `${placed.scale * 100}%`,
    transform: `translate(${-placed.x * 100}%, ${-placed.y * 100}%)`,
  };
}

/** Artwork drawn at its crop. It fades in once its shape is known, so it never jumps. */
export function CroppedArt({
  artUrl,
  crop,
  onAspect,
}: PortraitData & {
  /** Reports the artwork's width over height once it has loaded. */
  onAspect?: (aspect: number) => void;
}): ReactElement {
  const [loaded, setLoaded] = useState<{ url: string; aspect: number } | null>(
    null,
  );
  const aspect = loaded?.url === artUrl ? loaded.aspect : null;
  return (
    <img
      className={s.art}
      src={artUrl}
      alt=""
      draggable={false}
      data-art-ready={aspect !== null}
      style={artPlacement(crop, aspect ?? USUAL_ASPECT)}
      onLoad={(event) => {
        const image = event.currentTarget;
        if (image.naturalWidth === 0 || image.naturalHeight === 0) return;
        const next = image.naturalWidth / image.naturalHeight;
        setLoaded({ url: artUrl, aspect: next });
        onAspect?.(next);
      }}
    />
  );
}

/** The eight-point mark of a current supporter. */
export function SupporterMark(): ReactElement {
  return (
    <svg className={s.badgeMark} viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="currentColor"
        d="M12 1.5l2 6.2 5.4-3.1-3.1 5.4 6.2 2-6.2 2 3.1 5.4-5.4-3.1-2 6.2-2-6.2-5.4 3.1 3.1-5.4-6.2-2 6.2-2-3.1-5.4L10 7.7z"
      />
    </svg>
  );
}

/**
 * A person's round picture with their badge attached. `label` names it for
 * assistive technology. Leave it out where the name is written beside it.
 */
export function Portrait({
  portrait,
  badge = "none",
  diameter,
  label,
}: {
  portrait: PortraitData | null;
  badge?: BadgeChoice;
  /** In design pixels. */
  diameter: number;
  label?: string;
}): ReactElement {
  return (
    <span
      className={s.portrait}
      style={assignInlineVars({ [s.size]: designPixels(diameter) })}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <span className={s.clip}>
        {portrait ? (
          <CroppedArt {...portrait} />
        ) : (
          <CircleUserRound className={s.blank} strokeWidth={1.1} />
        )}
      </span>
      {badge === "supporter" && (
        <span className={s.badge} data-badge="supporter">
          <SupporterMark />
        </span>
      )}
    </span>
  );
}
