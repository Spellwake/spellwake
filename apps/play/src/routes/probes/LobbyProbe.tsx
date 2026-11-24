import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { DEFAULT_APPEARANCE } from "@spellwake/deck-box";
import { succeeded } from "../../decks/deck-sources";
import type {
  Departure,
  LobbySeat,
  LobbySource,
} from "../../lobby/lobby-sources";
import { LobbyPage } from "../lobby/LobbyPage";

/**
 * Development only: the lobby in states the preview reaches only by waiting
 * or never, for the stress checks and review captures. `?state=` names one.
 */
const noop = (): void => undefined;
const done = () => Promise.resolve(succeeded(undefined));
const goblins = {
  id: "probe-goblins",
  name: "Goblins",
  formatName: "Standard",
  appearance: DEFAULT_APPEARANCE,
};
const seat = (
  overrides: Partial<LobbySeat> & Pick<LobbySeat, "id" | "kind" | "name">,
): LobbySeat => ({
  portrait: null,
  isHost: false,
  isReady: false,
  deck: null,
  needsNewDeck: false,
  difficulty: null,
  ...overrides,
});
type Ready = Extract<LobbySource, { status: "ready" }>;
const host: Ready = {
  status: "ready",
  mode: "open",
  rules: {
    format: "Standard",
    tableRules: "Free-for-All",
    players: 4,
    match: "Single game",
    planechase: false,
    influence: "Unlimited",
  },
  seats: [
    seat({
      id: "you",
      kind: "you",
      name: "You",
      isHost: true,
      isReady: true,
      deck: goblins,
    }),
    seat({ id: "mira", kind: "person", name: "Mira", deck: "selected" }),
    seat({
      id: "ai",
      kind: "ai",
      name: "AI player",
      isReady: true,
      deck: "random",
      difficulty: "normal",
    }),
    seat({ id: "sol", kind: "person", name: "Sol", deck: "selected" }),
  ],
  isHost: true,
  yourDeck: goblins,
  phase: "open",
  deadline: null,
  notice: null,
  dismissNotice: noop,
  decks: { state: "ready", items: [goblins] },
  reloadDecks: noop,
  chooseDeck: done,
  setReady: done,
  start: done,
  checkStart: noop,
  setController: done,
  setDifficulty: done,
  setAiDeck: done,
  rulesChoices: {
    formats: ["Standard", "Modern", "Commander"],
    tableRules: ["Free-for-All", "Two-Headed Giant", "Emperor"],
    players: [2, 3, 4, 5, 6],
    matches: ["Single game", "Best of three"],
    planechase: true,
    influences: ["Unlimited", "1", "2"],
  },
  applyRules: done,
  findPlayers: () => Promise.resolve([]),
  invite: done,
  inviteLink: null,
  departure: null,
  beginDeparture: noop,
  cancelDeparture: noop,
  confirmDeparture: noop,
  checkDeparture: noop,
};
/** Mira's view of the same room: she chooses her deck and readies, nothing else. */
const member: Ready = {
  ...host,
  isHost: false,
  seats: [
    seat({
      id: "host",
      kind: "person",
      name: "Planeswalker",
      isHost: true,
      deck: "selected",
    }),
    seat({
      id: "you",
      kind: "you",
      name: "You · Mira",
      deck: { ...goblins, name: "Angels" },
    }),
    seat({
      id: "ai",
      kind: "ai",
      name: "AI player",
      isReady: true,
      deck: "random",
      difficulty: "normal",
    }),
    seat({ id: "open", kind: "open", name: "Open seat" }),
  ],
  yourDeck: { ...goblins, name: "Angels" },
  start: null,
  setController: null,
  setDifficulty: null,
  setAiDeck: null,
  rulesChoices: null,
  applyRules: null,
  findPlayers: null,
  invite: null,
};
const departing = (departure: Departure): Ready => ({ ...host, departure });

export function LobbyProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "host";
  const source: LobbySource =
    state === "member"
      ? member
      : state === "member-no-deck"
        ? {
            ...member,
            yourDeck: null,
            seats: member.seats.map((s) =>
              s.kind === "you" ? { ...s, deck: null } : s,
            ),
          }
        : state === "countdown"
          ? { ...host, phase: "countdown", deadline: Date.now() + 9_000 }
          : state === "starting"
            ? { ...host, phase: "starting" }
            : state === "start-unconfirmed"
              ? { ...host, phase: "start-unconfirmed" }
              : state === "needs-deck"
                ? {
                    ...host,
                    rules: { ...host.rules, format: "Modern" },
                    seats: host.seats.map((s) =>
                      s.kind === "you" || s.kind === "person"
                        ? { ...s, isReady: false, needsNewDeck: true }
                        : s,
                    ),
                  }
                : state === "invited"
                  ? { ...host, notice: "Invitation sent to Sol" }
                  : state === "invite-link"
                    ? {
                        ...host,
                        seats: host.seats.map((s) =>
                          s.name === "Sol"
                            ? seat({
                                id: "open",
                                kind: "open",
                                name: "Open seat",
                              })
                            : s,
                        ),
                        findPlayers: null,
                        invite: null,
                        inviteLink: () =>
                          Promise.resolve(
                            succeeded(
                              "https://app.spellwake.com/open-play/lobby?room=0b8f6c1e-2a4d-4e7b-9c3a-5d6e7f809a1b#invite=preview",
                            ),
                          ),
                      }
                    : state === "closing"
                      ? departing({ stage: "pending" })
                      : state === "closure-unconfirmed"
                        ? departing({ stage: "unconfirmed", isChecking: false })
                        : state === "not-closed"
                          ? departing({ stage: "rejected" })
                          : state === "close"
                            ? departing({ stage: "confirm" })
                            : state === "closed" ||
                                state === "full" ||
                                state === "unavailable" ||
                                state === "loading"
                              ? { status: state }
                              : host;
  return <LobbyPage source={source} onBack={noop} />;
}
