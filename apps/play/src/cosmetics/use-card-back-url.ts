import { useAccount } from "../auth/AccountProvider";
import { useAccountSummary } from "../data/account";
import { cardBackUrl } from "./card-backs";
import { useDemoDefaults } from "./demo-cosmetics";

/** Read the current viewer's default for face-down cards. */
export function useCardBackUrl(): string {
  const { state } = useAccount();
  const account = useAccountSummary();
  const demo = useDemoDefaults();
  return cardBackUrl(
    state.status === "unconfigured" ? demo.back : account.data?.defaults.backId,
  );
}
