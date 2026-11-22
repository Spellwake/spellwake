import { useCallback, useEffect, useMemo, useState } from "react";
import { atom, useAtom } from "jotai";
import type { Portrait } from "../account/account-sources";
import { CENTRED } from "../account/portrait-crop";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import { failed, succeeded, type Outcome } from "../decks/deck-sources";
import {
  canSend,
  type Delivery,
  type MessagingPerson,
  type MessagingSource,
  type PersonAction,
  type Relationship,
  type ThreadEvent,
} from "./messaging-sources";

/**
 * The local preview of Messages: a few friends, a friend request, a
 * non-friend's message and some players to find. Sending appends your
 * message after a moment; relationship actions change the preview only.
 * Nothing reaches an account or another person.
 */
type DemoPerson = {
  id: string;
  name: string;
  art: string | null;
  relationship: Relationship;
  isBlocked: boolean;
  isDeleted: boolean;
  unread: number;
  /** Shown by Find People; a person with no thread is only found. */
  inThreads: boolean;
};
type DemoState = {
  people: DemoPerson[];
  events: Record<string, ThreadEvent[]>;
  drafts: Record<string, string>;
};
const at = (minutes: number): string =>
  new Date(Date.UTC(2026, 10, 21, 14, minutes)).toISOString();
const person = (
  id: string,
  name: string,
  art: string | null,
  relationship: Relationship,
  extra: Partial<DemoPerson> = {},
): DemoPerson => ({
  id,
  name,
  art,
  relationship,
  isBlocked: false,
  isDeleted: false,
  unread: 0,
  inThreads: true,
  ...extra,
});
const SEED: DemoState = {
  people: [
    person("mira", "Mira", "Goblin Matron", "friend"),
    person("elowen", "Elowen", "Aether Vial", "friend"),
    person("lena", "Lena", "Conspicuous Snoop", "friend"),
    person("arden", "Arden", "Goblin Chieftain", "incoming"),
    person("nia", "Nia", "Legion Loyalist", "none", { unread: 1 }),
    person("nia-storm", "Nia Storm", "Goblin Guide", "none", {
      inThreads: false,
    }),
    person("corin", "Corin", "Goblin Piledriver", "none", { inThreads: false }),
  ],
  events: {
    mira: [
      {
        kind: "message",
        id: "m1",
        isMine: false,
        body: "Would you trade these?",
        sentAt: at(14),
      },
      { kind: "trade", id: "t1", state: "open", sentAt: at(14) },
      {
        kind: "message",
        id: "m2",
        isMine: true,
        body: "Let me take a look.",
        sentAt: at(19),
      },
      {
        kind: "message",
        id: "m3",
        isMine: false,
        body: "Want to play while you decide?",
        sentAt: at(20),
      },
      {
        kind: "game",
        id: "g1",
        detail: "Modern · 2v2",
        state: "open",
        sentAt: at(20),
      },
    ],
    elowen: [
      {
        kind: "message",
        id: "e1",
        isMine: false,
        body: "Good luck in the league tonight!",
        sentAt: at(2),
      },
    ],
    nia: [
      {
        kind: "message",
        id: "n1",
        isMine: false,
        body: "Good game. Want a rematch?",
        sentAt: at(31),
      },
    ],
  },
  drafts: {},
};
const demoAtom = atom<DemoState>(SEED);
const pause = (ms = 450): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

export function useDemoMessaging(personId: string | null): MessagingSource {
  const [state, setState] = useAtom(demoAtom);
  const { catalogue } = useBuilderCatalogue();
  const [query, setQuery] = useState("");
  const [searchState, setSearchState] = useState<
    "idle" | "searching" | "ready"
  >("idle");
  const [delivery, setDelivery] = useState<Delivery>({ state: "idle" });
  useEffect(() => setDelivery({ state: "idle" }), [personId]);
  // Search waits a moment, as a real read would.
  const needle = query.trim().toLowerCase();
  useEffect(() => {
    if (!needle) return setSearchState("idle");
    setSearchState("searching");
    const handle = window.setTimeout(() => setSearchState("ready"), 350);
    return () => window.clearTimeout(handle);
  }, [needle]);

  const view = useCallback(
    (demo: DemoPerson): MessagingPerson => {
      const card = demo.art ? catalogue?.find(demo.art) : undefined;
      const last = [...(state.events[demo.id] ?? [])]
        .reverse()
        .find((event) => event.kind === "message");
      return {
        id: demo.id,
        name: demo.name,
        portrait: card ? { artUrl: card.artUrl, crop: CENTRED } : null,
        relationship: demo.relationship,
        isBlocked: demo.isBlocked,
        canContact: !demo.isBlocked && !demo.isDeleted,
        isDeleted: demo.isDeleted,
        unread: demo.unread,
        preview:
          demo.relationship === "incoming"
            ? "Friend request"
            : demo.relationship === "outgoing"
              ? "Request sent"
              : last?.kind === "message"
                ? last.body
                : null,
      };
    },
    [catalogue, state.events],
  );
  const people = useMemo(() => state.people.map(view), [state.people, view]);
  const open = people.find((candidate) => candidate.id === personId) ?? null;
  // Opening a thread reads it.
  useEffect(() => {
    if (open && open.unread > 0)
      setState((current) => ({
        ...current,
        people: current.people.map((p) =>
          p.id === open.id ? { ...p, unread: 0 } : p,
        ),
      }));
  }, [open, setState]);

  const update = (id: string, change: Partial<DemoPerson>) =>
    setState((current) => ({
      ...current,
      people: current.people.map((p) =>
        p.id === id ? { ...p, ...change, inThreads: true } : p,
      ),
    }));
  const act = async (id: string, action: PersonAction): Promise<Outcome> => {
    const found = state.people.find((p) => p.id === id);
    if (!found) return failed("Couldn’t update this player.");
    await pause();
    const next: Partial<DemoPerson> =
      action === "add"
        ? { relationship: "outgoing" }
        : action === "accept"
          ? { relationship: "friend" }
          : action === "cancel" || action === "decline" || action === "remove"
            ? { relationship: "none" }
            : action === "block"
              ? { isBlocked: true }
              : { isBlocked: false };
    update(id, next);
    return succeeded(undefined);
  };
  const draft = personId ? (state.drafts[personId] ?? "") : "";
  const setDraft = (text: string) => {
    if (!personId) return;
    setState((current) => ({
      ...current,
      drafts: { ...current.drafts, [personId]: text },
    }));
  };

  return {
    status: "ready",
    friends: people.filter((p) => p.relationship === "friend"),
    requests: people.filter(
      (p) =>
        p.relationship !== "friend" &&
        (p.relationship !== "none" || (state.events[p.id]?.length ?? 0) > 0),
    ),
    attention: people.filter(
      (p) =>
        p.relationship !== "friend" &&
        (p.relationship === "incoming" || p.unread > 0),
    ).length,
    failure: null,
    dismissFailure: () => undefined,
    thread: open
      ? {
          person: open,
          state: "ready",
          events: state.events[open.id] ?? [],
          hasOlder: false,
          loadOlder: () => undefined,
        }
      : null,
    search: {
      query,
      setQuery,
      state: searchState,
      results:
        searchState === "ready"
          ? people.filter((p) => p.name.toLowerCase().includes(needle))
          : [],
    },
    draft,
    setDraft,
    delivery,
    send: () => {
      if (
        !open ||
        !canSend(draft) ||
        !open.canContact ||
        delivery.state === "sending"
      )
        return;
      const sent = draft;
      setDelivery({ state: "sending" });
      void pause(400).then(() => {
        setState((current) => ({
          ...current,
          events: {
            ...current.events,
            [open.id]: [
              ...(current.events[open.id] ?? []),
              {
                kind: "message",
                id: `sent-${Date.now()}`,
                isMine: true,
                body: sent,
                sentAt: new Date().toISOString(),
              },
            ],
          },
          // Only the exact text that was sent is cleared; a newer edit stays.
          drafts:
            current.drafts[open.id] === sent
              ? { ...current.drafts, [open.id]: "" }
              : current.drafts,
        }));
        setDelivery({ state: "idle" });
      });
    },
    checkDelivery: () => undefined,
    act,
    respond: async (eventId, answer) => {
      await pause();
      setState((current) => ({
        ...current,
        events: Object.fromEntries(
          Object.entries(current.events).map(([id, events]) => [
            id,
            events.map((event) =>
              event.id === eventId &&
              event.kind !== "message" &&
              answer === "decline"
                ? { ...event, state: "declined" as const }
                : event,
            ),
          ]),
        ),
      }));
      return succeeded(undefined);
    },
  };
}

/** A preview person by id, for screens Messages opens, such as Trade. Null for no such person. */
export function useDemoPerson(id: string): {
  name: string;
  portrait: Portrait | null;
  canContact: boolean;
} | null {
  const [state] = useAtom(demoAtom);
  const { catalogue } = useBuilderCatalogue();
  const found = state.people.find((p) => p.id === id);
  if (!found) return null;
  const card = found.art ? catalogue?.find(found.art) : undefined;
  return {
    name: found.name,
    portrait: card ? { artUrl: card.artUrl, crop: CENTRED } : null,
    canContact: !found.isBlocked && !found.isDeleted,
  };
}
