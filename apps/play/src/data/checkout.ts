import { useEffect, useMemo } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "./account";
import { withAccountStorageLock } from "./account-storage";
import { createCheckoutIntentStore } from "./checkout-intent";
import { checkoutRejectionSchema } from "@spellwake/application-contracts";

/** One selected product owns the recovery UI; locks serialize its requests across tabs. */
export function useCheckout(cosmeticId: string) {
  const { state, services } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  const queries = useQueryClient();
  const store = useMemo(
    () =>
      createCheckoutIntentStore(
        services?.api.origin ?? "unconfigured",
        owner?.accountId ?? "",
        cosmeticId,
        () => window.localStorage,
      ),
    [services, owner?.accountId, cosmeticId],
  );
  const intent = useAtomValue(store.stateAtom);
  const act = useSetAtom(store.actionAtom);
  const purchaseId = intent.status === "saved" ? intent.value.purchaseId : null;
  const purchase = useQuery({
    queryKey: ["purchase", owner?.accountId, owner?.sessionId, purchaseId],
    enabled: !!purchaseId && !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!purchaseId || !owner || !services)
        throw new Error("Sign in to recover checkout.");
      const result = await services.api.purchase(
        await ownerBearer(services, owner),
        purchaseId,
        signal,
      );
      if (result.status !== "success" || result.data.cosmeticId !== cosmeticId)
        throw new Error("Purchase could not be loaded.");
      return result.data;
    },
    refetchInterval: 5000,
  });
  useEffect(() => {
    if (purchase.data?.revision !== undefined)
      void queries.invalidateQueries({
        queryKey: ["cosmetics", owner?.accountId, owner?.sessionId],
      });
  }, [
    purchase.data?.state,
    purchase.data?.revision,
    queries,
    owner?.accountId,
    owner?.sessionId,
  ]);
  const reserve = useMutation({
    mutationFn: async (restart: boolean) => {
      if (!owner || !services || owner.state !== "active")
        throw new Error("An active account is required.");
      if (!navigator.locks)
        throw new Error("Checkout recovery is unavailable in this browser.");
      return navigator.locks.request(store.key, async () => {
        let saved = await withAccountStorageLock(
          services.api.origin,
          owner.accountId,
          () => act({ type: "prepare" }),
        );
        if (!saved)
          throw new Error("Allow browser storage, then retry this checkout.");
        if (saved.purchaseId) {
          const observed = await services.api.purchase(
            await ownerBearer(services, owner),
            saved.purchaseId,
          );
          if (
            observed.status !== "success" ||
            observed.data.cosmeticId !== cosmeticId
          )
            throw new Error("Purchase could not be recovered. Try again.");
          if (!restart) {
            await purchase.refetch();
            return saved.purchaseId;
          }
          if (
            !["canceled", "refunded", "reversed"].includes(observed.data.state)
          )
            throw new Error(
              "Recover the existing checkout before starting another.",
            );
          const prior = {
            requestId: saved.requestId,
            purchaseId: saved.purchaseId,
          };
          saved = await withAccountStorageLock(
            services.api.origin,
            owner.accountId,
            () =>
              act({
                type: "replace_settled",
                requestId: prior.requestId,
                purchaseId: prior.purchaseId,
              }),
          );
          if (!saved)
            throw new Error(
              "The new checkout identity could not be saved. Retry.",
            );
        }
        const result = await services.api.checkout(
          await ownerBearer(services, owner),
          saved.requestId,
          cosmeticId,
        );
        if (result.status === "fail") {
          const rejection = checkoutRejectionSchema.safeParse(result.data);
          if (
            rejection.success &&
            rejection.data.requestId === saved.requestId &&
            rejection.data.cosmeticId === cosmeticId
          ) {
            const prior = saved;
            if (
              !(await withAccountStorageLock(
                services.api.origin,
                owner.accountId,
                () =>
                  act({ type: "replace_rejected", requestId: prior.requestId }),
              ))
            )
              throw new Error(
                "The rejected checkout could not be reset. Retry recovery.",
              );
          }
          throw new Error("Checkout is unavailable for this cosmetic.");
        }
        if (result.status !== "success")
          throw new Error(
            "Checkout is still unresolved. Retry to recover the same purchase.",
          );
        const prior = saved;
        const acknowledged = await withAccountStorageLock(
          services.api.origin,
          owner.accountId,
          () =>
            act({
              type: "acknowledge",
              requestId: prior.requestId,
              purchaseId: result.data.resourceId,
            }),
        );
        if (!acknowledged)
          throw new Error(
            "Checkout was received. Retry to recover its saved result.",
          );
        void queries.invalidateQueries({
          queryKey: ["purchases", owner.accountId, owner.sessionId],
        });
        return acknowledged.purchaseId;
      });
    },
  });
  return { intent, purchase, reserve };
}
