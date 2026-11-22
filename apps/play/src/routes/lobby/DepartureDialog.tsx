import type { ReactElement } from "react";
import { ActionButton, Dialog } from "@spellwake/ui";
import type { Departure } from "../../lobby/lobby-sources";

/**
 * Leaving the lobby, or closing it as host: confirm, then the pending and
 * unresolved states, as approved. Nothing navigates away before the room
 * accepts the departure; an unknown outcome only offers Check status.
 */
export function DepartureDialog({
  departure,
  isHost,
  onCancel,
  onConfirm,
  onCheck,
}: {
  departure: Departure;
  isHost: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  onCheck: () => void;
}): ReactElement {
  const verb = isHost ? "Close" : "Leave";
  const copy = {
    confirm: {
      title: `${verb} lobby?`,
      text: isHost
        ? "This lobby will close for everyone."
        : "You’ll leave this lobby. Everyone else stays.",
    },
    pending: {
      title: isHost ? "Closing lobby…" : "Leaving lobby…",
      text: isHost
        ? "This closes the lobby for everyone."
        : "You’re leaving this lobby.",
    },
    unconfirmed: {
      title: isHost ? "Closure unconfirmed" : "Departure unconfirmed",
      text: isHost
        ? "The lobby closure is not confirmed."
        : "Leaving the lobby is not confirmed.",
    },
    rejected: {
      title: isHost ? "Lobby not closed" : "Still in lobby",
      text: isHost ? "The lobby remains open." : "You’re still in this lobby.",
    },
  }[departure.stage];
  const isBusy =
    departure.stage === "pending" ||
    (departure.stage === "unconfirmed" && departure.isChecking);
  return (
    <Dialog
      open
      title={copy.title}
      size="compact"
      busy={isBusy || departure.stage === "unconfirmed"}
      onClose={onCancel}
      actions={
        departure.stage === "confirm" ? (
          <>
            <ActionButton onClick={onCancel}>Cancel</ActionButton>
            <ActionButton emphasis="primary" onClick={onConfirm}>
              {verb} lobby
            </ActionButton>
          </>
        ) : departure.stage === "pending" ? (
          <>
            <ActionButton disabled>Cancel</ActionButton>
            <ActionButton emphasis="primary" pending>
              {isHost ? "Closing…" : "Leaving…"}
            </ActionButton>
          </>
        ) : departure.stage === "unconfirmed" ? (
          <>
            <ActionButton disabled>{verb} lobby</ActionButton>
            <ActionButton
              emphasis="primary"
              pending={departure.isChecking}
              onClick={onCheck}
            >
              Check status
            </ActionButton>
          </>
        ) : (
          <>
            <ActionButton onClick={onCancel}>Back to lobby</ActionButton>
            <ActionButton emphasis="primary" onClick={onCheck}>
              Check status
            </ActionButton>
          </>
        )
      }
    >
      <p>{copy.text}</p>
    </Dialog>
  );
}
