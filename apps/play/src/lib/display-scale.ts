import { useSyncExternalStore } from "react";

/** Density families. Each receives its own design pass for screen content. */
export type DeviceClass = "phone" | "tablet" | "desktop";
/**
 * How the frame presents navigation. "wide" is the header with destinations and
 * a side rail. "compact" is the header, a section strip and a floating dock.
 */
export type ShellLayout = "compact" | "wide";
export type Orientation = "portrait" | "landscape";

export type DisplayScale = {
  device: DeviceClass;
  shell: ShellLayout;
  orientation: Orientation;
  scale: number;
};

/**
 * Shared controls are authored in the pixel scale of the approved reference
 * rasters, which are 1586 px wide for a 1440 px desktop window. Each layout
 * maps that scale onto the real viewport, so the approved proportions hold at
 * any window size while layouts stay free to reflow.
 */
const DESIGN_WIDTH = {
  desktop: 1586,
  /** A tablet on its side keeps the wide frame at a tighter density. */
  tabletWide: 1320,
  /** An upright tablet uses the compact frame at close to full size. */
  tabletCompact: 800,
  phone: 540,
} as const;
/** Narrower than this, a tablet cannot hold the rail and a useful content column. */
const TABLET_WIDE_MIN_WIDTH = 900;
/** Below this design height the desktop scale follows the window height instead. */
const DESKTOP_MIN_DESIGN_HEIGHT = 860;
/**
 * The owner asked for desktop text and controls about 15% smaller than the
 * approved boards (November 21, 2026), and for a larger window to give more
 * room rather than larger type: the desktop scale stops at what a 1440 px
 * wide window gives.
 */
const DESKTOP_DENSITY = 0.85;
const DESKTOP_MAX_SCALE = (DESKTOP_DENSITY * 1440) / DESIGN_WIDTH.desktop;
const SCALE_LIMITS: Record<DeviceClass, readonly [number, number]> = {
  desktop: [0.6, DESKTOP_MAX_SCALE],
  tablet: [0.62, 1],
  phone: [0.66, 0.9],
};

const clamp = (value: number, [low, high]: readonly [number, number]) =>
  Math.min(high, Math.max(low, value));

/**
 * Classify by the short side so a rotated phone or tablet keeps its family.
 * A short window on a pointer device is a laptop, not a tablet, so it keeps
 * the desktop composition at a smaller scale.
 */
export function resolveDisplayScale(
  width: number,
  height: number,
  isTouch: boolean,
): DisplayScale {
  const short = Math.min(width, height);
  const device: DeviceClass =
    short < 600
      ? "phone"
      : width < 1100 || (isTouch && short < 900)
        ? "tablet"
        : "desktop";
  const shell: ShellLayout =
    device === "phone" || (device === "tablet" && width < TABLET_WIDE_MIN_WIDTH)
      ? "compact"
      : "wide";
  const designWidth =
    device === "desktop"
      ? DESIGN_WIDTH.desktop
      : device === "phone"
        ? DESIGN_WIDTH.phone
        : shell === "compact"
          ? DESIGN_WIDTH.tabletCompact
          : DESIGN_WIDTH.tabletWide;
  const byWidth = width / designWidth;
  const scale =
    device === "desktop"
      ? DESKTOP_DENSITY * Math.min(byWidth, height / DESKTOP_MIN_DESIGN_HEIGHT)
      : byWidth;
  return {
    device,
    shell,
    orientation: width > height ? "landscape" : "portrait",
    scale: Math.round(clamp(scale, SCALE_LIMITS[device]) * 1000) / 1000,
  };
}

/** Stylesheet lengths are compiled to rem against this root size. See scripts/design-pixels.ts. */
const ROOT_PIXELS = 16;
let current: DisplayScale | null = null;
/** Readers who raised their browser's default text size keep that preference. */
let readerScale = 1;
const listeners = new Set<() => void>();

function apply(): void {
  const next = resolveDisplayScale(
    window.innerWidth,
    window.innerHeight,
    window.matchMedia("(pointer: coarse)").matches,
  );
  if (
    current &&
    next.device === current.device &&
    next.shell === current.shell &&
    next.orientation === current.orientation &&
    next.scale === current.scale
  )
    return;
  current = next;
  const root = document.documentElement;
  root.dataset.device = next.device;
  root.dataset.shell = next.shell;
  root.dataset.orientation = next.orientation;
  root.style.fontSize = `${ROOT_PIXELS * next.scale * readerScale}px`;
  for (const listener of listeners) listener();
}

/** Start tracking the viewport. Call once, before the first render. */
export function startDisplayScale(): void {
  const preferred = parseFloat(
    getComputedStyle(document.documentElement).fontSize,
  );
  readerScale = Number.isFinite(preferred) ? preferred / ROOT_PIXELS : 1;
  apply();
  let frame = 0;
  window.addEventListener("resize", () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(apply);
  });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Whether the viewport is wider than it is tall. */
export function useOrientation(): Orientation {
  return useSyncExternalStore(
    subscribe,
    () => current?.orientation ?? "landscape",
    () => "landscape",
  );
}

/** The frame layout for the current viewport. */
export function useShellLayout(): ShellLayout {
  return useSyncExternalStore(
    subscribe,
    () => current?.shell ?? "wide",
    () => "wide",
  );
}

/** The device family, by the viewport's short side. A phone on its side is still a phone. */
export function useDeviceClass(): DeviceClass {
  return useSyncExternalStore(
    subscribe,
    () => current?.device ?? "desktop",
    () => "desktop",
  );
}
