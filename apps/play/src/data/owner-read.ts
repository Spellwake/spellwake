import type { AccountSession } from "@spellwake/application-contracts";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer } from "./account";

export type OwnerReadScope = {
  owner: Exclude<AccountSession, { state: "needs_registration" }>;
  services: { auth: Pick<ApplicationServices["auth"], "getSession"> };
  isCurrent: () => boolean;
};

/** Fence credential waits and response delivery against owner replacement and cancellation. */
export async function readForOwner<T>(
  scope: OwnerReadScope,
  signal: AbortSignal,
  read: (bearer: string) => Promise<T>,
): Promise<T> {
  const assertCurrent = () => {
    signal.throwIfAborted();
    if (!scope.isCurrent())
      throw new Error("Your account changed. Reopen this view.");
  };
  assertCurrent();
  const bearer = await ownerBearer(scope.services, scope.owner);
  assertCurrent();
  const result = await read(bearer);
  assertCurrent();
  return result;
}
