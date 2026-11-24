import { useEffect, useState, type ReactElement } from "react";
import { Search, Users } from "lucide-react";
import { ActionButton, Dialog, RadioGroup, SettingSwitch } from "@spellwake/ui";
import type { PlayMode } from "../../decks/deck-sources";
import * as s from "./play-dialog.css";

type MatchLength = "single" | "three";
const LENGTH_LABEL: Record<MatchLength, string> = {
  single: "Single game",
  three: "Best of three",
};

/** The time spent searching, as "0:12". */
function Elapsed({ since }: { since: number }): ReactElement {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, []);
  const seconds = Math.max(0, Math.floor((now - since) / 1000));
  return (
    <span className={s.elapsed}>
      {Math.floor(seconds / 60)}:{String(seconds % 60).padStart(2, "0")}
    </span>
  );
}

/**
 * Concept: what Play on a deck offers. A table with friends (the lobby), or a
 * match found for you, which goes from searching straight to the table.
 */
export function PlayDialog({
  deck,
  mode,
  initial = "choose",
  onFriends,
  onClose,
}: {
  deck: { name: string; format: string };
  mode: PlayMode;
  /** Concept only: opens at a later step for review captures. */
  initial?: "choose" | "find" | "searching" | "found";
  /** Opens the lobby with this deck. */
  onFriends: () => void;
  onClose: () => void;
}): ReactElement {
  const [step, setStep] = useState(initial);
  const [length, setLength] = useState<MatchLength>("single");
  const [ranked, setRanked] = useState(mode === "collection");
  const [since, setSince] = useState(() => Date.now());
  const terms = [
    deck.format,
    LENGTH_LABEL[length],
    ...(mode === "collection" && ranked ? ["Ranked"] : []),
  ].join(" · ");

  if (step === "searching" || step === "found")
    return (
      <Dialog
        open
        size="compact"
        title={step === "found" ? "Match found" : "Finding a match"}
        subtitle={terms}
        onClose={onClose}
        actions={
          step === "searching" ? (
            <ActionButton onClick={() => setStep("find")}>
              Cancel search
            </ActionButton>
          ) : undefined
        }
      >
        <div className={s.searching} role="status">
          <span className={s.pulse} data-found={step === "found"} />
          {step === "found" ? (
            <p className={s.line}>Taking you to the table…</p>
          ) : (
            <p className={s.line}>
              Searching for players <Elapsed since={since} />
            </p>
          )}
        </div>
      </Dialog>
    );

  if (step === "find")
    return (
      <Dialog
        open
        size="compact"
        title="Find a match"
        subtitle={`${deck.name} · ${deck.format}`}
        onClose={onClose}
        actions={
          <>
            <ActionButton onClick={() => setStep("choose")}>Back</ActionButton>
            <ActionButton
              emphasis="primary"
              onClick={() => {
                setSince(Date.now());
                setStep("searching");
              }}
            >
              Find match
            </ActionButton>
          </>
        }
      >
        <div className={s.options}>
          <RadioGroup
            label="Match length"
            treatment="chips"
            value={length}
            options={[
              { value: "single", label: LENGTH_LABEL.single },
              { value: "three", label: LENGTH_LABEL.three },
            ]}
            onValueChange={(value) =>
              setLength(value === "three" ? "three" : "single")
            }
          />
          {mode === "collection" && (
            <div className={s.setting}>
              <span className={s.settingText}>
                <span className={s.choiceTitle}>Ranked</span>
                <span className={s.choiceHint}>Counts toward your rank.</span>
              </span>
              <SettingSwitch
                label="Ranked"
                checked={ranked}
                onCheckedChange={setRanked}
              />
            </div>
          )}
        </div>
      </Dialog>
    );

  return (
    <Dialog
      open
      size="compact"
      title={`Play ${deck.name}`}
      subtitle={deck.format}
      onClose={onClose}
    >
      <div className={s.choices}>
        <button type="button" className={s.choice} onClick={onFriends}>
          <Users className={s.choiceIcon} aria-hidden="true" />
          <span className={s.choiceText}>
            <span className={s.choiceTitle}>Play with friends</span>
            <span className={s.choiceHint}>
              Open a table. Invite friends or add AI.
            </span>
          </span>
        </button>
        <button
          type="button"
          className={s.choice}
          onClick={() => setStep("find")}
        >
          <Search className={s.choiceIcon} aria-hidden="true" />
          <span className={s.choiceText}>
            <span className={s.choiceTitle}>Find a match</span>
            <span className={s.choiceHint}>
              {mode === "collection"
                ? "Be matched with a player. Ranked or casual."
                : "Be matched with a player."}
            </span>
          </span>
        </button>
      </div>
    </Dialog>
  );
}
