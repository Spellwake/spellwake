import { useEffect, useRef, useState, type ReactElement } from "react";
import { Check, Circle, CircleCheck, Sparkle } from "lucide-react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  TextField,
} from "@spellwake/ui";
import { Portrait } from "../../account/Portrait";
import type { Outcome } from "../../decks/deck-sources";
import {
  RANDOM_DECK,
  type InviteCandidate,
  type LobbyDeck,
} from "../../lobby/lobby-sources";
import { useCommand, useNotice } from "../account/operation";
import * as s from "./lobby.css";

/** The deck's face card art, where it has one. */
const thumbFor = (deck: LobbyDeck): string | null => deck.appearance.faceArt;

/**
 * Choose Deck, for your seat or an AI's: one dialog and one geometry, as
 * approved. The AI's adds only Random Deck. A row is a pending choice;
 * Select sends it once, and Cancel, X or Escape discard it.
 */
export function ChooseDeckDialog({
  formatName,
  decks,
  current,
  forAi,
  onReload,
  onSelect,
  onClose,
}: {
  formatName: string;
  decks: { state: "loading" | "ready" | "failed"; items: readonly LobbyDeck[] };
  /** The deck chosen now: a deck id, RANDOM_DECK, or null. */
  current: string | null;
  forAi: boolean;
  onReload: () => void;
  onSelect: (deckId: string) => Promise<Outcome>;
  onClose: () => void;
}): ReactElement {
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<string | null>(current);
  const list = useRef<HTMLDivElement>(null);
  const command = useCommand();
  const notice = useNotice();
  const needle = query.trim().toLowerCase();
  const shown = decks.items.filter((deck) =>
    deck.name.toLowerCase().includes(needle),
  );
  const showRandom = forAi && "random deck".includes(needle);
  // A new search starts the list from its top.
  useEffect(() => {
    list.current?.scrollTo(0, 0);
  }, [needle]);
  const select = (): void => {
    if (!pending || pending === current) return onClose();
    void command
      .run(() => onSelect(pending))
      .then((outcome) => {
        if (outcome?.status === "success") onClose();
        else if (outcome) notice.say(outcome.message);
      });
  };
  const row = (
    id: string,
    name: string,
    thumb: ReactElement,
    subtitle = formatName,
  ): ReactElement => (
    <button
      key={id}
      type="button"
      role="radio"
      aria-checked={pending === id}
      className={s.option}
      onClick={() => setPending(id)}
    >
      {thumb}
      <span className={s.optionDivider} />
      <span className={s.optionText}>
        {name}
        <span className={s.optionSub}>{subtitle}</span>
      </span>
      {pending === id ? (
        <CircleCheck className={s.radio} aria-hidden="true" />
      ) : (
        <Circle className={s.radio} aria-hidden="true" />
      )}
    </button>
  );
  return (
    <Dialog
      open
      title="Choose Deck"
      subtitle={formatName}
      className={s.chooser}
      busy={command.isBusy}
      onClose={onClose}
      actions={
        <>
          <ActionButton disabled={command.isBusy} onClick={onClose}>
            Cancel
          </ActionButton>
          <ActionButton
            emphasis="primary"
            disabled={!pending}
            pending={command.isBusy}
            onClick={select}
          >
            Select
          </ActionButton>
        </>
      }
    >
      <TextField
        type="search"
        label="Search decks"
        placeholder="Search decks…"
        value={query}
        onValueChange={setQuery}
        hideLabel
        treatment="compact-form"
        data-autofocus=""
      />
      <div
        ref={list}
        className={s.list}
        role="radiogroup"
        aria-label="Decks"
        aria-busy={decks.state === "loading" || undefined}
      >
        {decks.state === "loading" ? (
          <p className={s.listState} role="status">
            <span className={s.spinner} aria-hidden="true" />
            Loading decks…
          </p>
        ) : decks.state === "failed" ? (
          <div className={s.listState}>
            Couldn’t load your decks.
            <ActionButton treatment="compact-form" onClick={onReload}>
              Try again
            </ActionButton>
          </div>
        ) : shown.length === 0 && !showRandom ? (
          <p className={s.listState}>
            {decks.items.length === 0
              ? "No eligible decks"
              : "No matching decks"}
            <span className={s.listQuiet}>
              {decks.items.length === 0
                ? `Build a ${formatName} deck to play it here.`
                : "Try another name."}
            </span>
          </p>
        ) : (
          <>
            {showRandom &&
              row(
                RANDOM_DECK,
                "Random Deck",
                <span className={s.randomThumb} aria-hidden="true">
                  <Sparkle fill="currentColor" strokeWidth={1} />
                </span>,
              )}
            {shown.map((deck) => {
              const url = thumbFor(deck);
              const tags = [
                formatName,
                ...(deck.lentByHost ? ["Lent by host"] : []),
                ...(deck.hasCustomCards ? ["Custom cards"] : []),
              ];
              return row(
                deck.id,
                deck.name,
                url ? (
                  <img className={s.thumb} src={url} alt="" />
                ) : (
                  <span className={s.randomThumb} aria-hidden="true" />
                ),
                tags.join(" · "),
              );
            })}
          </>
        )}
      </div>
      <FeedbackMessages
        label="Choose Deck messages"
        presentation="inline"
        items={notice.items}
      />
    </Dialog>
  );
}

/**
 * Invite players: search for one person and send them an invitation. It
 * reserves no seat. On success the dialog closes and the lobby says so.
 */
export function InviteDialog({
  findPlayers,
  onInvite,
  onClose,
}: {
  findPlayers: (query: string) => Promise<readonly InviteCandidate[] | null>;
  onInvite: (playerId: string) => Promise<Outcome>;
  onClose: () => void;
}): ReactElement {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<
    readonly InviteCandidate[] | "searching" | "failed"
  >("searching");
  const [chosen, setChosen] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const command = useCommand();
  const notice = useNotice();
  useEffect(() => {
    let isCurrent = true;
    setResults("searching");
    const handle = window.setTimeout(() => {
      void findPlayers(query).then((found) => {
        if (isCurrent) setResults(found ?? "failed");
      });
    }, 250);
    return () => {
      isCurrent = false;
      window.clearTimeout(handle);
    };
  }, [query, findPlayers, attempt]);
  const list = Array.isArray(results) ? results : [];
  const invite = (): void => {
    if (!chosen) return;
    void command
      .run(() => onInvite(chosen))
      .then((outcome) => {
        if (outcome?.status === "success") onClose();
        else if (outcome) notice.say(outcome.message);
      });
  };
  return (
    <Dialog
      open
      title="Invite players"
      className={s.chooser}
      busy={command.isBusy}
      onClose={onClose}
      actions={
        <>
          <ActionButton disabled={command.isBusy} onClick={onClose}>
            Cancel
          </ActionButton>
          <ActionButton
            emphasis="primary"
            disabled={!chosen || !list.some((person) => person.id === chosen)}
            pending={command.isBusy}
            onClick={invite}
          >
            Invite
          </ActionButton>
        </>
      }
    >
      <TextField
        type="search"
        label="Search players"
        placeholder="Search players…"
        value={query}
        onValueChange={setQuery}
        hideLabel
        treatment="compact-form"
        data-autofocus=""
      />
      <div className={s.list} role="radiogroup" aria-label="Players">
        {results === "searching" ? (
          <p className={s.listState} role="status">
            <span className={s.spinner} aria-hidden="true" />
            Searching players…
          </p>
        ) : results === "failed" ? (
          <div className={s.listState}>
            Players couldn’t be loaded.
            <ActionButton
              treatment="compact-form"
              onClick={() => setAttempt((n) => n + 1)}
            >
              Try again
            </ActionButton>
          </div>
        ) : list.length === 0 ? (
          <p className={s.listState}>
            No players found
            <span className={s.listQuiet}>Try another name.</span>
          </p>
        ) : (
          list.map((person) => (
            <button
              key={person.id}
              type="button"
              role="radio"
              aria-checked={chosen === person.id}
              className={s.person}
              onClick={() => setChosen(person.id)}
            >
              <Portrait portrait={person.portrait} diameter={72} />
              <span className={s.optionText}>{person.name}</span>
              <span className={s.tick} aria-hidden="true">
                {chosen === person.id && <Check size="70%" />}
              </span>
            </button>
          ))
        )}
      </div>
      <FeedbackMessages
        label="Invite messages"
        presentation="inline"
        items={notice.items}
      />
    </Dialog>
  );
}
