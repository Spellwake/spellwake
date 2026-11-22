import type { Portrait } from "../account/account-sources";
import type { Outcome } from "../decks/deck-sources";

/**
 * Messages as the panel reads them, from the account or the local preview.
 * One thread per person. Friends sit on the rail; everyone else with a
 * pending request or a retained conversation sits under Requests.
 */
export type Relationship = "friend" | "incoming" | "outgoing" | "none";
export type MessagingPerson = {
  id: string;
  name: string;
  portrait: Portrait | null;
  relationship: Relationship;
  isBlocked: boolean;
  /** Whether a message can be sent now. History stays readable either way. */
  canContact: boolean;
  isDeleted: boolean;
  unread: number;
  /** The latest line, for the Requests list, or null. */
  preview: string | null;
};

export type ThreadEvent =
  | {
      kind: "message";
      id: string;
      isMine: boolean;
      body: string;
      sentAt: string;
    }
  | {
      kind: "trade";
      id: string;
      state: "open" | "declined" | "expired";
      sentAt: string;
    }
  | {
      kind: "game";
      id: string;
      /** "Modern · 2v2" */
      detail: string;
      state: "open" | "declined" | "expired";
      sentAt: string;
    };

/** Where the last send stands. Only "unconfirmed" offers Check delivery. */
export type Delivery =
  | { state: "idle" }
  | { state: "sending" }
  | { state: "unconfirmed"; isChecking: boolean }
  | { state: "rejected"; message: string };

export type PersonAction =
  "add" | "cancel" | "accept" | "decline" | "remove" | "block" | "unblock";

export type MessagingGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";
export type MessagingSource =
  | { status: MessagingGap }
  | {
      status: "ready";
      friends: readonly MessagingPerson[];
      /** Incoming and outgoing requests and retained conversations with non-friends. */
      requests: readonly MessagingPerson[];
      /** People needing attention: an incoming request or unread messages. */
      attention: number;
      /** A read that failed after its retries, said once. */
      failure: string | null;
      dismissFailure: () => void;
      /** The open person's thread, or null. */
      thread: {
        person: MessagingPerson;
        state: "loading" | "ready" | "failed";
        events: readonly ThreadEvent[];
        hasOlder: boolean;
        loadOlder: () => void;
      } | null;
      search: {
        query: string;
        setQuery: (query: string) => void;
        state: "idle" | "searching" | "ready" | "failed";
        results: readonly MessagingPerson[];
      };
      /** The open person's draft, kept per person. */
      draft: string;
      setDraft: (text: string) => void;
      delivery: Delivery;
      send: () => void;
      checkDelivery: () => void;
      act: (personId: string, action: PersonAction) => Promise<Outcome>;
      /** Answers a trade or game card, or null where none can be answered. */
      respond:
        | ((eventId: string, answer: "accept" | "decline") => Promise<Outcome>)
        | null;
    };

/** Production messages are limited to 4000 Unicode scalar values. */
export const MESSAGE_LIMIT = 4000;
export const canSend = (text: string): boolean =>
  text.trim().length > 0 && Array.from(text).length <= MESSAGE_LIMIT;

/** The person-menu actions for a relationship, as approved. */
export function personMenu(person: MessagingPerson): PersonAction[] {
  if (person.isDeleted) return [];
  if (person.isBlocked)
    return person.relationship === "friend"
      ? ["remove", "unblock"]
      : ["unblock"];
  if (person.relationship === "friend") return ["remove", "block"];
  if (person.relationship === "outgoing") return ["cancel", "block"];
  if (person.relationship === "incoming") return ["block"];
  return ["add", "block"];
}
export const ACTION_LABEL: Record<PersonAction, string> = {
  add: "Add friend",
  cancel: "Cancel friend request",
  accept: "Accept",
  decline: "Decline",
  remove: "Remove friend",
  block: "Block",
  unblock: "Unblock",
};
/** "Friend", "Friend request", "Request sent", "Player" or "Blocked". */
export function relationshipLabel(person: MessagingPerson): string {
  if (person.isDeleted) return "Deleted player";
  if (person.isBlocked) return "Blocked";
  return {
    friend: "Friend",
    incoming: "Friend request",
    outgoing: "Request sent",
    none: "Player",
  }[person.relationship];
}
