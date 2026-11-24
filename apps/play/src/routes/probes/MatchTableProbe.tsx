import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { useDemoMatch } from "../../match/demo-match";
import {
  previewScene,
  previewSnapshot,
  type PreviewScene,
} from "../../match/preview-table";
import type { GameResult } from "../../match/match-sources";
import { MatchPage, type EndLook } from "../match/MatchPage";

const noop = (): void => undefined;

const END_LOOKS: readonly EndLook[] = ["panel", "dialog", "band"];
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
];

/**
 * Development only: the preview match started part-way through a turn, for
 * the stress checks and review captures. `?state=` names the scene: declaring
 * attackers or blockers, assigning combat damage, or spells on the stack.
 */
export function MatchTableProbe(): ReactElement {
  const [params] = useSearchParams();
  const scene = SCENES.find((name) => name === params.get("state"));
  const source = useDemoMatch(noop, () =>
    scene ? previewScene(scene) : previewSnapshot(),
  );
  // `?chat=refused` makes the table refuse every message, to show that line.
  // `?pay=paused` makes every cast pause for payment, as a manual game would.
  const endLook = END_LOOKS.find((look) => look === params.get("endLook"));
  const refused = params.get("chat") === "refused";
  const paused = params.get("pay") === "paused";
  const shown =
    source.status !== "ready"
      ? source
      : {
          ...source,
          result: previewResult(
            params.get("end"),
            source.snapshot.viewerSeatId,
            source.snapshot.players
              .map((player) => player.seatId)
              .filter((seat) => seat !== source.snapshot.viewerSeatId),
          ),
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
  return <MatchPage source={shown} {...(endLook ? { endLook } : {})} />;
}
