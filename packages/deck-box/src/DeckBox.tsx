import { useEffect, useRef, useState, type ReactElement } from "react";
import type { BoxAppearance } from "./box-appearance";
import type { BoxMotion } from "./box-motion";
import type { BoxView } from "./box-view";
import {
  createViewportResource,
  type ViewportResource,
} from "./viewport-resource";

export type DeckBoxProps = {
  name: string;
  appearance: BoxAppearance;
  modelUrl: string;
  onInspect: () => void;
  view?: BoxView | undefined;
  className?: string;
  stageClassName?: string;
  statusClassName?: string;
};

/** Accessible collection trigger around the approved, viewport-bounded 3D renderer. */
export function DeckBox({
  name,
  appearance,
  modelUrl,
  onInspect,
  view,
  className,
  stageClassName,
  statusClassName,
}: DeckBoxProps): ReactElement {
  const host = useRef<HTMLSpanElement>(null);
  const lifetime = useRef<ViewportResource<BoxMotion> | null>(null);
  const isOpen = useRef(false);
  const appearanceRef = useRef(appearance);
  appearanceRef.current = appearance;
  const [status, setStatus] = useState<"loading" | "ready" | "error">(
    "loading",
  );
  const azimuth = view?.azimuth;
  const elevation = view?.elevation;

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const resource = createViewportResource<BoxMotion>({
      load: async (signal) => {
        const { mountBoxMotion } = await import("./box-motion");
        signal.throwIfAborted();
        return mountBoxMotion(element, modelUrl, {
          collection: true,
          appearance: appearanceRef.current,
          view:
            azimuth !== undefined && elevation !== undefined
              ? { azimuth, elevation }
              : undefined,
          signal,
        });
      },
      onLoading: () => setStatus("loading"),
      onReady: (handle) => {
        handle.setAppearance(appearanceRef.current);
        handle.setOpen(isOpen.current);
        setStatus("ready");
      },
      onError: (error) => {
        console.error("Unable to load collection box.", error);
        setStatus("error");
      },
    });
    lifetime.current = resource;
    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries.find((candidate) => candidate.target === element);
        if (entry) resource.setVisible(entry.isIntersecting);
      },
      { rootMargin: "120px" },
    );
    observer.observe(element);
    return () => {
      observer.disconnect();
      resource.dispose();
      lifetime.current = null;
    };
  }, [modelUrl, azimuth, elevation]);

  useEffect(() => {
    lifetime.current?.current()?.setAppearance(appearance);
  }, [appearance]);

  function open(value: boolean): void {
    isOpen.current = value;
    lifetime.current?.current()?.setOpen(value);
  }

  return (
    <button
      type="button"
      className={className}
      aria-label={`Inspect ${name}`}
      onPointerEnter={(event) => {
        if (event.pointerType !== "touch") open(true);
      }}
      onPointerLeave={() => open(false)}
      onFocus={() => open(true)}
      onBlur={() => open(false)}
      onClick={onInspect}
    >
      <span className={stageClassName} ref={host} aria-hidden="true" />
      {status !== "ready" && (
        <span className={statusClassName} role="status">
          {status === "error"
            ? "Preview unavailable · Open deck"
            : "Loading deck box…"}
        </span>
      )}
    </button>
  );
}
