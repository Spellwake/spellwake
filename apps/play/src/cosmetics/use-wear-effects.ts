import { useAtomValue } from "jotai";
import type { WearEffectSelection } from "spellwake-renderer-benchmark/wear-effects";
import { useAccount } from "../auth/AccountProvider";
import { useDemoDefaults } from "./demo-cosmetics";
import {
  connectedWearDefaultsAtom,
  useWearSeeds,
  wearTierOf,
} from "./wear-effects";

/** Read the viewer's active Card Effect and Sleeve Effect for the 3D match tabletop. */
export function useActiveWearEffects(): {
  cardEffect: WearEffectSelection;
  sleeveEffect: WearEffectSelection;
} {
  const { state } = useAccount();
  const demo = useDemoDefaults();
  const connected = useAtomValue(connectedWearDefaultsAtom);
  const { seedFor } = useWearSeeds();

  const isDemo = state.status === "unconfigured";
  const cardTier = wearTierOf(
    isDemo ? demo.card_effect : connected.cardEffectId,
    "card",
  );
  const sleeveTier = wearTierOf(
    isDemo ? demo.sleeve_effect : connected.sleeveEffectId,
    "sleeve",
  );

  return {
    cardEffect: { tier: cardTier, seed: seedFor("card", cardTier) },
    sleeveEffect: { tier: sleeveTier, seed: seedFor("sleeve", sleeveTier) },
  };
}
