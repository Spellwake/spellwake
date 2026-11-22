import { useEffect, useMemo, useRef, useState } from "react";
import type { SocialPersonSummary } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { failed, succeeded, type Outcome } from "../decks/deck-sources";
import {
  useDirectMessages,
  useSocialPeople,
  useSocialStatus,
} from "../data/social";
import { useSocialCommand } from "../data/social-commands";
import { useSocialDraft } from "../data/social-draft";
import type { SocialIntent } from "../data/social-journal";
import type { SocialJournal } from "../data/social-recovery";
import {
  canSend,
  type Delivery,
  type MessagingPerson,
  type MessagingSource,
  type PersonAction,
  type ThreadEvent,
} from "./messaging-sources";

const COULDNT = "Couldn’t update this player.";
const UNCONFIRMED = "Couldn’t confirm the change.";
const SEND_REFUSED = "Couldn’t send message.";

const personFrom = (person: SocialPersonSummary): MessagingPerson => ({
  id: person.id,
  name: person.username,
  portrait: null,
  relationship:
    person.relationship === "incoming_request"
      ? "incoming"
      : person.relationship === "outgoing_request"
        ? "outgoing"
        : person.relationship,
  isBlocked: person.isBlocked,
  canContact: person.canContact,
  isDeleted: person.isDeleted,
  unread: Number(person.unreadMessages),
  preview:
    person.relationship === "incoming_request"
      ? "Friend request"
      : person.relationship === "outgoing_request"
        ? "Request sent"
        : person.conversationId
          ? "Messages"
          : null,
});

/** Runs one social command and settles to its journal, or null when no answer could be had. */
function settleWith(
  command: ReturnType<typeof useSocialCommand>,
  input: Parameters<ReturnType<typeof useSocialCommand>["run"]["mutate"]>[0],
): Promise<SocialJournal | null> {
  return new Promise((resolve) =>
    command.run.mutate(input, {
      onSuccess: (journal) => resolve(journal),
      onError: () => resolve(null),
    }),
  );
}

/**
 * The account's Messages. The rail and Requests come from the retained
 * threads projection; the open person from their own status; history from
 * their conversation, read in pages of sixteen. Each relationship change,
 * conversation opening, message and read acknowledgment is one journaled
 * command, and an unknown send keeps its text and offers Check delivery,
 * which asks about that same attempt. The projection carries no pictures,
 * trade or game cards, so none are shown.
 */
export function useConnectedMessaging(
  personId: string | null,
): MessagingSource {
  const { state } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const threads = useSocialPeople("threads", "", true);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = threads;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const [query, setQuery] = useState("");
  const trimmed = query.trim();
  const directory = useSocialPeople("directory", trimmed, trimmed.length > 0);
  const status = useSocialStatus(personId, personId !== null);
  const [dismissed, setDismissed] = useState(false);
  const [delivery, setDelivery] = useState<Delivery>({ state: "idle" });
  const [localDrafts, setLocalDrafts] = useState<Record<string, string>>({});
  useEffect(() => setDelivery({ state: "idle" }), [personId]);

  const people = useMemo(
    () =>
      (threads.data?.pages ?? []).flatMap((page) => page.items).map(personFrom),
    [threads.data],
  );
  const selected = status.data?.person ?? null;
  const person = selected ? personFrom(selected) : null;
  const conversationId = selected?.conversationId ?? null;
  const messages = useDirectMessages(conversationId, conversationId !== null);
  const draft = useSocialDraft(conversationId);

  const friendship = useSocialCommand(
    personId ? { kind: "friendship", targetId: personId } : null,
  );
  const block = useSocialCommand(
    personId ? { kind: "block", targetId: personId } : null,
  );
  const openDm = useSocialCommand(
    personId ? { kind: "open_dm", targetId: personId } : null,
  );
  const sendDm = useSocialCommand(
    conversationId ? { kind: "send_dm", targetId: conversationId } : null,
  );
  const readDm = useSocialCommand(
    conversationId ? { kind: "read_dm", targetId: conversationId } : null,
  );

  const events = useMemo((): ThreadEvent[] => {
    const all = (messages.data?.pages ?? []).flatMap((page) => page.items);
    const unique = new Map(all.map((message) => [message.id, message]));
    return [...unique.values()]
      .sort(
        (a, b) => a.sentAt.localeCompare(b.sentAt) || a.id.localeCompare(b.id),
      )
      .map((message) => ({
        kind: "message",
        id: message.id,
        isMine: message.senderId === owner?.accountId,
        body: message.body,
        sentAt: message.sentAt,
      }));
  }, [messages.data, owner?.accountId]);

  // Seeing unread messages acknowledges exactly those, once.
  const acknowledged = useRef("");
  useEffect(() => {
    if (!conversationId || !person || person.unread === 0) return;
    const unseen = events
      .filter((event) => event.kind === "message" && !event.isMine)
      .slice(-16)
      .map((event) => event.id);
    const key = `${conversationId}:${unseen.join(",")}`;
    if (unseen.length === 0 || acknowledged.current === key) return;
    if (readDm.journal.status === "unavailable") return;
    acknowledged.current = key;
    const prior =
      readDm.journal.status === "saved" ? readDm.journal.value : null;
    if (prior?.state === "unresolved") return;
    readDm.run.mutate({
      type: "submit",
      expected: prior,
      draft: {
        kind: "read",
        input: {
          requestId: crypto.randomUUID(),
          conversationId,
          messageIds: unseen,
        },
      },
    });
  }, [conversationId, person, events, readDm]);

  /** Sends one social intent through its slot, settling an earlier unanswered one first. */
  const run = async (
    command: ReturnType<typeof useSocialCommand>,
    intent: SocialIntent,
  ): Promise<Outcome> => {
    if (command.journal.status === "unavailable") return failed(COULDNT);
    let prior =
      command.journal.status === "saved" ? command.journal.value : null;
    if (prior?.state === "unresolved") {
      prior = await settleWith(command, { type: "recover", expected: prior });
      if (!prior || prior.state === "unresolved") return failed(UNCONFIRMED);
    }
    const result = await settleWith(command, {
      type: "submit",
      expected: prior,
      draft: intent,
    });
    if (!result || result.state === "unresolved") return failed(UNCONFIRMED);
    return result.state === "accepted" ? succeeded(undefined) : failed(COULDNT);
  };
  const act = async (id: string, action: PersonAction): Promise<Outcome> => {
    if (id !== personId) return failed(COULDNT);
    const requestId = crypto.randomUUID();
    if (action === "block" || action === "unblock")
      return run(block, {
        kind: "social",
        input: {
          kind: "block",
          command: {
            requestId,
            otherAccountId: id,
            blocked: action === "block",
          },
        },
      });
    return run(friendship, {
      kind: "social",
      input: {
        kind: "friendship",
        command: {
          requestId,
          otherAccountId: id,
          // Declining, cancelling and removing all end the friendship record.
          action:
            action === "add"
              ? "request"
              : action === "accept"
                ? "accept"
                : "remove",
        },
      },
    });
  };

  const text = conversationId
    ? draft.text
    : (localDrafts[personId ?? ""] ?? "");
  const send = () => {
    if (!person || !personId || !canSend(text) || !person.canContact) return;
    if (delivery.state === "sending" || delivery.state === "unconfirmed")
      return;
    setDelivery({ state: "sending" });
    void (async () => {
      let target = conversationId;
      if (!target) {
        // A first message opens the conversation, then is sent into it.
        const opened = await run(openDm, {
          kind: "social",
          input: {
            kind: "open_dm",
            command: {
              requestId: crypto.randomUUID(),
              otherAccountId: personId,
            },
          },
        });
        const journal =
          openDm.journal.status === "saved" ? openDm.journal.value : null;
        target = journal?.state === "accepted" ? journal.ack.resourceId : null;
        if (opened.status !== "success" || !target)
          return setDelivery({ state: "rejected", message: SEND_REFUSED });
        await status.refetch();
      }
      const body = text;
      const prior =
        sendDm.journal.status === "saved" ? sendDm.journal.value : null;
      const result = await settleWith(sendDm, {
        type: "submit",
        expected: prior?.state === "unresolved" ? null : prior,
        draft: {
          kind: "social",
          input: {
            kind: "send_dm",
            command: {
              requestId: crypto.randomUUID(),
              conversationId: target,
              body,
            },
          },
        },
      });
      if (result?.state === "accepted") {
        // Only the exact text that was sent is cleared; a newer edit stays.
        draft.clearSent(body);
        setLocalDrafts((current) =>
          current[personId] === body ? { ...current, [personId]: "" } : current,
        );
        setDelivery({ state: "idle" });
      } else if (result?.state === "rejected")
        setDelivery({ state: "rejected", message: SEND_REFUSED });
      else setDelivery({ state: "unconfirmed", isChecking: false });
    })();
  };
  const checkDelivery = () => {
    const prior =
      sendDm.journal.status === "saved" ? sendDm.journal.value : null;
    if (!prior || prior.state !== "unresolved")
      return setDelivery({ state: "idle" });
    setDelivery({ state: "unconfirmed", isChecking: true });
    void settleWith(sendDm, { type: "recover", expected: prior }).then(
      (result) => {
        if (result?.state === "accepted") {
          const sent = prior.command.intent;
          if (sent.kind === "social" && sent.input.kind === "send_dm")
            draft.clearSent(sent.input.command.body);
          setDelivery({ state: "idle" });
        } else if (result?.state === "rejected")
          setDelivery({ state: "rejected", message: SEND_REFUSED });
        else setDelivery({ state: "unconfirmed", isChecking: false });
      },
    );
  };

  if (!owner)
    return { status: state.status === "signed_out" ? "signed-out" : "loading" };
  if (threads.isPending) return { status: "loading" };
  if (threads.isError && people.length === 0) return { status: "unavailable" };
  const friends = people.filter((p) => p.relationship === "friend");
  const requests = people.filter(
    (p) =>
      p.relationship !== "friend" &&
      (p.relationship !== "none" || p.preview !== null),
  );
  return {
    status: "ready",
    friends,
    requests,
    attention: requests.filter(
      (p) => p.relationship === "incoming" || p.unread > 0,
    ).length,
    failure: threads.isError && !dismissed ? "Couldn’t load everyone." : null,
    dismissFailure: () => setDismissed(true),
    thread: person
      ? {
          person,
          state:
            conversationId === null
              ? "ready"
              : messages.isPending
                ? "loading"
                : messages.isError && events.length === 0
                  ? "failed"
                  : "ready",
          events,
          hasOlder: !!messages.hasNextPage,
          loadOlder: () => void messages.fetchNextPage(),
        }
      : null,
    search: {
      query,
      setQuery,
      state: !trimmed
        ? "idle"
        : directory.isPending
          ? "searching"
          : directory.isError
            ? "failed"
            : "ready",
      results: (directory.data?.pages ?? [])
        .flatMap((page) => page.items)
        .map(personFrom),
    },
    draft: text,
    setDraft: (next) => {
      if (conversationId) draft.change(next);
      else if (personId)
        setLocalDrafts((current) => ({ ...current, [personId]: next }));
    },
    delivery,
    send,
    checkDelivery,
    act,
    respond: null,
  };
}
