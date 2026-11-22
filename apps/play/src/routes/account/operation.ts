import { useCallback, useEffect, useRef, useState } from "react";
import type { Outcome } from "../../decks/deck-sources";

const NOTICE_MS = 4000;
/**
 * One line of passive feedback. A working line stays until it is replaced. A
 * result leaves by itself, and never carries an action.
 */
export function useNotice(): {
  items: { id: string; text: string }[];
  say: (text: string, kind?: "working" | "result") => void;
  clear: () => void;
} {
  const [notice, setNotice] = useState<{
    id: number;
    text: string;
    isWorking: boolean;
  } | null>(null);
  const count = useRef(0);
  useEffect(() => {
    if (!notice || notice.isWorking) return;
    const timer = window.setTimeout(() => setNotice(null), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);
  const say = useCallback((text: string, kind = "result") => {
    count.current += 1;
    setNotice({ id: count.current, text, isWorking: kind === "working" });
  }, []);
  const clear = useCallback(() => setNotice(null), []);
  return {
    items: notice ? [{ id: String(notice.id), text: notice.text }] : [],
    say,
    clear,
  };
}

/**
 * Runs one command at a time. A second press while the first is in flight is
 * ignored, which a disabled button alone cannot promise.
 */
export function useCommand(): {
  isBusy: boolean;
  run: (work: () => Promise<Outcome>) => Promise<Outcome | null>;
} {
  const inFlight = useRef(false);
  const [isBusy, setIsBusy] = useState(false);
  const run = useCallback(async (work: () => Promise<Outcome>) => {
    if (inFlight.current) return null;
    inFlight.current = true;
    setIsBusy(true);
    try {
      return await work();
    } finally {
      inFlight.current = false;
      setIsBusy(false);
    }
  }, []);
  return { isBusy, run };
}
