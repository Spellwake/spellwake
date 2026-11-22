import { useEffect, useRef, useState, type ReactElement } from "react";
import { assignInlineVars } from "@vanilla-extract/dynamic";
import { motion, useMotionValue, useSpring } from "motion/react";
import { useAllowsControlMotion } from "@spellwake/ui";
import type { BackdropArt } from "../../lib/backdrop-art";
import * as s from "./backdrop.css";

/** Scrim used when there is no artwork, or its brightness cannot be read. */
const DEFAULT_SCRIM = 0;
const UNREADABLE_SCRIM = 0.6;
/** Brightest the artwork may read behind the controls, as relative luminance. */
const TARGET_LUMINANCE = 0.1;
const MINIMUM_SCRIM = 0.2;
const MAXIMUM_SCRIM = 0.88;

/**
 * Scrim strength for an artwork: dark paintings keep their colour, bright ones
 * are pulled down until light text and glass controls read clearly.
 */
export function scrimFor(image: HTMLImageElement): number {
  const sample = document.createElement("canvas");
  sample.width = 24;
  sample.height = 24;
  const context = sample.getContext("2d", { willReadFrequently: true });
  if (!context) return UNREADABLE_SCRIM;
  try {
    context.drawImage(image, 0, 0, sample.width, sample.height);
    const { data } = context.getImageData(0, 0, sample.width, sample.height);
    const linear = (channel: number): number => {
      const value = channel / 255;
      return value <= 0.04045
        ? value / 12.92
        : ((value + 0.055) / 1.055) ** 2.4;
    };
    let total = 0;
    for (let index = 0; index < data.length; index += 4) {
      total +=
        0.2126 * linear(data[index] ?? 0) +
        0.7152 * linear(data[index + 1] ?? 0) +
        0.0722 * linear(data[index + 2] ?? 0);
    }
    const luminance = total / (data.length / 4);
    if (luminance <= TARGET_LUMINANCE) return MINIMUM_SCRIM;
    return Math.min(
      MAXIMUM_SCRIM,
      Math.max(MINIMUM_SCRIM, 1 - TARGET_LUMINANCE / luminance),
    );
  } catch (error: unknown) {
    console.error("Backdrop artwork brightness could not be read.", error);
    return UNREADABLE_SCRIM;
  }
}

const MOTE_COUNT = 34;
/** Milliseconds between dust frames. */
const MOTE_INTERVAL = 50;
type Mote = {
  x: number;
  y: number;
  radius: number;
  drift: number;
  rise: number;
  phase: number;
};

/** Dust catching the light. Slow, sparse and low contrast: texture, never a focal point. */
function Motes(): ReactElement {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const element = canvas.current;
    const context = element?.getContext("2d");
    if (!element || !context) return;
    let width = 0;
    let height = 0;
    let frame = 0;
    let previous = performance.now();
    const motes: Mote[] = Array.from({ length: MOTE_COUNT }, () => ({
      x: Math.random(),
      y: Math.random(),
      radius: 0.6 + Math.random() * 1.5,
      drift: (Math.random() - 0.5) * 0.006,
      rise: 0.004 + Math.random() * 0.01,
      phase: Math.random() * Math.PI * 2,
    }));
    const resize = (): void => {
      const density = Math.min(window.devicePixelRatio, 2);
      width = element.clientWidth;
      height = element.clientHeight;
      element.width = Math.round(width * density);
      element.height = Math.round(height * density);
      context.setTransform(density, 0, 0, density, 0, 0);
    };
    const draw = (now: number): void => {
      frame = requestAnimationFrame(draw);
      // Glass surfaces re-blur on every change beneath them, so dust updates at a third of the display rate.
      if (now - previous < MOTE_INTERVAL) return;
      const elapsed = Math.min(0.1, (now - previous) / 1000);
      previous = now;
      context.clearRect(0, 0, width, height);
      for (const mote of motes) {
        mote.y -= mote.rise * elapsed;
        mote.x +=
          (mote.drift + Math.sin(now / 2400 + mote.phase) * 0.004) * elapsed;
        if (mote.y < -0.02) {
          mote.y = 1.02;
          mote.x = Math.random();
        }
        const twinkle = 0.5 + 0.5 * Math.sin(now / 1300 + mote.phase);
        context.globalAlpha = 0.1 + 0.32 * twinkle;
        context.fillStyle = "#ffd9a3";
        context.beginPath();
        context.arc(
          mote.x * width,
          mote.y * height,
          mote.radius,
          0,
          Math.PI * 2,
        );
        context.fill();
      }
    };
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();
    const visibility = (): void => {
      cancelAnimationFrame(frame);
      if (document.visibilityState === "visible") {
        previous = performance.now();
        frame = requestAnimationFrame(draw);
      }
    };
    document.addEventListener("visibilitychange", visibility);
    visibility();
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      document.removeEventListener("visibilitychange", visibility);
    };
  }, []);
  return <canvas ref={canvas} className={s.motes} aria-hidden="true" />;
}

/**
 * The surroundings behind every screen. It accepts any artwork: the image is
 * cropped around its own focal point, and a scrim sized to its brightness keeps
 * controls legible. Ambient motion is limited to drifting dust and a few pixels
 * of lean toward the pointer. Screens own all primary motion.
 */
export function Backdrop({ art }: { art: BackdropArt | null }): ReactElement {
  const animate = useAllowsControlMotion();
  const [ready, setReady] = useState<{ url: string; scrim: number } | null>(
    null,
  );
  const x = useMotionValue(0);
  const y = useMotionValue(0);
  const leanX = useSpring(x, { stiffness: 40, damping: 18, mass: 1.2 });
  const leanY = useSpring(y, { stiffness: 40, damping: 18, mass: 1.2 });
  useEffect(() => {
    if (!animate || !art || !window.matchMedia("(pointer: fine)").matches)
      return;
    const follow = (event: PointerEvent): void => {
      x.set((0.5 - event.clientX / window.innerWidth) * 22);
      y.set((0.5 - event.clientY / window.innerHeight) * 12);
    };
    window.addEventListener("pointermove", follow, { passive: true });
    return () => window.removeEventListener("pointermove", follow);
  }, [animate, art, x, y]);
  const isReady = art !== null && ready?.url === art.url;
  return (
    <div
      className={s.root}
      aria-hidden="true"
      style={assignInlineVars({
        [s.scrim]: String(isReady ? ready.scrim : DEFAULT_SCRIM),
        [s.focusX]: `${(art?.focus.x ?? 0.5) * 100}%`,
        [s.focusY]: `${(art?.focus.y ?? 0.5) * 100}%`,
      })}
    >
      {art && (
        <motion.img
          key={art.url}
          className={s.art}
          src={art.url}
          alt=""
          draggable={false}
          data-ready={isReady}
          style={{ x: leanX, y: leanY }}
          onLoad={(event) =>
            setReady({ url: art.url, scrim: scrimFor(event.currentTarget) })
          }
          onError={() => console.error("Backdrop artwork failed to load.")}
        />
      )}
      {animate && <Motes />}
      <div className={s.veil} />
    </div>
  );
}
