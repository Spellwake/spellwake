import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { AccountSession } from "@spellwake/application-contracts";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer, useAccountSummary, useOwnProfile } from "../data/account";
import { useCosmeticCatalogue, usePurchaseHistory } from "../data/cosmetics";
import { useConfirmedDeletion } from "../data/deletion";
import { failed, succeeded } from "../decks/deck-sources";
import type {
  AccountSettingsSource,
  BadgeChoice,
  OwnIdentity,
} from "./account-sources";
import { usePatreonLink } from "./connected-patreon";
import { purchaseRows } from "./purchase-rows";

/**
 * The signed-in account's settings, through the application API. This only
 * composes the existing reads and the existing deletion command. Where the
 * client has no read or command yet, the source says so and offers nothing:
 *
 * - Picture: the catalogue read returns card names without artwork, so there
 *   is nothing to choose from or to draw a saved picture with.
 * - Badge: the account has no saved badge choice.
 * - Patreon in the native app: see `connected-patreon.ts`.
 * - Manage billing: there is no hosted billing page to open.
 */
export type RegisteredSession = AccountSession & { accountId: string };

export function useConnectedAccountSettings(
  owner: RegisteredSession,
  services: ApplicationServices,
): AccountSettingsSource {
  const summary = useAccountSummary();
  const profile = useOwnProfile();
  const queries = useQueryClient();
  const history = usePurchaseHistory();
  const catalogue = useCosmeticCatalogue();
  const deletion = useConfirmedDeletion(owner, services);
  const patreon = usePatreonLink(owner, services);
  const publicSummary = useQuery({
    queryKey: ["player-public-summary", owner.accountId, owner.sessionId],
    queryFn: async ({ signal }) => {
      const res = await services.api.playerPublicSummary(
        await ownerBearer(services, owner),
        owner.accountId,
        signal,
      );
      if (res.status !== "success") throw new Error("Summary unavailable.");
      return res.data;
    },
  });
  const [savedBadge, setSavedBadge] = useState<BadgeChoice | null>(null);
  const activeBadge: BadgeChoice =
    savedBadge ??
    (publicSummary.data?.selectedBadge === "supporter" ? "supporter" : "none");
  const canUseSupporter =
    patreon.link === "active" || profile.data?.supporter === true;

  // Purchases are named from the cosmetic catalogue, which is read in full.
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = catalogue;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const names = useMemo(
    () =>
      new Map(
        (catalogue.data?.pages ?? []).flatMap((page) =>
          page.items.map((item) => [item.id, item.name] as const),
        ),
      ),
    [catalogue.data],
  );
  const rows = useMemo(
    () =>
      purchaseRows(
        (history.data?.pages ?? []).flatMap((page) => page.items),
        names,
      ),
    [history.data, names],
  );

  if (!summary.data)
    return { status: summary.isError ? "unavailable" : "loading" };
  const isRequested =
    owner.state === "deletion_pending" ||
    summary.data.state === "deletion_pending" ||
    (deletion.saved.status === "saved" &&
      deletion.saved.value.state === "accepted");
  return {
    status: "ready",
    username: summary.data.username,
    portrait: null,
    badge: activeBadge,
    picture: null,
    badges: {
      canUseSupporter,
      save: async (choice) => {
        try {
          const bearer = await ownerBearer(services, owner);
          const res = await services.api.setProfileBadge(
            bearer,
            choice === "none" ? null : choice,
          );
          if (res.status !== "success") {
            return failed("Couldn’t save badge choice.");
          }
          setSavedBadge(choice);
          await queries.invalidateQueries({
            queryKey: ["player-public-summary", owner.accountId],
          });
          return succeeded(undefined);
        } catch {
          return failed("Couldn’t reach the server.");
        }
      },
    },
    patreon,
    billing: {
      manage: null,
      purchases: {
        // A failed read keeps the rows it already has and is never shown as empty.
        state: history.data ? "ready" : history.isError ? "failed" : "loading",
        rows,
        loadMore:
          history.hasNextPage && !history.isFetchingNextPage
            ? () => void history.fetchNextPage()
            : null,
      },
    },
    deletion: {
      state: isRequested ? "requested" : "none",
      request: async () => {
        try {
          await deletion.mutation.mutateAsync();
          return succeeded(undefined);
        } catch {
          // A request that got no answer may still have been received.
          return failed("Couldn’t confirm deletion.");
        }
      },
    },
  };
}

/** The account's picture and badge. Neither can be read yet, so neither is shown. */
export const CONNECTED_IDENTITY: OwnIdentity = {
  portrait: null,
  badge: "none",
};
