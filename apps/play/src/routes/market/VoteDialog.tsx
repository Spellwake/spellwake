import {
  useEffect,
  useMemo,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import { useNavigate } from "react-router-dom";
import { Check } from "lucide-react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  SupplyIcon,
  TextField,
} from "@spellwake/ui";
import { BALLOT_LIMIT, type Ballot } from "../../market/supply-sources";
import * as s from "./supply.css";

const sameChoices = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((id) => b.includes(id));

/**
 * The supply vote, as approved: up to ten of the frozen candidate sets,
 * searched without losing choices made elsewhere in the list. X, Escape or a
 * press outside before saving discards the draft. Save vote replaces the
 * saved choices whole, and holds the dialog until the outcome is known. A
 * closed vote is read-only; published results list the winners.
 */
export function VoteDialog({
  open,
  ballot,
  onSave,
  onCheck,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  ballot: Ballot;
  onSave: (setIds: readonly string[]) => void;
  onCheck: () => void;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement {
  const navigate = useNavigate();
  const saved = ballot.status === "ready" ? ballot.saved : [];
  const [choices, setChoices] = useState<readonly string[]>(saved);
  const [query, setQuery] = useState("");
  const [isReadNoticeDismissed, setIsReadNoticeDismissed] = useState(false);
  // Each opening starts from the saved choices with an empty search.
  const savedKey = saved.join(",");
  useEffect(() => {
    if (!open) return;
    setChoices(savedKey ? savedKey.split(",") : []);
    setQuery("");
    setIsReadNoticeDismissed(false);
  }, [open, savedKey]);

  const ready = ballot.status === "ready" ? ballot : null;
  const save = ready?.save ?? null;
  const isUnconfirmed = save?.kind === "unconfirmed";
  const isFrozen = save?.kind === "pending" || isUnconfirmed;
  const needle = query.trim().toLowerCase();
  const shown = useMemo(
    () =>
      (ready?.candidates ?? []).filter((candidate) =>
        `${candidate.code ?? ""} ${candidate.name}`
          .toLowerCase()
          .includes(needle),
      ),
    [ready?.candidates, needle],
  );
  const isResults = ready?.results != null;
  const canEdit = !!ready?.canVote && !ready.readFailed && !isFrozen;
  const atLimit = choices.length >= BALLOT_LIMIT;

  const body =
    ballot.status === "loading" ? (
      <>
        <div className={s.voteHead}>
          <span>Choose up to {BALLOT_LIMIT} sets</span>
        </div>
        <TextField
          type="search"
          label="Search sets"
          placeholder="Search sets…"
          hideLabel
          treatment="compact-form"
          value=""
          onValueChange={() => undefined}
          disabled
        />
        <ul className={s.candidates} aria-hidden="true">
          {Array.from({ length: BALLOT_LIMIT }, (_, index) => (
            <li key={index} className={s.placeholder} />
          ))}
        </ul>
        <div className={s.voteFoot}>
          <span />
          <ActionButton className={s.saveVote} pending disabled>
            Save vote
          </ActionButton>
        </div>
      </>
    ) : ballot.status === "none" || ballot.status === "unavailable" ? (
      <div className={s.voteQuiet}>
        <SupplyIcon className={s.voteQuietIcon} aria-hidden="true" />
        <p>
          {ballot.status === "none"
            ? "No vote available"
            : "Account unavailable"}
        </p>
      </div>
    ) : isResults && ready ? (
      <>
        <p className={s.voteHead}>
          <span>
            {ready.results!.length} winning{" "}
            {ready.results!.length === 1 ? "set" : "sets"}
          </span>
        </p>
        <table className={s.results}>
          <thead>
            <tr>
              <th scope="col">Set</th>
              <th scope="col">Votes</th>
              <th scope="col">Packs</th>
            </tr>
          </thead>
          <tbody>
            {ready.results!.map((row) => (
              <tr key={row.name}>
                <th scope="row">{row.name}</th>
                <td>{row.votes}</td>
                <td>{row.packs}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className={s.voteFootNote}>Next drop {ready.nextDrop}</p>
      </>
    ) : ready ? (
      <>
        {ready.readFailed && !isReadNoticeDismissed && (
          <FeedbackMessages
            label="Vote messages"
            presentation="inline"
            items={[
              {
                id: "read",
                text: "Couldn’t load voting.",
                onDismiss: () => setIsReadNoticeDismissed(true),
              },
            ]}
          />
        )}
        <div className={s.voteHead}>
          <span>
            {ready.isOpen
              ? `Choose up to ${BALLOT_LIMIT} sets`
              : "Voting closed"}
          </span>
          <span>
            {ready.isOpen ? `Closes ${ready.closes}` : `Closed ${ready.closes}`}
          </span>
        </div>
        <TextField
          type="search"
          label="Search sets"
          placeholder="Search sets…"
          hideLabel
          treatment="compact-form"
          value={query}
          onValueChange={setQuery}
          disabled={ready.readFailed}
        />
        {shown.length > 0 ? (
          <ul className={s.candidates} aria-label="Candidate sets">
            {shown.map((candidate) => {
              const checked = choices.includes(candidate.setId);
              const disabled = !canEdit || (!checked && atLimit);
              return (
                <li key={candidate.setId}>
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={checked}
                    disabled={disabled}
                    className={s.candidate}
                    onClick={() =>
                      setChoices((current) =>
                        checked
                          ? current.filter((id) => id !== candidate.setId)
                          : [...current, candidate.setId],
                      )
                    }
                  >
                    <span className={s.box} aria-hidden="true">
                      {checked && <Check />}
                    </span>
                    {candidate.code && (
                      <span className={s.candidateCode}>{candidate.code}</span>
                    )}
                    <span className={s.candidateName}>{candidate.name}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className={s.voteNoMatch} role="status">
            No matching sets
          </p>
        )}
        {isUnconfirmed && (
          <p className={s.stageLine} data-tone="unknown" role="status">
            Vote not confirmed.
          </p>
        )}
        {save?.kind === "rejected" && (
          <p className={s.stageLine} data-tone="known" role="status">
            {save.message}
          </p>
        )}
        <div className={s.voteFoot}>
          {ready.isOpen && !ready.qualifies ? (
            <>
              <span>Complete a qualifying game to vote.</span>
              <ActionButton
                className={s.saveVote}
                onClick={() => {
                  onClose();
                  void navigate("/open-play");
                }}
              >
                Open Play
              </ActionButton>
            </>
          ) : (
            <>
              <span role="status">{choices.length} selected</span>
              {ready.isOpen ? (
                <ActionButton
                  className={s.saveVote}
                  pending={
                    save?.kind === "pending" ||
                    (isUnconfirmed && save.isChecking)
                  }
                  disabled={
                    !isUnconfirmed &&
                    (!canEdit || sameChoices(choices, ready.saved))
                  }
                  onClick={() => (isUnconfirmed ? onCheck() : onSave(choices))}
                >
                  {isUnconfirmed ? "Check status" : "Save vote"}
                </ActionButton>
              ) : (
                <span>Results pending</span>
              )}
            </>
          )}
        </div>
      </>
    ) : null;

  return (
    <Dialog
      open={open}
      title={isResults ? "Supply results" : "Supply vote"}
      size="wide"
      busy={isFrozen}
      returnFocusRef={returnFocusRef}
      onClose={() => {
        if (!isFrozen) onClose();
      }}
    >
      <div className={s.vote}>{body}</div>
    </Dialog>
  );
}
