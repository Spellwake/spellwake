import { useCallback, useEffect, useRef, useState } from "react";
import {
  searchTerms,
  type FindMatchSource,
  type MatchLength,
} from "./find-match";

/** How the preview's search behaves, for review and the stress checks. */
export type DemoSearch =
  /** A match is found a few seconds in. */
  | "finds"
  /** The search keeps running. */
  | "waits"
  /** The search cannot start. */
  | "refused"
  /** The format has no best of three. */
  | "single-only"
  /** The format cannot be queued at all. */
  | "none";

const FOUND_AFTER_MS = 4000;
const TABLE_AFTER_MS = 1400;

/** The preview's stand-in for finding a match: no account and no server. */
export function useDemoFindMatch(
  deck: { format: string },
  behaviour: DemoSearch,
  onFound: () => void,
): FindMatchSource {
  const [phase, setPhase] = useState<FindMatchSource["phase"]>("idle");
  const [since, setSince] = useState<number | null>(null);
  const [terms, setTerms] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const found = useRef(onFound);
  useEffect(() => {
    found.current = onFound;
  }, [onFound]);
  useEffect(() => {
    if (phase === "searching" && behaviour === "finds") {
      const timer = window.setTimeout(() => setPhase("found"), FOUND_AFTER_MS);
      return () => window.clearTimeout(timer);
    }
    if (phase === "found") {
      const timer = window.setTimeout(() => found.current(), TABLE_AFTER_MS);
      return () => window.clearTimeout(timer);
    }
    return undefined;
  }, [phase, behaviour]);
  const find = useCallback(
    (length: MatchLength, ranked: boolean) => {
      if (behaviour === "refused") {
        setFailure("Couldn’t start the search.");
        return;
      }
      setFailure(null);
      setTerms(searchTerms(deck.format, length, ranked));
      setSince(Date.now());
      setPhase("searching");
    },
    [behaviour, deck.format],
  );
  const cancel = useCallback(async () => {
    setPhase("idle");
    setSince(null);
    return Promise.resolve();
  }, []);
  return {
    lengths:
      behaviour === "none"
        ? { single: false, three: false }
        : { single: true, three: behaviour !== "single-only" },
    phase,
    since,
    terms,
    failure,
    find,
    cancel,
  };
}
