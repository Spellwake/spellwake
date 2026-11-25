import { useMemo, useState } from "react";
import type { PlayerSnapshot } from "@spellwake/application-contracts";
import { useDemoMatch } from "./demo-match";
import { previewScene, previewSnapshot } from "./preview-table";
import type { ReplaySource } from "./replay-sources";
import { nextTurn, previousTurn, type Timeline } from "./replay";

const noop = (): void => undefined;

/** A turn of the preview table, renumbered so the steps read as one game. */
function atTurn(snapshot: PlayerSnapshot, number: number): PlayerSnapshot {
  return snapshot.turn
    ? { ...snapshot, turn: { ...snapshot.turn, number: String(number) } }
    : snapshot;
}

/** The preview's replay: the preview table's scenes, in order, as two turns and a third. */
function previewFrames(): PlayerSnapshot[] {
  return [
    atTurn(previewSnapshot(), 4),
    atTurn(previewScene("stack"), 4),
    atTurn(previewScene("stack-long"), 4),
    atTurn(previewScene("attack"), 5),
    atTurn(previewScene("block"), 5),
    atTurn(previewScene("damage"), 5),
    atTurn(previewScene("trample"), 5),
    atTurn(previewSnapshot(), 6),
    atTurn(previewScene("stack-waiting"), 6),
  ];
}

/** The preview's stand-in for a replay: no account and no server. `reading` is still reading its steps. */
export function useDemoReplay(
  state: "steps" | "reading" = "steps",
): ReplaySource {
  const table = useDemoMatch(noop);
  const frames = useMemo(previewFrames, []);
  const line = useMemo<Timeline>(
    () => ({
      steps: frames.map((_, index) => String(index + 1)),
      turnStarts: frames.flatMap((frame, index) =>
        index === 0 || frames[index - 1]?.turn?.number !== frame.turn?.number
          ? [index]
          : [],
      ),
    }),
    [frames],
  );
  const [step, setStep] = useState(0);
  const [game, setGame] = useState(0);
  const [everyHand, setEveryHand] = useState(false);
  if (table.status !== "ready")
    return { status: table.status === "idle" ? "loading" : table.status };
  const faceByName = table.faceByName;
  return {
    status: "ready",
    title: "vs Alex · Modern",
    games: [
      { id: "preview-game-1", label: "Game 1" },
      { id: "preview-game-2", label: "Game 2" },
      { id: "preview-game-3", label: "Game 3" },
    ],
    gameIndex: game,
    chooseGame: (index) => {
      setGame(index);
      setStep(0);
    },
    seatNames: table.seatNames,
    faceOf: (object) =>
      object.identity.kind === "known"
        ? faceByName?.get(object.identity.characteristics.name)
        : undefined,
    snapshot: state === "reading" ? null : (frames[step] ?? null),
    step,
    steps: state === "reading" ? 0 : frames.length,
    go: (to) => setStep(Math.max(0, Math.min(frames.length - 1, to))),
    nextTurn: () => setStep((at) => nextTurn(line, at)),
    previousTurn: () => setStep((at) => previousTurn(line, at)),
    everyHand,
    setEveryHand,
    notice: null,
  };
}
