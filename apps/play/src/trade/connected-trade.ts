import { useAccount } from "../auth/AccountProvider";
import type { TradeSource } from "./trade-sources";

/**
 * The account's trade room. The application service offers immutable trade
 * offers: one side proposes two fixed bundles and the other accepts, rejects
 * or cancels. The approved room needs shared editable offers, Ready-time
 * reservations and a confirmation from each person on the exact current
 * terms, which that protocol cannot express. Rather than present local edits
 * as reservations the service never made, the account's room says it is
 * unavailable until the service offers shared rooms.
 */
export function useConnectedTrade(): TradeSource {
  const { state } = useAccount();
  if (state.status === "signed_out") return { status: "signed-out" };
  if (state.status !== "ready") return { status: "loading" };
  if (state.session.state === "needs_registration")
    return { status: "unregistered" };
  return { status: "unavailable" };
}
