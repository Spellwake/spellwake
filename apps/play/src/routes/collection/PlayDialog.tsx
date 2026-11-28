import { useEffect, useState, type ReactElement } from "react";
import { Search, Users } from "lucide-react";
import { ActionButton, Dialog, RadioGroup, SettingSwitch } from "@spellwake/ui";
import type { PlayMode } from "../../decks/deck-sources";
import {
  LENGTH_LABEL,
  type FindMatchSource,
  type MatchLength,
} from "../../play/find-match";
import * as s from "./play-dialog.css";

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
 * What Play on a deck offers: a table with friends (the lobby), or a match
 * found for you, which goes from searching straight to the table.
 */
export function PlayDialog({
  deck,
  mode,
  find,
  onFriends,
  onClose,
}: {
  deck: { name: string; format: string };
  mode: PlayMode;
  find: FindMatchSource;
  /** Opens the lobby with this deck. */
  onFriends: () => void;
  onClose: () => void;
}): ReactElement {
  const [choosing, setChoosing] = useState<"how" | "match">("how");
  const [wanted, setWanted] = useState<MatchLength>("single");
  const [ranked, setRanked] = useState(mode === "collection");
  const [stopping, setStopping] = useState(false);
  const lengths = find.lengths;
  // A format with one match length offers that one.
  const length: MatchLength =
    lengths && !lengths[wanted]
      ? lengths.single
        ? "single"
        : "three"
      : wanted;
  const canQueue = !!lengths && (lengths.single || lengths.three);
  const stop = (then?: () => void) => {
    setStopping(true);
    void find.cancel().finally(() => {
      setStopping(false);
      then?.();
    });
  };

  if (find.phase === "searching" || find.phase === "found") {
    const isFound = find.phase === "found";
    return (
      <Dialog
        open
        size="compact"
        title={isFound ? "Match found" : "Finding a match"}
        {...(find.terms ? { subtitle: find.terms } : {})}
        // Closing the dialog ends the search with it; a found match is not left.
        onClose={() => (isFound ? undefined : stop(onClose))}
        busy={isFound || stopping}
        actions={
          isFound ? undefined : (
            <ActionButton pending={stopping} onClick={() => stop()}>
              Cancel search
            </ActionButton>
          )
        }
      >
        <div className={s.searching} role="status">
          <span className={s.pulse} data-found={isFound} />
          {isFound ? (
            <p className={s.line}>Taking you to the table…</p>
          ) : (
            <p className={s.line}>
              Searching for players{" "}
              {find.since !== null && <Elapsed since={find.since} />}
            </p>
          )}
          {find.failure && <p className={s.failure}>{find.failure}</p>}
        </div>
      </Dialog>
    );
  }

  if (choosing === "match")
    return (
      <Dialog
        open
        size="compact"
        title="Find a match"
        subtitle={`${deck.name} · ${deck.format}`}
        onClose={onClose}
        busy={find.phase === "starting"}
        actions={
          <>
            <ActionButton
              disabled={find.phase === "starting"}
              onClick={() => setChoosing("how")}
            >
              Back
            </ActionButton>
            <ActionButton
              emphasis="primary"
              disabled={!canQueue}
              pending={find.phase === "starting"}
              onClick={() => find.find(length, ranked && mode === "collection")}
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
            disabled={find.phase === "starting"}
            options={(["single", "three"] as const).map((value) => ({
              value,
              label: LENGTH_LABEL[value],
              disabled: !lengths?.[value],
            }))}
            onValueChange={(value) =>
              setWanted(value === "three" ? "three" : "single")
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
                disabled={find.phase === "starting"}
                onCheckedChange={setRanked}
              />
            </div>
          )}
          {find.failure && (
            <p className={s.failure} role="alert">
              {find.failure}
            </p>
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
              Open a table. Invite friends or add bots.
            </span>
          </span>
        </button>
        <button
          type="button"
          className={s.choice}
          disabled={lengths !== null && !canQueue}
          onClick={() => setChoosing("match")}
        >
          <Search className={s.choiceIcon} aria-hidden="true" />
          <span className={s.choiceText}>
            <span className={s.choiceTitle}>Find a match</span>
            <span className={s.choiceHint}>
              {lengths !== null && !canQueue
                ? `Not available for ${deck.format} yet.`
                : mode === "collection"
                  ? "Be matched with a player. Ranked or casual."
                  : "Be matched with a player."}
            </span>
          </span>
        </button>
      </div>
    </Dialog>
  );
}
