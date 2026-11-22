import { useEffect, useRef, useState } from "react";
import { atom, useAtom } from "jotai";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer, useOwnProfile } from "../data/account";
import { newPatreonLinkRequest } from "../data/patreon-association";
import {
  completionFor,
  flowOf,
  readPatreonLink,
  writePatreonLink,
  type PatreonReturn,
} from "../data/patreon-link";
import { failed, succeeded, type Outcome } from "../decks/deck-sources";
import type { AccountSettingsSource, PatreonLink } from "./account-sources";
import type { RegisteredSession } from "./connected-account";

/**
 * Linking the signed-in account to Patreon, in the browser.
 *
 * Connect saves one request on this device, asks the application API to issue
 * it, and opens Patreon's own authorisation in this tab. Patreon sends the
 * person back to `/patreon/return`, which hands what came back to this hook
 * and reopens Account settings. The return is finished here, once, and only
 * if its state matches the saved request. The code is held in memory only.
 *
 * Coming back from Patreon proves nothing about membership. "Active
 * supporter" is shown only when the account's own profile says so.
 *
 * The native app is not linked here: Patreon returns to a web address, and
 * handing that back to the app is not built. It offers neither action.
 */

/** What Patreon sent back, held until Account settings finishes it. Memory only. */
export const patreonReturnAtom = atom<PatreonReturn | null>(null);

const MANAGE_URL = "https://www.patreon.com/settings/memberships";
const FAILED = "Couldn’t link Patreon.";
/** If Patreon has not opened by now, the browser refused to leave the page. */
const OPEN_WAIT_MS = 8000;

type Patreon = Extract<AccountSettingsSource, { status: "ready" }>["patreon"];

export function usePatreonLink(
  owner: RegisteredSession,
  services: ApplicationServices,
): Patreon {
  const origin = services.api.origin;
  const { isCurrentSession } = useAccount();
  const queries = useQueryClient();
  const profile = useOwnProfile();
  const [record, setRecord] = useState(() =>
    readPatreonLink(origin, owner.accountId),
  );
  const [returned, setReturned] = useAtom(patreonReturnAtom);
  const [isLinking, setIsLinking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const isMounted = useRef(true);
  useEffect(() => {
    isMounted.current = true;
    return () => {
      isMounted.current = false;
    };
  }, []);

  const accountPatreon = useQuery({
    queryKey: ["patreon-account-status", owner.accountId, owner.sessionId],
    queryFn: async ({ signal }) => {
      const result = await services.api.patreonAccountStatus(
        await ownerBearer(services, owner),
        signal,
      );
      if (result.status !== "success")
        throw new Error("Patreon account status unavailable.");
      return result.data;
    },
  });

  const status = useQuery({
    // The request's private state is never part of a key.
    queryKey: [
      "patreon-link",
      owner.accountId,
      owner.sessionId,
      record?.requestId ?? null,
    ],
    enabled: record !== null,
    queryFn: async ({ signal }) => {
      if (!record) throw new Error("No Patreon link request is saved.");
      const result = await services.api.patreonStatus(
        await ownerBearer(services, owner),
        owner.accountId,
        flowOf(record),
        signal,
      );
      if (result.status !== "success")
        throw new Error("The Patreon link could not be read.");
      return result.data;
    },
  });

  // Finish a return from Patreon. It is taken once, so a second render, a
  // second tab or a reload cannot send the same code again.
  const isFinishing = useRef(false);
  useEffect(() => {
    if (!returned || isFinishing.current) return;
    isFinishing.current = true;
    setReturned(null);
    setIsLinking(true);
    const finish = async (): Promise<boolean> => {
      const saved = readPatreonLink(origin, owner.accountId);
      const completion = completionFor(saved, returned);
      if (!saved || !completion) {
        // A refusal ends the request it answered. A return for some other
        // state leaves the saved request alone: it may not be ours.
        if (saved?.phase === "opened" && returned.kind === "refused")
          await writePatreonLink(origin, owner.accountId, null);
        return false;
      }
      if (!isCurrentSession(owner)) return false;
      const bearer = await ownerBearer(services, owner);
      const result = await services.api.patreonComplete(
        bearer,
        owner.accountId,
        completion,
      );
      let isLinked =
        result.status === "success" && result.data.phase === "linked";
      if (result.status === "error") {
        // No answer is not a refusal: the exchange may have happened. The
        // request's own status says, and the code is never sent a second time.
        const observed = await services.api.patreonStatus(
          bearer,
          owner.accountId,
          flowOf(saved),
        );
        isLinked =
          observed.status === "success" && observed.data.phase === "linked";
      }
      if (isLinked)
        await writePatreonLink(origin, owner.accountId, {
          ...saved,
          phase: "linked",
        });
      // A refused exchange is spent. Linking again needs a new request.
      else if (result.status === "fail")
        await writePatreonLink(origin, owner.accountId, null);
      return isLinked;
    };
    void finish()
      .catch(() => false)
      .then(async (isLinked) => {
        setRecord(readPatreonLink(origin, owner.accountId));
        if (!isLinked) setProblem(FAILED);
        await Promise.all(
          ["own-profile", "patreon-link"].map((family) =>
            queries.invalidateQueries({ queryKey: [family, owner.accountId] }),
          ),
        );
      })
      .finally(() => {
        isFinishing.current = false;
        setIsLinking(false);
      });
  }, [
    returned,
    setReturned,
    origin,
    owner,
    services,
    queries,
    isCurrentSession,
  ]);

  const isSupporter =
    accountPatreon.data?.supporter ?? profile.data?.supporter;
  const isLinked =
    accountPatreon.data !== undefined
      ? accountPatreon.data.linked
      : record !== null &&
        (status.data ? status.data.phase === "linked" : record.phase === "linked");
  const link: PatreonLink =
    isSupporter === true
      ? "active"
      : isLinked
        ? isSupporter === false
          ? "inactive"
          : "linked"
        : accountPatreon.data !== undefined
          ? "unlinked"
          : "unknown";
  const canLink =
    !isTauri() &&
    typeof navigator !== "undefined" &&
    Boolean(navigator.locks) &&
    owner.state === "active";

  const connect = async (): Promise<Outcome> => {
    const flow = newPatreonLinkRequest();
    // Saved first: a return that cannot be matched to a request is refused.
    const isSaved = await writePatreonLink(origin, owner.accountId, {
      version: 1,
      ...flow,
      phase: "opened",
    });
    if (!isSaved) return failed(FAILED);
    setRecord(readPatreonLink(origin, owner.accountId));
    let started;
    try {
      started = await services.api.patreonStart(
        await ownerBearer(services, owner),
        owner.accountId,
        flow,
      );
    } catch {
      started = null;
    }
    if (started?.status !== "success") {
      await writePatreonLink(origin, owner.accountId, null);
      if (isMounted.current) setRecord(null);
      return failed(FAILED);
    }
    // Account settings was closed while this was prepared: do not open
    // Patreon unasked. The issued request is left to expire.
    if (!isMounted.current) return succeeded(undefined);
    window.location.assign(started.data.authorizationUrl);
    await new Promise((resolve) => window.setTimeout(resolve, OPEN_WAIT_MS));
    return failed(FAILED);
  };

  return {
    link,
    connect:
      canLink && (link === "unknown" || link === "unlinked") ? connect : null,
    manage: isTauri()
      ? null
      : () => {
          window.open(MANAGE_URL, "_blank", "noopener");
          return Promise.resolve(succeeded(undefined));
        },
    isLinking,
    problem,
    dismissProblem: () => setProblem(null),
  };
}
