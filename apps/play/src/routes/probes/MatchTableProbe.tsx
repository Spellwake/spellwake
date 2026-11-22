import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { useDemoMatch } from "../../match/demo-match";
import { previewScene, previewSnapshot } from "../../match/preview-table";
import { MatchPage } from "../match/MatchPage";

const noop = (): void => undefined;

/**
 * Development only: the preview match started part-way through combat, for
 * the stress checks and review captures. `?state=attack` offers attackers and
 * `?state=block` offers blockers against Alex's attack.
 */
export function MatchTableProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state");
  const source = useDemoMatch(noop, () =>
    state === "attack" || state === "block"
      ? previewScene(state)
      : previewSnapshot(),
  );
  return <MatchPage source={source} />;
}
