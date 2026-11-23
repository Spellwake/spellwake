import { atomWithStorage } from "jotai/utils";
import type { MatchChatEntry } from "./match-sources";

/**
 * Table chat and match settings, as approved (CM-01 to CM-11). The rules that
 * can be tested live here: what a setting may hold, when a sent draft may be
 * cleared, and when the chat button shows its unread dot.
 */

/** A player's own match settings, kept on this device. Nothing here is sent to the game. */
export interface MatchPreferences {
  /** Each 0 to 100. Master scales the other two. */
  master: number;
  music: number;
  effects: number;
  /** A personal preference for paying costs. It never casts or taps by being changed. */
  autoTap: boolean;
}
export const DEFAULT_PREFERENCES: MatchPreferences = {
  master: 80,
  music: 50,
  effects: 80,
  autoTap: true,
};

const level = (value: unknown, fallback: number): number =>
  typeof value === "number" && Number.isFinite(value)
    ? Math.min(100, Math.max(0, Math.round(value)))
    : fallback;
/** Whatever was stored, as settings that can be shown: unknown or damaged values fall back. */
export function readPreferences(stored: unknown): MatchPreferences {
  const record =
    typeof stored === "object" && stored !== null
      ? (stored as Record<string, unknown>)
      : {};
  return {
    master: level(record.master, DEFAULT_PREFERENCES.master),
    music: level(record.music, DEFAULT_PREFERENCES.music),
    effects: level(record.effects, DEFAULT_PREFERENCES.effects),
    autoTap:
      typeof record.autoTap === "boolean"
        ? record.autoTap
        : DEFAULT_PREFERENCES.autoTap,
  };
}
/** Stored as it is changed: there is no Save. */
export const matchPreferences = atomWithStorage<unknown>(
  "spellwake:match:preferences:v1",
  DEFAULT_PREFERENCES,
  undefined,
  { getOnInit: true },
);

/** A table message may be this long, counted in characters as a reader sees them. */
export const CHAT_LIMIT = 4000;

/**
 * The draft after a send settles. Only the exact text that was sent is
 * cleared: words typed since, or a message that was refused, stay.
 */
export function settleDraft(
  current: string,
  sent: string,
  accepted: boolean,
): string {
  return accepted && current === sent ? "" : current;
}

/**
 * Whether the chat button shows its dot: someone else has written since the
 * last message this player actually saw.
 */
export function hasUnseen(
  messages: readonly MatchChatEntry[],
  lastSeenId: string | null,
): boolean {
  const from =
    lastSeenId === null
      ? -1
      : messages.findIndex((message) => message.id === lastSeenId);
  // A seen message that is no longer in the history proves nothing newer.
  if (lastSeenId !== null && from < 0) return false;
  return messages.slice(from + 1).some((message) => !message.isMine);
}
