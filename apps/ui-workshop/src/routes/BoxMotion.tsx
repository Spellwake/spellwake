import { useEffect, useRef, useState, type ReactElement } from "react";
import { Button, Slider } from "@spellwake/ui";
import type { BoxMotion as MotionHandle } from "../lib/box-motion";
import "../workshop.css";
import * as s from "./box-motion.css";

export function BoxMotion(): ReactElement {
  const host = useRef<HTMLDivElement>(null);
  const motion = useRef<MotionHandle | null>(null);
  const [ready, setReady] = useState(false);
  const [failed, setFailed] = useState(false);
  const [separated, setSeparated] = useState(true);
  const [guides, setGuides] = useState(true);
  const [rear, setRear] = useState(0);
  const [front, setFront] = useState(0);
  useEffect(() => {
    let cancelled = false;
    document.title = "Case & flap · Spellwake";
    if (host.current) {
      const element = host.current;
      void import("../lib/box-motion")
        .then(({ mountBoxMotion }) =>
          cancelled
            ? null
            : mountBoxMotion(
                element,
                `${import.meta.env.BASE_URL}models/deck-box-parts.glb?cover=4`,
              ),
        )
        .then((handle) => {
          if (!handle) return;
          if (cancelled) handle.dispose();
          else {
            motion.current = handle;
            setReady(true);
          }
        })
        .catch((error: unknown) => {
          console.error("Unable to load case and flap inspection", error);
          if (!cancelled) setFailed(true);
        });
    }
    return () => {
      cancelled = true;
      motion.current?.dispose();
      motion.current = null;
    };
  }, []);
  return (
    <main className={s.page}>
      <a href="?screen=collection">← Collection preview</a>
      <h1>Case &amp; flap</h1>
      <p>Two separate pieces. Two folds. Drag to inspect.</p>
      <div className={s.stage} ref={host} />
      <div className={s.actions}>
        <Button
          disabled={!ready}
          selected={separated}
          onClick={() => {
            setSeparated(!separated);
            motion.current?.setSeparated(!separated);
          }}
        >
          {separated ? "Fit together" : "Separate pieces"}
        </Button>
        <Button
          variant="ghost"
          disabled={!ready}
          selected={guides}
          onClick={() => {
            setGuides(!guides);
            motion.current?.setGuides(!guides);
          }}
        >
          {guides ? "Hide joints" : "Show joints"}
        </Button>
        <Button
          variant="ghost"
          disabled={!ready}
          onClick={() => {
            setRear(0);
            setFront(0);
            motion.current?.setFold("rear", 0);
            motion.current?.setFold("front", 0);
            motion.current?.reset();
          }}
        >
          Reset folds &amp; view
        </Button>
        <a
          href={`${import.meta.env.BASE_URL}models/deck-box-parts.glb`}
          download
        >
          Download rigged parts
        </a>
      </div>
      <fieldset className={s.folds} disabled={!ready}>
        <legend>Inspect the fold joints</legend>
        <div className={s.fold}>
          <label className={s.rear}>
            A · Rear attachment <output>{rear}°</output>
          </label>
          <Slider
            label="Rear attachment angle"
            value={rear}
            onValueChange={(value) => {
              setRear(value);
              motion.current?.setFold("rear", value);
            }}
          />
          <span>Rear cover panel → top cover</span>
        </div>
        <div className={s.fold}>
          <label className={s.front}>
            B · Front fold <output>{front}°</output>
          </label>
          <Slider
            label="Front fold angle"
            value={front}
            onValueChange={(value) => {
              setFront(value);
              motion.current?.setFold("front", value);
            }}
          />
          <span>Top cover → stitched front flap</span>
        </div>
      </fieldset>
      <p className={s.note} role="status">
        {failed
          ? "Unable to load the model. Reload to retry."
          : ready
            ? separated
              ? "Joint placement study. Left: case. Right: complete wraparound cover."
              : "Joint placement study. Fitted together; adjust each fold independently."
            : "Loading model…"}
      </p>
    </main>
  );
}
