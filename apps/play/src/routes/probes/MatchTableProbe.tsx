import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { useDemoMatch } from "../../match/demo-match";
import {
  previewScene,
  previewSnapshot,
  type PreviewScene,
} from "../../match/preview-table";
import { MatchPage } from "../match/MatchPage";

const noop = (): void => undefined;

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
  const refused = params.get("chat") === "refused";
  const paused = params.get("pay") === "paused";
  const shown =
    source.status !== "ready"
      ? source
      : {
          ...source,
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
