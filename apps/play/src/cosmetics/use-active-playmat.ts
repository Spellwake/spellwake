import { useAtomValue } from "jotai";
import type { PlaymatEffectSelection } from "spellwake-renderer-benchmark/playmat-effects";
import { useAccount } from "../auth/AccountProvider";
import { useAccountSummary } from "../data/account";
import { useDemoDefaults } from "./demo-cosmetics";
import {
  connectedPlaymatEffectDefaultAtom,
  playmatEffectIdOf,
  usePlaymatEffectSeeds,
} from "./playmat-effects";

/**
 * Read the viewer's equipped playmat ID (null when none is equipped, which is the
 * default because no playmats are granted free).
 */
export function useActivePlaymat(): string | null {
  const { state } = useAccount();
  const account = useAccountSummary();
  const demo = useDemoDefaults();
  return state.status === "unconfigured"
    ? demo.playmat
    : (account.data?.defaults.playmatId ?? null);
}

/**
 * Read the viewer's active Playmat Effect and persisted pattern seed for the 3D tabletop.
 * Defaults cleanly to Clean (zero overhead / bypassed).
 */
export function useActivePlaymatEffect(): PlaymatEffectSelection {
  const { state } = useAccount();
  const demo = useDemoDefaults();
  const connectedId = useAtomValue(connectedPlaymatEffectDefaultAtom);
  const { seedFor } = usePlaymatEffectSeeds();

  const effectId = playmatEffectIdOf(
    state.status === "unconfigured" ? demo.playmat_effect : connectedId,
  );
  return { id: effectId, seed: seedFor(effectId) };
}
