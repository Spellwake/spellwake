import { useEffect, useState, type ReactElement } from "react";
import { motion, useReducedMotion } from "motion/react";
import { ChevronLeft, Pencil, UsersRound } from "lucide-react";
import {
  ActionButton,
  Dialog,
  EmptyState,
  FeedbackMessages,
} from "@spellwake/ui";
import { DeckBox } from "@spellwake/deck-box";
import {
  canStart,
  RANDOM_DECK,
  rulesLine,
  type LobbySource,
} from "../../lobby/lobby-sources";
import type { Outcome } from "../../decks/deck-sources";
import { useCommand, useNotice } from "../account/operation";
import { DepartureDialog } from "./DepartureDialog";
import { ChooseDeckDialog, InviteDialog } from "./LobbyChoosers";
import { RulesDrawer } from "./RulesDrawer";
import { SeatRow } from "./SeatRow";
import { useArrival } from "../../lib/arrival";
import * as s from "./lobby.css";

const MODEL_URL = `${import.meta.env.BASE_URL}models/deck-box-leather.glb?bend=1`;
const GAP_COPY = {
  closed: { title: "Lobby closed", text: "The host closed this lobby." },
  full: { title: "Lobby full", text: "Ask the host to add a player slot." },
  unavailable: {
    title: "Lobby unavailable",
    text: "Return to your play area.",
  },
  "signed-out": {
    title: "Sign in to play",
    text: "Lobbies belong to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    text: "Choose a username to start playing.",
  },
} as const;

type Chooser = { seatId: string; forAi: boolean } | null;

/**
 * The friend-game lobby, shared by Open Play and Collection Play. Back is the
 * only way out, and it asks first. Seats, rules and readiness come from the
 * source; the screen changes nothing until the source accepts it.
 */
export function LobbyPage({
  source,
  onBack,
}: {
  source: LobbySource;
  /** Where Back, or a closed lobby, returns to. */
  onBack: () => void;
}): ReactElement {
  const [chooser, setChooser] = useState<Chooser>(null);
  const [isInviting, setIsInviting] = useState(false);
  const [isEditingRules, setIsEditingRules] = useState(false);
  const readyCommand = useCommand();
  const notice = useNotice();
  const reduce = useReducedMotion() ?? false;
  const arrival = useArrival(source.status === "ready");

  if (source.status !== "ready")
    return (
      <section className={s.page} aria-label="Lobby">
        {source.status === "loading" ? (
          <p
            className={s.starting}
            role="status"
            style={{ justifyContent: "center", marginTop: "18vh" }}
          >
            <span className={s.spinner} aria-hidden="true" />
            Opening lobby…
          </p>
        ) : (
          <div className={s.gap} role="alert">
            <h1 className={s.gapTitle}>{GAP_COPY[source.status].title}</h1>
            <p className={s.gapText}>{GAP_COPY[source.status].text}</p>
            <ActionButton emphasis="primary" onClick={onBack}>
              Back
            </ActionButton>
          </div>
        )}
      </section>
    );

  const lobby = source;
  const you = lobby.seats.find((seat) => seat.kind === "you");
  const isLocked = lobby.phase !== "open";
  const isCommitting =
    lobby.phase === "starting" || lobby.phase === "start-unconfirmed";
  const filled = lobby.seats.filter((seat) => seat.kind !== "open").length;
  const hasOpenSeat = lobby.seats.some((seat) => seat.kind === "open");
  const chosenSeat = lobby.seats.find((seat) => seat.id === chooser?.seatId);
  const run = (work: () => Promise<Outcome>): void => {
    void readyCommand.run(work).then((outcome) => {
      if (outcome?.status === "fail") notice.say(outcome.message);
    });
  };
  const messages = [
    ...(lobby.notice
      ? [{ id: "lobby", text: lobby.notice, onDismiss: lobby.dismissNotice }]
      : []),
    ...notice.items,
  ];

  return (
    // The whole lobby scrolls as one region on screens too short to hold it.
    <section className={s.page} role="region" aria-labelledby="lobby-title">
      <div className={s.head}>
        <span className={s.back}>
          <ActionButton
            treatment="compact-form"
            icon={<ChevronLeft />}
            disabled={isCommitting}
            onClick={lobby.beginDeparture}
          >
            Back
          </ActionButton>
        </span>
        <div>
          <h1 className={s.title} id="lobby-title">
            Lobby
          </h1>
          <p className={s.subtitle}>{rulesLine(lobby.rules)}</p>
        </div>
      </div>

      <div className={s.layout}>
        <section className={s.players} aria-labelledby="players-title">
          <div className={s.playersHead}>
            <h2 className={s.playersTitle} id="players-title">
              Players
              <span className={s.count}>
                {filled}/{lobby.seats.length}
              </span>
            </h2>
            {lobby.findPlayers && lobby.invite && (
              <ActionButton
                treatment="compact-form"
                icon={<UsersRound />}
                disabled={isLocked || !hasOpenSeat}
                onClick={() => setIsInviting(true)}
              >
                Invite players
              </ActionButton>
            )}
          </div>
          {lobby.aiQuota &&
            lobby.seats.some((seat) => seat.kind === "ai") &&
            (() => {
              const q = lobby.aiQuota;
              const overLimit =
                q.requiredAdvanced > 0 &&
                q.remainingToday !== null &&
                q.requiredAdvanced > q.remainingToday;
              const label = q.displayUnlimited
                ? q.remainingToday === 0
                  ? "Advanced AI limit reached · Resets 00:00 UTC"
                  : "Advanced AI: Unlimited"
                : `Advanced AI: ${q.remainingToday ?? 0}/${q.dailyLimit ?? 0} remaining today · Resets 00:00 UTC`;
              return (
                <p
                  className={`${overLimit ? s.warning : s.passive} ${s.quotaLine}`}
                >
                  {label}
                </p>
              );
            })()}
          <ul className={s.seats} {...arrival.group}>
            {lobby.seats.map((seat) => (
              <SeatRow
                key={seat.id}
                seat={seat}
                isLocked={isLocked}
                onChooseDeck={() =>
                  setChooser({ seatId: seat.id, forAi: false })
                }
                onChooseAiDeck={
                  lobby.setAiDeck
                    ? () => setChooser({ seatId: seat.id, forAi: true })
                    : null
                }
                setController={
                  lobby.setController &&
                  (seat.kind === "open" || seat.kind === "ai")
                    ? (kind) => {
                        const set = lobby.setController;
                        if (set) run(() => set(seat.id, kind));
                      }
                    : null
                }
                setDifficulty={
                  lobby.setDifficulty
                    ? (difficulty) => {
                        const set = lobby.setDifficulty;
                        if (set) run(() => set(seat.id, difficulty));
                      }
                    : null
                }
              />
            ))}
          </ul>
        </section>

        <div className={s.side}>
          <section className={s.deckPanel} aria-label="Your deck">
            <span className={s.deckPanelLabel}>Your deck</span>
            <div>
              {lobby.yourDeck ? (
                <>
                  <h2 className={s.deckName}>{lobby.yourDeck.name}</h2>
                  <p className={s.deckFormat}>{lobby.yourDeck.formatName}</p>
                </>
              ) : (
                <p className={s.noDeck}>No deck chosen</p>
              )}
            </div>
            {lobby.yourDeck && (
              <span className={s.box}>
                <DeckBox
                  name={lobby.yourDeck.name}
                  appearance={lobby.yourDeck.appearance}
                  modelUrl={MODEL_URL}
                  artworkUrl={import.meta.env.BASE_URL}
                  className={s.boxButton}
                  stageClassName={s.boxStage}
                  statusClassName={s.boxStatus}
                  // Pressing the box chooses another deck, as the row's selector does.
                  onInspect={() =>
                    !isLocked &&
                    you &&
                    setChooser({ seatId: you.id, forAi: false })
                  }
                />
              </span>
            )}
          </section>
          <section className={s.rules} aria-label="Game rules">
            <dl className={s.rulesList}>
              <dt>Format</dt>
              <dd>{lobby.rules.format}</dd>
              <dt>Table rules</dt>
              <dd>{lobby.rules.tableRules}</dd>
              <dt>Players</dt>
              <dd>{lobby.rules.players}</dd>
              <dt>Match</dt>
              <dd>{lobby.rules.match}</dd>
              {lobby.rules.planechase !== null && (
                <>
                  <dt>Planechase</dt>
                  <dd>{lobby.rules.planechase ? "On" : "Off"}</dd>
                </>
              )}
              {lobby.rules.influence !== null && (
                <>
                  <dt>Influence</dt>
                  <dd>{lobby.rules.influence}</dd>
                </>
              )}
            </dl>
            {lobby.isHost && lobby.applyRules && lobby.rulesChoices && (
              <div className={s.editRules}>
                <ActionButton
                  treatment="compact-form"
                  icon={<Pencil />}
                  disabled={isLocked}
                  onClick={() => setIsEditingRules(true)}
                >
                  Edit rules
                </ActionButton>
              </div>
            )}
          </section>
        </div>
      </div>

      {/* Toasts float in the gap above the actions, never over them. */}
      <div className={s.notice}>
        <FeedbackMessages
          label="Lobby messages"
          presentation="toast"
          items={messages}
        />
      </div>
      <div className={s.footer}>
        {isCommitting ? (
          <p className={s.starting} role="status">
            <span className={s.spinner} aria-hidden="true" />
            Starting game…
          </p>
        ) : (
          <>
            <ActionButton
              emphasis={you?.isReady ? "neutral" : "primary"}
              disabled={
                !you || (!you.isReady && (!lobby.yourDeck || you.needsNewDeck))
              }
              pending={readyCommand.isBusy}
              onClick={() => run(() => lobby.setReady(!you?.isReady))}
            >
              {you?.isReady ? "Not ready" : "Ready"}
            </ActionButton>
            {lobby.phase === "countdown" && lobby.deadline !== null ? (
              <Countdown deadline={lobby.deadline} reduce={reduce} />
            ) : (
              lobby.start && (
                <ActionButton
                  emphasis={you?.isReady ? "primary" : "neutral"}
                  disabled={!canStart(lobby.seats)}
                  onClick={() => {
                    const start = lobby.start;
                    if (start) run(start);
                  }}
                >
                  Start game
                </ActionButton>
              )
            )}
          </>
        )}
      </div>

      {chooser && chosenSeat && (
        <ChooseDeckDialog
          formatName={lobby.rules.format}
          decks={lobby.decks}
          current={
            chosenSeat.deck === "random"
              ? RANDOM_DECK
              : chosenSeat.deck && typeof chosenSeat.deck === "object"
                ? chosenSeat.deck.id
                : null
          }
          forAi={chooser.forAi}
          onReload={lobby.reloadDecks}
          onSelect={(deckId) =>
            chooser.forAi && lobby.setAiDeck
              ? lobby.setAiDeck(chooser.seatId, deckId)
              : lobby.chooseDeck(deckId)
          }
          onClose={() => setChooser(null)}
        />
      )}
      {isInviting && lobby.findPlayers && lobby.invite && (
        <InviteDialog
          findPlayers={lobby.findPlayers}
          onInvite={lobby.invite}
          onClose={() => setIsInviting(false)}
        />
      )}
      {isEditingRules && lobby.rulesChoices && lobby.applyRules && (
        <RulesDrawer
          rules={lobby.rules}
          choices={lobby.rulesChoices}
          onApply={lobby.applyRules}
          onClose={() => setIsEditingRules(false)}
        />
      )}
      {lobby.departure && (
        <DepartureDialog
          departure={lobby.departure}
          isHost={lobby.isHost}
          onCancel={lobby.cancelDeparture}
          onConfirm={lobby.confirmDeparture}
          onCheck={lobby.checkDeparture}
        />
      )}
      {lobby.phase === "start-unconfirmed" && (
        <Dialog
          open
          title="Start unconfirmed"
          size="compact"
          busy
          onClose={() => undefined}
          actions={
            <ActionButton emphasis="primary" onClick={lobby.checkStart}>
              Check status
            </ActionButton>
          }
        >
          <p>The game start is not confirmed.</p>
        </Dialog>
      )}
      {lobby.phase === "started" && (
        <EmptyState title="Game started" description="Opening the table…" />
      )}
    </section>
  );
}

/** "Starting in N seconds" over a bar that drains to the deadline. */
function Countdown({
  deadline,
  reduce,
}: {
  deadline: number;
  reduce: boolean;
}): ReactElement {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const handle = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(handle);
  }, []);
  const left = Math.max(0, deadline - now);
  const seconds = Math.ceil(left / 1000);
  return (
    <div className={s.countdown} role="timer" aria-live="off">
      <span aria-live="polite">
        Starting in {seconds} {seconds === 1 ? "second" : "seconds"}
      </span>
      <span className={s.bar} aria-hidden="true">
        <motion.span
          className={s.barFill}
          initial={false}
          animate={{ scaleX: left / 10_000 }}
          transition={{ duration: reduce ? 0 : 0.25, ease: "linear" }}
        />
      </span>
    </div>
  );
}
