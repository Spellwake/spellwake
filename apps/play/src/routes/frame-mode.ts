import { useMatches } from "react-router-dom";

/**
 * How much of the application frame a screen keeps.
 * - browse: header, sections and destinations. The default.
 * - focus: one full-screen task with its own task header, such as the deck builder.
 * - immersive: the screen owns everything and floats its own controls, such as a match.
 */
export type FrameMode = "browse" | "focus" | "immersive";
export type FrameHandle = { frame: FrameMode };

const isFrameHandle = (handle: unknown): handle is FrameHandle =>
  typeof handle === "object" &&
  handle !== null &&
  "frame" in handle &&
  (handle.frame === "browse" ||
    handle.frame === "focus" ||
    handle.frame === "immersive");

/** The frame mode declared by the deepest matched route. */
export function useFrameMode(): FrameMode {
  const matches = useMatches();
  for (let index = matches.length - 1; index >= 0; index -= 1) {
    const handle = matches[index]?.handle;
    if (isFrameHandle(handle)) return handle.frame;
  }
  return "browse";
}
