import { useRef, type ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { Dialog } from "@spellwake/ui";
import { succeeded } from "../../decks/deck-sources";
import { useDeviceClass } from "../../lib/display-scale";
import type {
  MessagingPerson,
  MessagingSource,
} from "../../messaging/messaging-sources";
import { MessagesPanel, type MessagesView } from "../messages/MessagesPanel";

/**
 * Development only: Messages in states the preview cannot reach on its own,
 * for the stress checks and review captures. `?state=` names one.
 */
const noop = (): void => undefined;
const someone = (
  id: string,
  name: string,
  extra: Partial<MessagingPerson> = {},
): MessagingPerson => ({
  id,
  name,
  portrait: null,
  relationship: "friend",
  isBlocked: false,
  canContact: true,
  isDeleted: false,
  unread: 0,
  preview: null,
  ...extra,
});
const mira = someone("mira", "Mira");
const ready = (
  overrides: Partial<Extract<MessagingSource, { status: "ready" }>>,
): MessagingSource => ({
  status: "ready",
  friends: [mira, someone("elowen", "Elowen")],
  requests: [],
  attention: 0,
  failure: null,
  dismissFailure: noop,
  thread: {
    person: mira,
    state: "ready",
    events: [
      {
        kind: "message",
        id: "1",
        isMine: false,
        body: "Want to play later?",
        sentAt: "2026-11-21T14:20:00Z",
      },
      {
        kind: "game",
        id: "2",
        detail: "Standard · Free-for-All",
        state: "declined",
        sentAt: "2026-11-21T14:21:00Z",
      },
      {
        kind: "trade",
        id: "3",
        state: "expired",
        sentAt: "2026-11-21T14:22:00Z",
      },
    ],
    hasOlder: false,
    loadOlder: noop,
  },
  search: { query: "", setQuery: noop, state: "idle", results: [] },
  draft: "",
  setDraft: noop,
  delivery: { state: "idle" },
  send: noop,
  checkDelivery: noop,
  act: () => Promise.resolve(succeeded(undefined)),
  respond: () => Promise.resolve(succeeded(undefined)),
  ...overrides,
});
const withPerson = (person: MessagingPerson) =>
  ready({
    thread: {
      person,
      state: "ready",
      events: [],
      hasOlder: false,
      loadOlder: noop,
    },
  });

export function MessagesProbe(): ReactElement {
  const [params] = useSearchParams();
  const trigger = useRef<HTMLButtonElement>(null);
  const state = params.get("state") ?? "unconfirmed";
  const isPhone = useDeviceClass() === "phone";
  const source: MessagingSource =
    state === "loading"
      ? { status: "loading" }
      : state === "unconfirmed"
        ? ready({
            draft: "Ready when you are.",
            delivery: { state: "unconfirmed", isChecking: false },
          })
        : state === "resolved"
          ? ready({})
          : state === "no-contact"
            ? withPerson(
                someone("rowan", "Rowan", {
                  relationship: "none",
                  canContact: false,
                }),
              )
            : state === "deleted"
              ? withPerson(
                  someone("gone", "Deleted player", {
                    relationship: "none",
                    canContact: false,
                    isDeleted: true,
                  }),
                )
              : state === "thread-loading"
                ? ready({
                    thread: {
                      person: mira,
                      state: "loading",
                      events: [],
                      hasOlder: false,
                      loadOlder: noop,
                    },
                  })
                : ready({ requests: [] });
  const view: MessagesView =
    state === "no-requests"
      ? { kind: "requests" }
      : { kind: "person", id: "mira" };
  const panel = (
    <section aria-label="Messages probe">
      <button ref={trigger} type="button" hidden>
        Messages
      </button>
      <MessagesPanel
        source={source}
        view={view}
        onView={noop}
        onClose={noop}
        onTrade={noop}
        onJoin={noop}
        presentation={isPhone ? "sheet" : "panel"}
      />
    </section>
  );
  return isPhone ? (
    <Dialog open title="Messages" chrome="bare" onClose={noop}>
      {panel}
    </Dialog>
  ) : (
    panel
  );
}
