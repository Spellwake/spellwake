import {
  playerTargetKey,
  type PlayerTarget,
} from "@spellwake/application-contracts";
import type { MatchActionCommand } from "./match-sources";

/**
 * Casting a spell from hand, as approved (BC-01 to BC-09, MC-01 to MC-06,
 * MM-00 to MM-07, MT-00 to MT-06, the mixed spell and permanent sequence, and
 * DC-00 to DC-11): an offered card is started, a modal spell waits for its
 * modes, each thing it targets is chosen in printed order, a divided amount
 * is shared out, and a cost the game pauses for waits for payment.
 * Everything that decides what may be done comes from the game's offers;
 * nothing here reads a card's text or counts mana. Pure, so every step can
 * be tested.
 */

/** A card, a player, or a spell on the stack. */
export type CastTarget = PlayerTarget;
export const castTargetKey = playerTargetKey;

/** One printed mode of a modal spell, as the game offers it. */
export interface CastModeOffer {
  /** The game's zero-based number for this mode, in printed order. */
  mode: number;
  /** The mode's own words, which name its outlined paragraph. */
  label: string;
  /** What this mode may target, or null for a mode with no target. */
  targets: readonly CastTarget[] | null;
  /** What its target is called when asked for, such as Choose a permanent. */
  ask?: string;
  /** Why the mode cannot be chosen now, such as No legal targets. */
  unavailableReason?: string;
}

/** One printed instruction that takes a target, of a spell with several. */
export interface CastSlotOffer {
  /** What is asked for, such as Choose a creature. */
  ask: string;
  /** What this instruction may target. A later one may repeat an earlier choice. */
  targets: readonly CastTarget[];
}

/** An amount shared out among the spell's targets as it is cast. */
export interface CastDivision {
  /** What is asked for, such as Divide 3 damage. */
  ask: string;
  total: number;
  /** The least each chosen target must receive. */
  minimum: number;
  /** The most targets that may be chosen. */
  most: number;
  /** What may be chosen, in the game's own order. */
  targets: readonly CastTarget[];
}

/**
 * One card the game offers to cast. The player view does not carry these yet,
 * so only the preview supplies them: a match with no offers shows no playable
 * cards and starts no cast here.
 */
export interface CastOffer {
  /** The card that may be cast. */
  source: string;
  /** What it may target, or null for a spell with no target. */
  targets: readonly CastTarget[] | null;
  /**
   * A modal spell's modes. Each mode carries its own targets; `targets` above
   * is then unused.
   */
  modes?: readonly CastModeOffer[];
  /** How many different modes must be chosen. One when absent. */
  choose?: number;
  /** Several printed instructions, each given its own target in order. */
  slots?: readonly CastSlotOffer[];
  /** An amount divided among one or more chosen targets. */
  divide?: CastDivision;
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

/** One target's part of a divided amount. */
export interface CastShare {
  target: CastTarget;
  amount: number;
}

export interface CastDraft {
  source: string;
  /** Waiting for modes, for a target, for a division, or for payment. */
  stage: "mode" | "target" | "divide" | "payment";
  /** The chosen modes, in printed order. */
  modes: readonly number[];
  /** The target being chosen now, counted from zero. */
  step: number;
  /**
   * One target for each step, in order. A step gone back to keeps its old
   * choice until another replaces it.
   */
  targets: readonly (CastTarget | null)[];
  /** A divided spell's chosen targets and what each receives. */
  shares: readonly CastShare[];
}

/** One target the spell asks for, in the order it is asked. */
export interface CastTargetStep {
  ask: string;
  targets: readonly CastTarget[];
  /** The mode, or the printed instruction, that this target belongs to. */
  of: { kind: "mode"; mode: number } | { kind: "slot"; slot: number } | null;
}

export type CastStep =
  /** Nothing was offered: the card is not started. */
  | { kind: "ignored" }
  /** The draft now waits for the player. */
  | { kind: "draft"; draft: CastDraft }
  /** Every choice is made: this is sent to the game. */
  | {
      kind: "submit";
      draft: CastDraft;
      action: MatchActionCommand;
      /**
       * A divided spell's amounts. The cast names only its targets; the game
       * then asks how the amount is shared, and this answers it.
       */
      division?: readonly { target: CastTarget; amount: string }[];
    };

const IGNORED: CastStep = { kind: "ignored" };
const ASK = "Choose a target";

export const offerFor = (
  offers: readonly CastOffer[],
  source: string,
): CastOffer | undefined => offers.find((offer) => offer.source === source);

const modeOf = (
  offer: CastOffer,
  mode: number | undefined,
): CastModeOffer | undefined =>
  offer.modes?.find((entry) => entry.mode === mode);

/** How many modes the spell asks for. */
export const modeCount = (offer: CastOffer): number => offer.choose ?? 1;

/** The targets a draft will be asked for, in printed order. */
export function castSteps(
  offer: CastOffer,
  modes: readonly number[],
): CastTargetStep[] {
  if (offer.divide) return [];
  if (offer.modes)
    return offer.modes.flatMap((entry) =>
      modes.includes(entry.mode) && entry.targets !== null
        ? [
            {
              ask: entry.ask ?? ASK,
              targets: entry.targets,
              of: { kind: "mode" as const, mode: entry.mode },
            },
          ]
        : [],
    );
  if (offer.slots)
    return offer.slots.map((slot, index) => ({
      ask: slot.ask,
      targets: slot.targets,
      of: { kind: "slot" as const, slot: index },
    }));
  return offer.targets === null
    ? []
    : [{ ask: ASK, targets: offer.targets, of: null }];
}

const offered = (targets: readonly CastTarget[], target: CastTarget) =>
  targets.some((entry) => castTargetKey(entry) === castTargetKey(target));

/** What is sent for a finished draft. A null payment leaves paying to the game. */
export function castAction(
  offer: CastOffer,
  draft: CastDraft,
): MatchActionCommand {
  const steps = castSteps(offer, draft.modes);
  const resources = {
    kind: "cast-with-resources" as const,
    source: draft.source,
    target: null,
    payment: null,
    resources: [],
  };
  if (offer.modes)
    return {
      ...resources,
      // Modes go in printed order, each with the target chosen for it.
      modes: offer.modes.flatMap((entry) => {
        if (!draft.modes.includes(entry.mode)) return [];
        const at = steps.findIndex(
          (step) => step.of?.kind === "mode" && step.of.mode === entry.mode,
        );
        return [{ mode: entry.mode, target: draft.targets[at] ?? null }];
      }),
    };
  if (offer.slots)
    return {
      ...resources,
      fixedTargets: draft.targets.flatMap((target) => (target ? [target] : [])),
    };
  if (offer.divide)
    return {
      ...resources,
      fixedTargets: inOfferedOrder(offer.divide, draft.shares).map(
        (share) => share.target,
      ),
    };
  const target = draft.targets[0];
  return target
    ? {
        kind: "cast-targeted",
        source: draft.source,
        target,
        payment: null,
      }
    : { kind: "cast-untargeted", source: draft.source, payment: null };
}

const submit = (offer: CastOffer, draft: CastDraft): CastStep => ({
  kind: "submit",
  draft,
  action: castAction(offer, draft),
  ...(offer.divide
    ? {
        division: inOfferedOrder(offer.divide, draft.shares).map((share) => ({
          target: share.target,
          amount: String(share.amount),
        })),
      }
    : {}),
});

const settle = (offer: CastOffer, draft: CastDraft): CastStep =>
  offer.payment === "paused"
    ? { kind: "draft", draft: { ...draft, stage: "payment" } }
    : submit(offer, draft);

/** Go on to the first target, or finish when nothing is targeted. */
function toTargets(offer: CastOffer, draft: CastDraft): CastStep {
  const steps = castSteps(offer, draft.modes);
  if (steps.length === 0) return settle(offer, draft);
  // A spell that needs a target and is offered none cannot go on.
  if (steps.some((step) => step.targets.length === 0)) return IGNORED;
  return {
    kind: "draft",
    draft: {
      ...draft,
      stage: "target",
      step: 0,
      targets: steps.map(() => null),
    },
  };
}

/** Start casting an offered card. A spell with no target and no pause is sent at once. */
export function beginCast(
  offers: readonly CastOffer[],
  source: string,
): CastStep {
  const offer = offerFor(offers, source);
  if (!offer) return IGNORED;
  const draft: CastDraft = {
    source,
    stage: "target",
    modes: [],
    step: 0,
    targets: [],
    shares: [],
  };
  if (offer.modes) {
    // A modal spell without enough modes that can be chosen cannot be started.
    const open = offer.modes.filter((mode) => !mode.unavailableReason).length;
    return open >= modeCount(offer)
      ? { kind: "draft", draft: { ...draft, stage: "mode" } }
      : IGNORED;
  }
  if (offer.divide)
    return offer.divide.targets.length > 0
      ? { kind: "draft", draft: { ...draft, stage: "divide" } }
      : IGNORED;
  return toTargets(offer, draft);
}

/**
 * Press one of a modal spell's modes. A spell that asks for one goes straight
 * on to that mode's targets, or to payment when it has none. A spell that
 * asks for several marks the mode instead: pressing a marked mode unmarks it,
 * and once enough are marked the others cannot be added until one is removed
 * (MM-01 to MM-05). Nothing is replaced on the player's behalf.
 */
export function chooseCastMode(
  offers: readonly CastOffer[],
  draft: CastDraft,
  mode: number,
): CastStep {
  const offer = offerFor(offers, draft.source);
  const chosen = offer ? modeOf(offer, mode) : undefined;
  if (!offer || !chosen || draft.stage !== "mode" || chosen.unavailableReason)
    return IGNORED;
  if (modeCount(offer) === 1)
    return toTargets(offer, { ...draft, modes: [mode] });
  if (draft.modes.includes(mode))
    return {
      kind: "draft",
      draft: { ...draft, modes: draft.modes.filter((entry) => entry !== mode) },
    };
  if (draft.modes.length >= modeCount(offer)) return IGNORED;
  const marked = new Set([...draft.modes, mode]);
  return {
    kind: "draft",
    draft: {
      ...draft,
      modes: offer.modes!.flatMap((entry) =>
        marked.has(entry.mode) ? [entry.mode] : [],
      ),
    },
  };
}

/** Whether the marked modes are exactly as many as the spell asks for. */
export const canConfirmModes = (
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): boolean => {
  const offer = draft ? offerFor(offers, draft.source) : undefined;
  return Boolean(
    draft &&
    offer?.modes &&
    draft.stage === "mode" &&
    draft.modes.length === modeCount(offer),
  );
};

/** Next, once enough modes are marked: on to their targets, or to payment. */
export function confirmCastModes(
  offers: readonly CastOffer[],
  draft: CastDraft,
): CastStep {
  const offer = offerFor(offers, draft.source);
  return offer && canConfirmModes(offers, draft)
    ? toTargets(offer, draft)
    : IGNORED;
}

/** What the waiting spell is asking for now. */
export function currentStep(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): CastTargetStep | null {
  if (!draft || draft.stage !== "target") return null;
  const offer = offerFor(offers, draft.source);
  return offer ? (castSteps(offer, draft.modes)[draft.step] ?? null) : null;
}

/** The targets the waiting spell may be given now. */
export const castTargets = (
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): readonly CastTarget[] => currentStep(offers, draft)?.targets ?? [];

/**
 * Choose the target being asked for. It goes on to the next one; the last
 * choice sends the cast unless the game pauses for payment. There is no
 * review of the targets in between.
 */
export function chooseCastTarget(
  offers: readonly CastOffer[],
  draft: CastDraft,
  target: CastTarget,
): CastStep {
  const offer = offerFor(offers, draft.source);
  const step = currentStep(offers, draft);
  if (!offer || !step || !offered(step.targets, target)) return IGNORED;
  const targets = draft.targets.map((entry, index) =>
    index === draft.step ? target : entry,
  );
  const next: CastDraft = { ...draft, targets };
  return draft.step + 1 < targets.length
    ? { kind: "draft", draft: { ...next, step: draft.step + 1 } }
    : settle(offer, next);
}

/** Whether an earlier target can be gone back to. The first cannot: Cancel leaves. */
export const canGoBack = (draft: CastDraft | null): boolean =>
  draft?.stage === "target" && draft.step > 0;

/**
 * Go back to the target before this one. Its old choice stays as it was
 * until another is chosen in its place, and nothing is paid (MT-03, MT-05).
 */
export function backCast(draft: CastDraft): CastStep {
  return canGoBack(draft)
    ? { kind: "draft", draft: { ...draft, step: draft.step - 1 } }
    : IGNORED;
}

const inOfferedOrder = (
  division: CastDivision,
  shares: readonly CastShare[],
): CastShare[] =>
  division.targets.flatMap((target) => {
    const share = shares.find(
      (entry) => castTargetKey(entry.target) === castTargetKey(target),
    );
    return share ? [share] : [];
  });

/** How much of the divided amount has been given out. */
export const sharedOut = (draft: CastDraft): number =>
  draft.shares.reduce((total, share) => total + share.amount, 0);

/**
 * Who gives up one when nothing is left to hand out: the last chosen target,
 * in the game's order, that can spare it. The same rule as combat damage.
 */
function donor(
  division: CastDivision,
  shares: readonly CastShare[],
  receiver: CastTarget,
): CastShare | undefined {
  return inOfferedOrder(division, shares)
    .reverse()
    .find(
      (share) =>
        castTargetKey(share.target) !== castTargetKey(receiver) &&
        share.amount > division.minimum,
    );
}

const divisionOf = (
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): CastDivision | null =>
  draft?.stage === "divide"
    ? (offerFor(offers, draft.source)?.divide ?? null)
    : null;

const shareOf = (draft: CastDraft, target: CastTarget) =>
  draft.shares.find(
    (share) => castTargetKey(share.target) === castTargetKey(target),
  );

/** Whether a target not yet chosen can be added. */
export function canAddShare(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
  target: CastTarget,
): boolean {
  const division = divisionOf(offers, draft);
  if (!division || !draft || !offered(division.targets, target)) return false;
  if (shareOf(draft, target) || draft.shares.length >= division.most)
    return false;
  return (
    sharedOut(draft) < division.total ||
    donor(division, draft.shares, target) !== undefined
  );
}

/**
 * Press a target of a divided spell (DC-01 to DC-08). A new target takes
 * everything not yet given out, or one from a target that can spare it, so
 * the other amounts stay as they were wherever they can. Pressing a chosen
 * target removes it and returns all it had.
 */
export function toggleShare(
  offers: readonly CastOffer[],
  draft: CastDraft,
  target: CastTarget,
): CastStep {
  const division = divisionOf(offers, draft);
  if (!division) return IGNORED;
  if (shareOf(draft, target))
    return {
      kind: "draft",
      draft: {
        ...draft,
        shares: draft.shares.filter(
          (share) => castTargetKey(share.target) !== castTargetKey(target),
        ),
      },
    };
  if (!canAddShare(offers, draft, target)) return IGNORED;
  const left = division.total - sharedOut(draft);
  if (left > 0)
    return {
      kind: "draft",
      draft: { ...draft, shares: [...draft.shares, { target, amount: left }] },
    };
  const from = donor(division, draft.shares, target)!;
  return {
    kind: "draft",
    draft: {
      ...draft,
      shares: [
        ...draft.shares.map((share) =>
          share === from ? { ...share, amount: share.amount - 1 } : share,
        ),
        { target, amount: 1 },
      ],
    },
  };
}

/** Whether a chosen target can give one back. At the least it may hold, it is removed instead. */
export function canShareMinus(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
  target: CastTarget,
): boolean {
  const division = divisionOf(offers, draft);
  const share = draft ? shareOf(draft, target) : undefined;
  return Boolean(division && share && share.amount > division.minimum);
}

/** Whether a chosen target can take one more. */
export function canSharePlus(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
  target: CastTarget,
): boolean {
  const division = divisionOf(offers, draft);
  if (!division || !draft || !shareOf(draft, target)) return false;
  return (
    sharedOut(draft) < division.total ||
    donor(division, draft.shares, target) !== undefined
  );
}

/** Take one from, or give one to, a chosen target (DC-04). */
export function adjustShare(
  offers: readonly CastOffer[],
  draft: CastDraft,
  target: CastTarget,
  change: -1 | 1,
): CastStep {
  const division = divisionOf(offers, draft);
  const allowed =
    change === 1
      ? canSharePlus(offers, draft, target)
      : canShareMinus(offers, draft, target);
  if (!division || !allowed) return IGNORED;
  // One more comes from what is left before it is taken from another target.
  const from =
    change === 1 && sharedOut(draft) >= division.total
      ? donor(division, draft.shares, target)
      : undefined;
  return {
    kind: "draft",
    draft: {
      ...draft,
      shares: draft.shares.map((share) =>
        castTargetKey(share.target) === castTargetKey(target)
          ? { ...share, amount: share.amount + change }
          : share === from
            ? { ...share, amount: share.amount - 1 }
            : share,
      ),
    },
  };
}

/** Whether the whole amount is given out among targets that each hold enough. */
export function canConfirmShares(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): boolean {
  const division = divisionOf(offers, draft);
  return Boolean(
    division &&
    draft &&
    draft.shares.length >= 1 &&
    draft.shares.length <= division.most &&
    draft.shares.every((share) => share.amount >= division.minimum) &&
    sharedOut(draft) === division.total,
  );
}

/** Confirm a finished division: it is cast unless the game pauses for payment. */
export function confirmShares(
  offers: readonly CastOffer[],
  draft: CastDraft,
): CastStep {
  const offer = offerFor(offers, draft.source);
  return offer && canConfirmShares(offers, draft)
    ? settle(offer, draft)
    : IGNORED;
}

/** Pay for a paused draft. Only a draft waiting for payment can be paid. */
export function payCast(
  offers: readonly CastOffer[],
  draft: CastDraft,
): CastStep {
  const offer = offerFor(offers, draft.source);
  return draft.stage === "payment" && offer ? submit(offer, draft) : IGNORED;
}

/**
 * After the game changes: a draft survives only while its card is still
 * offered with everything already chosen. Nothing is chosen on its behalf.
 */
export function reconcileCast(
  offers: readonly CastOffer[],
  draft: CastDraft | null,
): CastDraft | null {
  if (!draft) return null;
  const offer = offerFor(offers, draft.source);
  if (!offer) return null;
  // Every chosen mode must still be one the game offers and allows.
  if (draft.modes.length > 0 && !offer.modes) return null;
  if (offer.modes && draft.stage !== "mode" && draft.modes.length === 0)
    return null;
  for (const chosen of draft.modes) {
    const mode = modeOf(offer, chosen);
    if (!mode || mode.unavailableReason) return null;
  }
  if (draft.modes.length > modeCount(offer)) return null;
  if (Boolean(offer.divide) !== (draft.stage === "divide" || hasShares(draft)))
    return null;
  if (offer.divide) {
    const division = offer.divide;
    if (
      draft.shares.length > division.most ||
      sharedOut(draft) > division.total ||
      draft.shares.some((share) => !offered(division.targets, share.target))
    )
      return null;
    return draft;
  }
  if (draft.stage === "mode") return draft;
  const steps = castSteps(offer, draft.modes);
  if (steps.length !== draft.targets.length) return null;
  const kept = draft.targets.every(
    (target, index) => !target || offered(steps[index]!.targets, target),
  );
  return kept ? draft : null;
}
const hasShares = (draft: CastDraft): boolean => draft.shares.length > 0;

/** The symbols of a printed cost, in order: `{1}{R}` gives `1` and `R`. */
export const costSymbols = (cost: string): string[] =>
  Array.from(cost.matchAll(/\{([^}]+)\}/g), (match) => match[1] ?? "");
