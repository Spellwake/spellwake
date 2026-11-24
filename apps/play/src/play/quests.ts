import type { z } from "zod";
import {
  dailyWinPageSchema,
  type QuestPage,
} from "@spellwake/application-contracts";
import type { DailyQuest } from "./quest-sources";

type QuestAssignment = QuestPage["items"][number];
type QuestObjective = NonNullable<QuestAssignment["objective"]>;
type DailyWins = z.infer<typeof dailyWinPageSchema>;

/**
 * What a quest asks, as a short sentence. A quest names what it counts by a
 * key, not in words, so only the kinds the game is known to count are worded;
 * any other reads as a plain quest with its count.
 */
export function questText(objective: QuestObjective): string {
  const named = `${objective.definitionKey} ${objective.objectiveRef}`;
  const count = objective.target;
  if (named.includes("cast-red-or-green"))
    return `Cast ${count} red or green spells`;
  if (named.includes("cast-black-or-green"))
    return `Cast ${count} black or green spells`;
  return "Daily quest";
}

/**
 * The quests to show today: those still to do, and those finished today.
 * A quest that was swapped away is gone, and one whose objective cannot be
 * read is left out rather than shown without a target.
 */
export function shownQuests(page: QuestPage): DailyQuest[] {
  return page.items.flatMap((item) => {
    const objective = item.objective;
    if (!objective || item.replacedById !== null) return [];
    const done = item.completedAt !== null;
    if (done && item.completedAt!.slice(0, 10) !== page.utcDay) return [];
    return [
      {
        id: item.id,
        text: questText(objective),
        progress: Number(item.progress),
        target: Number(objective.target),
        rewardCoins: Number(objective.rewardCoins),
        done,
      },
    ];
  });
}

/**
 * The quest today's one swap may be spent on: the one given today, while it
 * is unfinished, still in place and the swap is unused. Null otherwise.
 */
export function swappableQuest(page: QuestPage): string | null {
  const today = page.daily?.generatedAssignmentId ?? null;
  if (!today || page.daily?.rerollsUsed !== "0") return null;
  const quest = page.items.find((item) => item.id === today);
  return quest && quest.completedAt === null && quest.replacedById === null
    ? quest.id
    : null;
}

/** Games won today and the coins they have paid. Nothing won yet reads as none. */
export function winsToday(
  page: DailyWins,
  utcDay: string,
): { count: number; coins: number } {
  const today = page.items.find((item) => item.utcDay === utcDay);
  return {
    count: Number(today?.wins ?? 0),
    coins: Number(today?.issuedCoins ?? 0),
  };
}

/** Why a swap did not happen, in the row's words. */
export function swapFailure(errors: Record<string, unknown>): string {
  if ("reroll" in errors) return "Today’s swap is already used.";
  if ("definition" in errors) return "No other quest is available today.";
  if ("day" in errors || "assignment" in errors)
    return "This quest can no longer be swapped.";
  return "Couldn’t swap the quest.";
}
