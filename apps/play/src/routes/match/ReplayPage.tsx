import { useEffect, useState, type ReactElement } from "react";
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  CircleUserRound,
  Pause,
  Play,
  SkipBack,
  SkipForward,
} from "lucide-react";
import {
  designPixels,
  EmptyState,
  LoadingShape,
  LoadingState,
  PlayerIdentity,
  RadioGroup,
  SettingSwitch,
  TurnPhaseDisplay,
  type MatchPhase,
} from "@spellwake/ui";
import { ImmersiveFrame } from "../../components/shell/ImmersiveFrame";
import * as frame from "../../components/shell/frames.css";
import type { ReplaySource } from "../../match/replay-sources";
import type { TableView } from "../../match/table-model";
import { MatchTable } from "./MatchTable";
import { PHASE_MAP, useLowerDock } from "./MatchPage";
import * as s from "./replay.css";

const ICON = designPixels(26);
/** How long each step shows while the replay plays. */
const PLAY_STEP_MS = 1100;
const noop = (): void => undefined;

type Ready = Extract<ReplaySource, { status: "ready" }>;

function Controls({ source }: { source: Ready }): ReactElement {
  const [playing, setPlaying] = useState(false);
  const { step, steps, go } = source;
  const last = Math.max(0, steps - 1);
  // Playing moves on a step at a time and stops at the end.
  useEffect(() => {
    if (!playing) return;
    if (step >= last) {
      setPlaying(false);
      return;
    }
    const timer = window.setTimeout(() => go(step + 1), PLAY_STEP_MS);
    return () => window.clearTimeout(timer);
  }, [playing, step, last, go]);
  const turn = source.snapshot?.turn ?? null;
  const phase = turn
    ? PHASE_MAP[turn.phase]
    : { phase: "beginning" as MatchPhase, label: "Opening" };
  const active = turn?.activeSeatIds[0];
  const turnLabel = turn
    ? `Turn ${turn.number} · ${source.seatNames.get(active ?? "") ?? "Player"}`
    : "Opening";
  const reading = steps === 0;
  return (
    <section className={s.panel} aria-label="Replay">
      <p className={s.title}>
        <span className={s.kicker}>Replay</span> {source.title}
      </p>
      {source.games.length > 1 && (
        <RadioGroup
          label="Game"
          treatment="chips"
          value={String(source.gameIndex)}
          options={source.games.map((game, index) => ({
            value: String(index),
            label: game.label,
          }))}
          onValueChange={(value) => {
            setPlaying(false);
            source.chooseGame(Number(value));
          }}
        />
      )}
      {source.snapshot && (
        <TurnPhaseDisplay
          turnLabel={turnLabel}
          phase={phase.phase}
          phaseLabel={phase.label}
        />
      )}
      <input
        className={s.scrubber}
        type="range"
        min={0}
        max={last}
        value={step}
        disabled={reading}
        aria-label="Step"
        aria-valuetext={`Step ${step + 1} of ${steps}`}
        onChange={(event) => {
          setPlaying(false);
          go(Number(event.currentTarget.value));
        }}
      />
      <div className={s.buttons}>
        <button
          type="button"
          className={s.button}
          aria-label="Previous turn"
          disabled={reading || step === 0}
          onClick={() => {
            setPlaying(false);
            source.previousTurn();
          }}
        >
          <SkipBack aria-hidden="true" />
        </button>
        <button
          type="button"
          className={s.button}
          aria-label="Previous step"
          disabled={reading || step === 0}
          onClick={() => {
            setPlaying(false);
            go(step - 1);
          }}
        >
          <ChevronLeft aria-hidden="true" />
        </button>
        <button
          type="button"
          className={s.play}
          aria-label={playing ? "Pause" : "Play"}
          aria-pressed={playing}
          disabled={reading || (!playing && step >= last)}
          onClick={() => setPlaying((on) => !on)}
        >
          {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
        </button>
        <button
          type="button"
          className={s.button}
          aria-label="Next step"
          disabled={reading || step >= last}
          onClick={() => {
            setPlaying(false);
            go(step + 1);
          }}
        >
          <ChevronRight aria-hidden="true" />
        </button>
        <button
          type="button"
          className={s.button}
          aria-label="Next turn"
          disabled={reading || step >= last}
          onClick={() => {
            setPlaying(false);
            source.nextTurn();
          }}
        >
          <SkipForward aria-hidden="true" />
        </button>
      </div>
      <p className={s.count} aria-hidden="true">
        {reading ? "Reading the game…" : `Step ${step + 1} of ${steps}`}
      </p>
      {source.everyHand !== null && (
        <div className={s.setting}>
          <span>Show every hand</span>
          <SettingSwitch
            label="Show every hand"
            checked={source.everyHand}
            onCheckedChange={source.setEveryHand}
          />
        </div>
      )}
      {source.notice && (
        <p className={s.notice} role="status">
          {source.notice}
        </p>
      )}
    </section>
  );
}

function ReadyReplay({
  source,
  onBack,
}: {
  source: Ready;
  onBack: () => void;
}): ReactElement {
  const dock = useLowerDock();
  const snapshot = source.snapshot;
  const [view, setView] = useState<TableView | null>(null);
  const viewer = snapshot?.viewerSeatId ?? null;
  const shownView: TableView | null = snapshot
    ? view &&
      (view.kind === "overview" ||
        snapshot.players.some((p) => p.seatId === view.seatId))
      ? view
      : { kind: "seat", seatId: snapshot.viewerSeatId }
    : null;
  return (
    <ImmersiveFrame
      playersLabel="Players"
      trayHeight={dock.trayHeight}
      leading={
        <button
          type="button"
          className={frame.hudButton}
          aria-label="Back to history"
          onClick={onBack}
        >
          <ArrowLeft size={ICON} aria-hidden="true" />
        </button>
      }
      players={snapshot?.players.map((player) => {
        const name = source.seatNames.get(player.seatId) ?? "Player";
        return (
          <PlayerIdentity
            key={player.seatId}
            presentation="seat"
            name={name}
            {...(player.eliminated ? {} : { life: Number(player.life) || 0 })}
            actionLabel={
              player.seatId === viewer
                ? "View your board"
                : `View ${name}’s board`
            }
            onActivate={() => setView({ kind: "seat", seatId: player.seatId })}
            takingTurn={
              snapshot.turn?.activeSeatIds.includes(player.seatId) ?? false
            }
            viewingOpponent={
              shownView?.kind === "seat" &&
              shownView.seatId === player.seatId &&
              player.seatId !== viewer
            }
            portrait={<CircleUserRound aria-hidden="true" />}
          />
        );
      })}
      side={<Controls source={source} />}
    >
      {snapshot && shownView ? (
        <MatchTable
          // Nothing on a replayed table can be played.
          snapshot={{
            ...snapshot,
            decisions: [],
            turn: snapshot.turn && { ...snapshot.turn, prioritySeatIds: [] },
          }}
          view={shownView}
          seatNames={source.seatNames}
          faceOf={source.faceOf}
          clearBottom={dock.clearBottom}
          onCard={noop}
          onPile={noop}
        />
      ) : (
        <div className={s.reading}>
          <LoadingState label="Reading the game…">
            <LoadingShape width={260} height={36} />
          </LoadingState>
        </div>
      )}
    </ImmersiveFrame>
  );
}

/**
 * A finished match, replayed on the table: every step of a game from the
 * player's own seat (or with every hand, once the game allows it), with the
 * steps to move through where the turn's actions were.
 */
export function ReplayPage({
  source,
  onBack,
}: {
  source: ReplaySource;
  onBack: () => void;
}): ReactElement {
  if (source.status === "ready")
    return <ReadyReplay source={source} onBack={onBack} />;
  if (source.status === "loading")
    return (
      <section className={s.empty} aria-label="Loading replay">
        <LoadingState label="Reading the match…">
          <LoadingShape width={260} height={36} />
        </LoadingState>
      </section>
    );
  return (
    <section className={s.empty} aria-label="Replay unavailable">
      <EmptyState
        title="Replay unavailable"
        description={
          source.status === "missing"
            ? "This match has no game to replay."
            : "Sign in to replay your matches."
        }
      />
    </section>
  );
}
