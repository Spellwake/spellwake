import { useAccount } from "../../auth/AccountProvider";
import { endCurrentSession } from "../../auth/services";
import type { OwnIdentity } from "../../account/account-sources";
import { useConnectedIdentity } from "../../account/connected-account";
import { DEMO_USERNAME, useDemoIdentity } from "../../account/demo-account";
import { useAccountSummary } from "../../data/account";
import { coinsOf, useDemoCollection } from "../../collection/demo-collection";

export type HeaderAccount = {
  /** Available coins, or null while they are unknown. */
  coins: number | null;
  username: string | null;
  /** The person's picture and badge, as far as they are known. */
  identity: OwnIdentity;
  /** Ends this device's session. Null where there is no account to leave. */
  signOut: (() => Promise<boolean>) | null;
};

/**
 * What the header shows about the person. A build with no application API has
 * no account: it shows the preview's stand-in person and wallet, and offers
 * no sign out.
 */
export function useHeaderAccount(): HeaderAccount {
  const { state, services } = useAccount();
  const summary = useAccountSummary();
  const preview = useDemoIdentity();
  const connectedIdentity = useConnectedIdentity();
  const [collection] = useDemoCollection();
  if (state.status === "unconfigured")
    return {
      coins: coinsOf(collection),
      username: DEMO_USERNAME,
      identity: preview,
      signOut: null,
    };
  const available = Number(summary.data?.wallet.available);
  return {
    // A balance too large to show exactly is shown as unknown, never rounded.
    coins: Number.isSafeInteger(available) ? available : null,
    username: summary.data?.username ?? null,
    identity: connectedIdentity,
    signOut:
      services && state.status === "ready"
        ? () => endCurrentSession(services)
        : null,
  };
}
