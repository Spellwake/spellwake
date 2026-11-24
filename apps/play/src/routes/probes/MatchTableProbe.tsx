import { useState, type ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { useDemoMatch } from "../../match/demo-match";
import {
  previewScene,
  previewSnapshot,
  type PreviewScene,
} from "../../match/preview-table";
import type { GameResult, MatchSeries } from "../../match/match-sources";
import { MatchPage } from "../match/MatchPage";

const noop = (): void => undefined;

/** `?end=` finishes the game for the viewer: won, lost, draw or none. */
function previewResult(
  end: string | null,
  viewer: string,
  others: readonly string[],
): GameResult | null {
  if (end === "won")
    return {
      outcome: "won",
      reason: "Every other player is out.",
      winnerSeatIds: [viewer],
    };
  if (end === "lost")
    return {
      outcome: "lost",
      reason: "Your life reached 0.",
      winnerSeatIds: others.slice(0, 1),
    };
  if (end === "draw")
    return { outcome: "draw", reason: null, winnerSeatIds: [] };
  if (end === "none")
    return {
      outcome: "no-result",
      reason: "The game could not be finished.",
      winnerSeatIds: [],
    };
  return null;
}

/**
 * `?series=` makes the finished game one of a best of three: `deciding` (the
 * next game waits on the viewer, with two minutes on the clock), `untimed`,
 * `stuck` (the answer was not taken), `waiting`, or `over`.
 */
function previewSeries(
  kind: string | null,
  result: GameResult | null,
  deadlineAt: string,
  answer: { sent: boolean; send: () => void },
): MatchSeries | null {
  if (!result) return null;
  const wonGame = result.outcome === "won";
  const first = {
    gameNumber: 1,
    score: wonGame ? { you: 1, them: 0 } : { you: 0, them: 1 },
  };
  if (kind === "over")
    return {
      gameNumber: 3,
      score: wonGame ? { you: 2, them: 1 } : { you: 1, them: 2 },
      next: { kind: "over", outcome: result.outcome },
    };
  if (kind === "waiting" || answer.sent)
    return { ...first, next: { kind: "waiting" } };
  if (kind === "deciding" || kind === "untimed" || kind === "stuck")
    return {
      ...first,
      next: {
        kind: "deciding",
        deadlineAt: kind === "untimed" ? null : deadlineAt,
        ready: answer.send,
        readyPending: false,
        readyFailed: kind === "stuck",
      },
    };
  return null;
}

const SCENES: readonly PreviewScene[] = [
  "attack",
  "block",
  "damage",
  "damage-two",
  "trample",
  "stack",
  "stack-long",
  "stack-waiting",
  "watching",
  "no-artifacts",
  "ask-order",
  "ask-targets",
  "ask-counters",
  "ask-prevention",
  "ask-modes",
  "ask-attack-cost",
];

/**
 * Development only: the preview match started part-way through a turn, for
 * the stress checks and review captures. `?state=` names the scene: declaring
 * attackers or blockers, assigning combat damage, spells on the stack, or
 * (`ask-…`) one of the questions answered by building something up.
 */
export function MatchTableProbe(): ReactElement {
  const [params] = useSearchParams();
  const scene = SCENES.find((name) => name === params.get("state"));
  const source = useDemoMatch(noop, () =>
    scene ? previewScene(scene) : previewSnapshot(),
  );
  // `?chat=refused` makes the table refuse every message, to show that line.
  // `?pay=paused` makes every cast pause for payment, as a manual game would.
  // The clock starts when the page opens, as it would when the game ended.
  const [deadlineAt] = useState(() =>
    new Date(Date.now() + 161_000).toISOString(),
  );
  const [sent, setSent] = useState(false);
  const refused = params.get("chat") === "refused";
  const paused = params.get("pay") === "paused";
  const result =
    source.status !== "ready"
      ? null
      : previewResult(
          params.get("end"),
          source.snapshot.viewerSeatId,
          source.snapshot.players
            .map((player) => player.seatId)
            .filter((seat) => seat !== source.snapshot.viewerSeatId),
        );
  const shown =
    source.status !== "ready"
      ? source
      : {
          ...source,
          result,
          series: previewSeries(params.get("series"), result, deadlineAt, {
            sent,
            send: () => setSent(true),
          }),
          ...(refused
            ? { chat: { ...source.chat, send: () => Promise.resolve(false) } }
            : {}),
          ...(paused && source.castOffers
            ? {
                castOffers: source.castOffers.map((offer) => ({
                  ...offer,
                  payment: "paused" as const,
                })),
              }
            : {}),
        };
  return <MatchPage source={shown} />;
}
