import {
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type CSSProperties,
} from "react";
import {
  arrow,
  autoUpdate,
  flip,
  offset,
  shift,
  useFloating,
  type Placement,
} from "@floating-ui/react-dom";
import { motion } from "motion/react";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { controlMotion } from "../control-motion";
import { OverlayFrame, pointer } from "./OverlayFrame";
import * as s from "./overlays.css";
export interface TooltipProps extends Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "disabled" | "title" | "content"
> {
  text: string;
  placement?: Placement;
  alignOffset?: number;
  /** Additional space to clear a surrounding panel. */
  clearance?: number;
  bubbleClassName?: string;
}
/** Supplemental text for a focusable button. Use aria-disabled to expose an unavailable action's explanation. */
export function Tooltip({
  text,
  placement = "top",
  alignOffset = 0,
  clearance = 0,
  bubbleClassName = "",
  className = "",
  children,
  onClick,
  "aria-describedby": description,
  ...props
}: TooltipProps) {
  const id = useId(),
    arrowRef = useRef<SVGSVGElement>(null),
    [open, setOpen] = useState(false);
  const animate = useAllowsControlMotion();
  const {
    refs,
    floatingStyles,
    middlewareData,
    placement: actual,
    update,
  } = useFloating({
    open,
    placement,
    strategy: "fixed",
    middleware: [
      offset({
        mainAxis: pointer.gap + pointer.depth + clearance,
        crossAxis: alignOffset,
      }),
      flip({ padding: 8 }),
      shift({ padding: 8 }),
      arrow({ element: arrowRef, padding: pointer.corner }),
    ],
  });
  const side = actual.split("-")[0]!;
  useEffect(() => {
    void (async () => {
      if (!("popover" in HTMLElement.prototype))
        await import("@oddbird/popover-polyfill");
      if (!("interestForElement" in HTMLButtonElement.prototype))
        await import("interestfor");
    })();
  }, []);
  useEffect(() => {
    const a = refs.reference.current,
      b = refs.floating.current;
    return open && a && b ? autoUpdate(a, b, update) : undefined;
  }, [open, refs, update]);
  const horizontal = side === "left" || side === "right";
  const arrowStyle: CSSProperties = {
    position: "absolute",
    pointerEvents: "none",
    left: middlewareData.arrow?.x,
    top: middlewareData.arrow?.y,
    ...{
      top: { bottom: -pointer.depth },
      bottom: { top: -pointer.depth },
      left: { right: -pointer.depth },
      right: { left: -pointer.depth },
    }[side],
  };
  return (
    <>
      <button
        {...props}
        {...{ interestfor: id }}
        ref={refs.setReference}
        type={props.type ?? "button"}
        className={[s.tipTrigger, className].join(" ")}
        aria-describedby={[description, id].filter(Boolean).join(" ")}
        onClick={(event) => {
          if (
            props["aria-disabled"] === true ||
            props["aria-disabled"] === "true"
          ) {
            event.preventDefault();
            return;
          }
          onClick?.(event);
        }}
      >
        {children}
      </button>
      <div
        ref={refs.setFloating}
        id={id}
        role="tooltip"
        popover="hint"
        className={[s.tip, bubbleClassName].join(" ")}
        style={floatingStyles}
        onToggle={(event) => setOpen(event.newState === "open")}
      >
        <motion.div
          data-side={side}
          className={s.tipBody}
          initial={false}
          animate={{
            opacity: open ? 1 : 0,
            y: animate && !open ? (side === "top" ? 2 : -2) : 0,
          }}
          transition={{
            duration: animate ? controlMotion.feedback : 0,
            ease: controlMotion.ease,
          }}
        >
          {text}
          <svg
            aria-hidden="true"
            ref={arrowRef}
            style={arrowStyle}
            width={horizontal ? pointer.depth : pointer.base}
            height={horizontal ? pointer.base : pointer.depth}
          />
          <OverlayFrame marker={arrowRef} />
        </motion.div>
      </div>
    </>
  );
}
