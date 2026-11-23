import type { useStore } from "jotai";
import { z } from "zod";
import {
  identifierSchema,
  type CommandAck,
} from "@spellwake/application-contracts";
import type { ApiResponse } from "../data/application-api";
import { withAccountStorageLock } from "../data/account-storage";
import {
  createIntentJournal,
  type IntentDisposition,
  type IntentJournal,
  type IntentJournalAction,
} from "../data/intent-journal";
import { recoverIntent } from "../data/intent-recovery";

/** Total tries at settling one room action, the first included. */
const ATTEMPTS = 3;
const pause = (ms: number): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

/** The room actions this device journals, each in its own slot. */
export const ROOM_ACTION_KINDS = [
  "select_deck",
  "leave",
  "ready",
  "unready",
  "force_start",
  "add_ai",
  "bootstrap_ai",
  "remove_ai",
  "set_ai_tier",
  "configure",
] as const;
export type RoomActionKind = (typeof ROOM_ACTION_KINDS)[number] | "create";

/** The storage key of one action's journal: `create`, or a kind and its room or seat. */
export const roomJournalKey = (
  origin: string,
  accountId: string,
  kind: RoomActionKind,
  target: string | null,
): string =>
  `spellwake.room-command.v1:${origin}:${accountId}:${kind}${target ? `:${target}` : ""}`;

export type RoomOutcome<A> =
  | { state: "accepted"; ack: A }
  | { state: "rejected"; errors: Record<string, string> }
  /** No answer could be had: the action may or may not have happened. */
  | { state: "unknown" };

type Store = ReturnType<typeof useStore>;

/**
 * One journaled room action. An earlier attempt in the same slot that never
 * got its answer is asked about first, and nothing new is sent while it is
 * unknown. Otherwise the new action is sent once, and an unknown result is
 * asked about again up to three tries in all. `recoverOnly` asks about the
 * earlier attempt without sending anything new, as Check status does.
 */
export async function runRoomAction<Command, Ack>(options: {
  store: Store;
  origin: string;
  accountId: string;
  kind: RoomActionKind;
  target: string | null;
  commandSchema: z.ZodType<Command>;
  /** The command's request ID, which names it to the server for good. */
  requestIdOf: (command: Command) => string;
  ackSchema: z.ZodType<Ack>;
  errorsSchema: z.ZodType<Record<string, string>>;
  dispositionSchema: z.ZodType<IntentDisposition<Record<string, string>>>;
  /** Builds the new command with a fresh request ID. Null with `recoverOnly`. */
  build: (() => Command) | null;
  send: (command: Command) => Promise<ApiResponse<CommandAck>>;
  lookup: (
    command: Command,
  ) => Promise<ApiResponse<IntentDisposition<Record<string, string>>>>;
  refresh: () => Promise<void>;
}): Promise<RoomOutcome<Ack>> {
  const key = roomJournalKey(
    options.origin,
    options.accountId,
    options.kind,
    options.target,
  );
  const source = {
    version: z.literal(1),
    apiOrigin: z.literal(options.origin),
    accountId: z.literal(options.accountId),
    command: options.commandSchema,
  };
  const journalSchema = z.discriminatedUnion("state", [
    z.strictObject({ ...source, state: z.literal("unresolved") }),
    z.strictObject({
      ...source,
      state: z.literal("accepted"),
      ack: options.ackSchema,
    }),
    z.strictObject({
      ...source,
      state: z.literal("rejected"),
      errors: options.errorsSchema,
    }),
  ]) as unknown as z.ZodType<IntentJournal<Command, Record<string, string>>>;
  const cell = createIntentJournal<Command, Record<string, string>>(
    key,
    options.origin,
    options.accountId,
    journalSchema,
    options.commandSchema,
    options.dispositionSchema,
    options.requestIdOf,
    () => window.localStorage,
  );
  const act = (action: IntentJournalAction<Command, Record<string, string>>) =>
    withAccountStorageLock(options.origin, options.accountId, () =>
      options.store.set(cell.actionAtom, action),
    );
  const run = (
    input:
      | {
          type: "recover";
          expected: IntentJournal<Command, Record<string, string>>;
        }
      | {
          type: "submit";
          expected: IntentJournal<Command, Record<string, string>> | null;
          draft: null;
        },
  ) =>
    recoverIntent<null, Command, Record<string, string>>(input, {
      key,
      act,
      build: () => {
        if (!options.build) throw new Error("Nothing new to send.");
        return options.build();
      },
      send: options.send,
      lookup: options.lookup,
      refreshCurrent: options.refresh,
    });
  const outcome = (
    journal: IntentJournal<Command, Record<string, string>>,
  ): RoomOutcome<Ack> =>
    journal.state === "accepted"
      ? { state: "accepted", ack: journal.ack as unknown as Ack }
      : journal.state === "rejected"
        ? { state: "rejected", errors: journal.errors }
        : { state: "unknown" };

  try {
    const saved = options.store.get(cell.stateAtom);
    if (saved.status === "unavailable") return { state: "unknown" };
    let prior = saved.status === "saved" ? saved.value : null;
    if (prior?.state === "unresolved") {
      try {
        prior = await run({ type: "recover", expected: prior });
      } catch {
        return { state: "unknown" };
      }
      if (prior.state === "unresolved" || !options.build) return outcome(prior);
    } else if (!options.build)
      return prior ? outcome(prior) : { state: "unknown" };
    let journal: IntentJournal<Command, Record<string, string>>;
    try {
      journal = await run({ type: "submit", expected: prior, draft: null });
    } catch {
      const latest = options.store.get(cell.stateAtom);
      if (latest.status !== "saved" || latest.value.state !== "unresolved")
        return { state: "unknown" };
      journal = latest.value;
    }
    for (
      let attempt = 1;
      journal.state === "unresolved" && attempt < ATTEMPTS;
      attempt += 1
    ) {
      await pause(700);
      try {
        journal = await run({ type: "recover", expected: journal });
      } catch {
        // Still unknown: try again, or give up below.
      }
    }
    return outcome(journal);
  } catch {
    return { state: "unknown" };
  }
}

/** A journal slot's target, for storage cleanup: a room or seat identifier. */
export const isRoomTarget = (target: string): boolean =>
  identifierSchema.safeParse(target).success;
