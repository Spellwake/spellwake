import type { MatchActionCommand } from "./match-sources";
import { targetKey, type CombatTarget } from "./combat-draft";

/**
 * Casting a basic spell from hand, as approved (BC-01 to BC-09): an offered
 * card is started, a spell that needs a target waits for one, and a cost the
 * game pauses for waits for payment. Everything that decides what may be done
 * comes from the game's offers; nothing here reads a card's text or counts
 * mana. Pure, so every step can be tested.
 */

/**
 * One card the game offers to cast. The player view does not carry these yet,
 * so only the preview supplies them: a match with no offers shows no playable
 * cards and starts no cast here.
 */
export interface CastOffer {
  /** The card that may be cast. */
  source: string;
  /** What it may target, or null for a spell with no target. */
  targets: readonly CombatTarget[] | null;
  /** Its cost as printed, such as `{1}{R}`, for the payment prompt. */
  cost: string;
  /**
   * Whether the game completes an ordinary payment itself, or pauses for the
   * player to pay.
   */
  payment: "automatic" | "paused";
  /** While paused: the mana sources an automatic payment would use. */
  paySources: readonly string[];
}

export interface CastDraft {
  source: string;
  /** Waiting for a target, or for payment. */
  stage: "target" | "payment";
  target: CombatTarget | null;
}

export type CastStep =
  /** Nothing was offered: the card is not started. */
  | { kind: "ignored" }
  /** The draft now waits for the player. */
  | { kind: "draft"; draft: CastDraft }
  /** Every choice is made: this is sent to the game. */
  | { kind: "submit"; draft: CastDraft; action: MatchActionCommand };

export const offerFor = (
  offers: readonly CastOffer[],
  source: string,
): CastOffer | undefined => offers.find((offer) => offer.source === source);

/** What is sent for a finished draft. A null payment leaves paying to the game. */
export function castAction(draft: CastDraft): MatchActionCommand {
  return draft.target
    ? {
        kind: "cast-targeted",
        source: draft.source,
        target: draft.target,
        payment: null,
      }
    : { kind: "cast-untargeted", source: draft.source, payment: null };
}

const settle = (offer: CastOffer, draft: CastDraft): CastStep =>
  offer.payment === "paused"
    ? { kind: "draft", draft: { ...draft, stage: "payment" } }
    : { kind: "submit", draft, action: castAction(draft) };

/** Start casting an offered card. A spell with no target and no pause is sent at once. */
export function beginCast(
  offers: readonly CastOffer[],
  source: string,
): CastStep {
  const offer = offerFor(offers, source);
  if (!offer) return { kind: "ignored" };
  const draft: CastDraft = { source, stage: "target", target: null };
  if (offer.targets !== null) {
    // A spell that needs a target and is offered none cannot be started.
    return offer.targets.length > 0
      ? { kind: "draft", draft }
      : { kind: "ignored" };
  }
  return settle(offer, draft);
}

/** The targets the waiting spell may be given. */
export function castTargets(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): readonly CombatTarget[] {
  if (!draft || draft.stage !== "target") return [];
  return offerFor(offers, draft.source)?.targets ?? [];
}

/** Choose the waiting spell's target. The last choice sends it unless the game pauses for payment. */
export function chooseCastTarget(
  offers: readonly CastOffer[],
  draft: CastDraft,
  target: CombatTarget,
): CastStep {
  const offer = offerFor(offers, draft.source);
  if (!offer || draft.stage !== "target") return { kind: "ignored" };
  const offered = (offer.targets ?? []).some(
    (entry) => targetKey(entry) === targetKey(target),
  );
  if (!offered) return { kind: "ignored" };
  return settle(offer, { ...draft, target });
}

/** Pay for a paused draft. Only a draft waiting for payment can be paid. */
export function payCast(
  offers: readonly CastOffer[],
  draft: CastDraft,
): CastStep {
  return draft.stage === "payment" && offerFor(offers, draft.source)
    ? { kind: "submit", draft, action: castAction(draft) }
    : { kind: "ignored" };
}

/**
 * After the game changes: a draft survives only while its card is still
 * offered with the target already chosen. Nothing is chosen on its behalf.
 */
export function reconcileCast(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): CastDraft | null {
  if (!draft) return null;
  const offer = offerFor(offers, draft.source);
  if (!offer) return null;
  if (!draft.target) return draft;
  const chosen = targetKey(draft.target);
  return (offer.targets ?? []).some((entry) => targetKey(entry) === chosen)
    ? draft
    : null;
}

/** The symbols of a printed cost, in order: `{1}{R}` gives `1` and `R`. */
export const costSymbols = (cost: string): string[] =>
  Array.from(cost.matchAll(/\{([^}]+)\}/g), (match) => match[1] ?? "");
