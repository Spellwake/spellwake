import type { z } from "zod";
import {
  enqueueMatchmakingSchema,
  queueEntrySchema,
  type profileConfigurationSchema,
  type tableProfileSchema,
} from "@spellwake/application-contracts";
import type { PlayMode } from "../decks/deck-sources";

export type MatchLength = "single" | "three";
export const LENGTH_LABEL: Record<MatchLength, string> = {
  single: "Single game",
  three: "Best of three",
};

type TableProfile = z.infer<typeof tableProfileSchema>;
type ProfileConfiguration = z.infer<typeof profileConfigurationSchema>;
export type QueueEntry = z.infer<typeof queueEntrySchema>;
/** One table a deck may be queued for. */
export type QueueChoice = { profileId: string; configurationKey: string };

/**
 * The tables a format can be queued for, by match length: every configuration
 * of that format on a profile that takes part in matchmaking.
 */
export function queueChoices(
  profiles: readonly TableProfile[],
  configurations: ReadonlyMap<string, readonly ProfileConfiguration[]>,
  formatId: string,
): Record<MatchLength, QueueChoice[]> {
  const choices: Record<MatchLength, QueueChoice[]> = { single: [], three: [] };
  for (const profile of profiles) {
    if (!profile.hasMatchmaking) continue;
    for (const configuration of configurations.get(profile.id) ?? []) {
      if (configuration.formatId !== formatId) continue;
      choices[configuration.structure === "bo3" ? "three" : "single"].push({
        profileId: profile.id,
        configurationKey: configuration.key,
      });
    }
  }
  return choices;
}

/**
 * The request that queues one deck for the given tables against players
 * only. Null where the contract would refuse it, so nothing invalid is sent.
 */
export function enqueueRequest(input: {
  requestId: string;
  mode: PlayMode;
  ranked: boolean;
  deckRevisionId: string;
  choices: readonly QueueChoice[];
}): z.infer<typeof enqueueMatchmakingSchema> | null {
  const request = enqueueMatchmakingSchema.safeParse({
    requestId: input.requestId,
    mode: input.mode,
    // Only a Collection deck can be played for rank.
    competitive: input.ranked && input.mode === "collection",
    alternatives: [
      {
        deckRevisionId: input.deckRevisionId,
        choices: input.choices.map((choice) => ({
          ...choice,
          maximumAiSeats: "0",
          maximumSponsoredAiSeats: "0",
        })),
      },
    ],
  });
  return request.success ? request.data : null;
}

/** The queue's entries that read as the contract says. */
export function queueEntries(items: readonly unknown[]): QueueEntry[] {
  return items.flatMap((item) => {
    const entry = queueEntrySchema.safeParse(item);
    return entry.success ? [entry.data] : [];
  });
}

/**
 * The search this account has running, if any: the entry of the given
 * request where one is named, else any entry still waiting or just matched.
 */
export function activeEntry(
  entries: readonly QueueEntry[],
  intentId: string | null,
): QueueEntry | null {
  const live = entries.filter((entry) => entry.state !== "canceled");
  return (
    (intentId
      ? live.find((entry) => entry.intentId === intentId)
      : live.find(
          (entry) => entry.state === "queued" || entry.state === "claimed",
        )) ?? null
  );
}

/** Why a search could not start, in the dialog's words. */
export function enqueueFailure(errors: Record<string, unknown>): string {
  if ("deck" in errors) return "Save this deck before finding a match.";
  if ("choice" in errors || "choices" in errors)
    return "This match isn’t available for this format.";
  return "Couldn’t start the search.";
}

/** "Standard · Best of three · Ranked". */
export function searchTerms(
  format: string,
  length: MatchLength,
  ranked: boolean,
): string {
  return [format, LENGTH_LABEL[length], ...(ranked ? ["Ranked"] : [])].join(
    " · ",
  );
}

/** What the dialog shows of finding a match for one deck. */
export type FindMatchSource = {
  /** Which match lengths this deck's format can be queued for. Null while that is read. */
  lengths: Record<MatchLength, boolean> | null;
  phase: "idle" | "starting" | "searching" | "found";
  /** When the search began, for its timer. */
  since: number | null;
  /** What the running search was started with; null for one found already running. */
  terms: string | null;
  /** Set when the last step failed; cleared by the next. */
  failure: string | null;
  find: (length: MatchLength, ranked: boolean) => void;
  /** Stops the search. Resolves once it has stopped or cannot be. */
  cancel: () => Promise<void>;
};
