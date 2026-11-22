import { useState } from "react";
import { AudioLines, Droplet, Music, Volume2, X } from "lucide-react";
import {
  ActionButton,
  ChatComposer,
  ChatMessage,
  LoadingShape,
  LoadingState,
  MatchActions,
  RangeSlider,
  SettingSwitch,
  TurnPhaseDisplay,
  TargetConnection,
} from "@spellwake/ui";
import { cardImage } from "./assets";

const history = [
  ["Alex", "Good luck, everyone."],
  ["Morgan", "Good luck!"],
  ["You", "Have fun."],
  ["Sam", "Nice opening."],
  ["Riley", "Your turn, Alex."],
  ["Jordan", "Ready."],
] as const;
type ChatKind =
  | "read"
  | "draft"
  | "sent"
  | "watch"
  | "empty"
  | "loading"
  | "failed"
  | "unreadable";
export function TableChat({
  kind,
  close,
  go,
  stored,
  onStore,
  savedMessages = [],
  onAccept,
}: {
  kind: ChatKind;
  close: () => void;
  go: (id: string) => void;
  stored: string;
  onStore: (s: string) => void;
  savedMessages?: string[];
  onAccept?: (s: string) => void;
}) {
  const [draft, setDraft] = useState(
    stored ||
      (["draft", "failed"].includes(kind) ? "Good game, everyone." : ""),
  );
  const [sent, setSent] = useState<string[]>(
    savedMessages.length
      ? savedMessages
      : kind === "sent"
        ? ["Good game, everyone."]
        : [],
  );
  const hidden =
    kind === "empty" || kind === "loading" || kind === "unreadable";
  function submit(s: string) {
    setSent((p) => [...p, s]);
    setDraft("");
    onStore("");
    if (onAccept) onAccept(s);
    else if (kind === "draft") go("CM-04");
  }
  return (
    <section className="table-conversation">
      <header>
        Table chat
        <button type="button" aria-label="Close table chat" onClick={close}>
          <X size={23} />
        </button>
      </header>
      <div className="table-log" role="log">
        {!hidden &&
          history
            .slice(0, kind === "read" || kind === "failed" ? 3 : 6)
            .map(([author, body]) => (
              <ChatMessage
                key={author}
                author={author}
                body={body}
                surface="table"
              />
            ))}
        {sent.map((body, i) => (
          <ChatMessage key={i} author="You" body={body} isNew surface="table" />
        ))}
        {kind === "empty" && !sent.length && <i>No messages yet.</i>}
        {kind === "unreadable" && <i>Couldn’t load chat.</i>}
        {kind === "loading" && (
          <LoadingState label="Loading messages">
            {[200, 280, 230].map((w) => (
              <LoadingShape key={w} width={w} height={15} />
            ))}
          </LoadingState>
        )}
      </div>
      {kind === "failed" && <em>Couldn’t send message.</em>}
      {kind === "watch" ? (
        <ChatComposer mode="watching" />
      ) : (
        <ChatComposer
          mode="editable"
          maxCharacters={1000}
          surface="table"
          pending={kind === "loading" || kind === "unreadable"}
          value={draft}
          placeholder="Message the table…"
          onChange={(s) => {
            setDraft(s);
            onStore(s);
          }}
          onSend={submit}
        />
      )}
    </section>
  );
}
const channels = [
  { name: "Master volume", Icon: Volume2 },
  { name: "Music", Icon: Music },
  { name: "Sound effects", Icon: AudioLines },
];
export interface Preferences {
  levels: number[];
  auto: boolean;
}
export function Settings({
  prefs,
  change,
  close,
  concede,
}: {
  prefs: Preferences;
  change: (p: Preferences) => void;
  close: () => void;
  concede?: () => void;
}) {
  return (
    <section className="match-settings" aria-label="Settings">
      <header>
        Settings
        <button type="button" aria-label="Close settings" onClick={close}>
          <X />
        </button>
      </header>
      {channels.map(({ name, Icon }, i) => (
        <div key={name} className="match-setting">
          <span>
            <Icon />
            {name}
          </span>
          <RangeSlider
            label={name}
            value={prefs.levels[i] ?? 0}
            min={0}
            max={100}
            step={1}
            onValueChange={(value) =>
              change({
                ...prefs,
                levels: prefs.levels.map((n, j) => (i === j ? value : n)),
              })
            }
          />
        </div>
      ))}
      <div className="match-setting match-switch">
        <span>
          <Droplet />
          Auto-tap mana
        </span>
        <SettingSwitch
          label="Auto-tap mana"
          checked={prefs.auto}
          onCheckedChange={(auto) => change({ ...prefs, auto })}
        />
      </div>
      {concede && (
        <button type="button" className="concede-entry" onClick={concede}>
          Concede
        </button>
      )}
    </section>
  );
}
export function Concede({
  close,
  confirm,
  pending = false,
  failed = false,
}: {
  close: () => void;
  confirm: () => void;
  pending?: boolean;
  failed?: boolean;
}) {
  return (
    <section
      className="match-settings concede-confirm"
      aria-label="Confirm concession"
    >
      <header>
        Concede?
        <button
          type="button"
          disabled={pending}
          onClick={close}
          aria-label="Close confirmation"
        >
          <X />
        </button>
      </header>
      <p>You can stay and watch.</p>
      {failed && <p role="alert">Couldn’t concede.</p>}
      <div>
        <ActionButton disabled={pending} onClick={close}>
          Cancel
        </ActionButton>
        <ActionButton className="danger" pending={pending} onClick={confirm}>
          Concede
        </ActionButton>
      </div>
    </section>
  );
}
export function feedbackBoard(id: string) {
  return ["CM-05", "CM-10", "AT-07", "BL-06", "BL-09", "DA09"].includes(id);
}
export function FeedbackBoard({
  id,
  go,
  origin,
}: {
  id: string;
  go: (id: string) => void;
  origin: string;
}) {
  const [prefs, change] = useState<Preferences>({
    levels: [80, 50, 80],
    auto: true,
  });
  if (id === "CM-05")
    return (
      <div className="match-excerpts chat-excerpts">
        {(["empty", "loading", "failed", "unreadable"] as const).map(
          (kind, i) => (
            <section key={kind}>
              <h2>
                {
                  [
                    "A. Empty",
                    "B. Loading",
                    "C. Send feedback",
                    "D. Read feedback",
                  ][i]
                }
              </h2>
              <TableChat
                kind={kind}
                close={() => go("CM-01")}
                go={go}
                stored=""
                onStore={() => {}}
              />
            </section>
          ),
        )}
      </div>
    );
  if (id === "CM-10")
    return (
      <div className="match-excerpts settings-excerpts">
        <article>
          <h2>A · Settings while watching</h2>
          <Settings prefs={prefs} change={change} close={() => go("CM-09")} />
        </article>
        <article>
          <h2>B · Submitting concession</h2>
          <Concede pending close={() => {}} confirm={() => {}} />
          <h2>C · Could not concede</h2>
          <Concede
            failed
            close={() => go("CM-07")}
            confirm={() => go("CM-09")}
          />
        </article>
      </div>
    );
  if (id === "BL-09")
    return (
      <div className="match-excerpts blocks-excerpts">
        <h1>
          Spellwake <small>Component state review board</small>
        </h1>
        <p>Offered capacity · up to two attackers</p>
        <div>
          {[0, 1, 2].map((count) => (
            <BlockCapacity key={count} initial={count} go={go} />
          ))}
        </div>
      </div>
    );
  const damage = id === "DA09",
    block = id === "BL-06",
    verb = block ? "Block" : damage ? "Confirm" : "Attack";
  const statuses = damage
    ? ["Submitting", "Not submitted", "Accepted"]
    : [
        "Required " + (block ? "blocker" : "attacker"),
        "No legal " + (block ? "blockers" : "attackers"),
        "Updated selection",
        "Submitting",
        "Submission feedback",
      ];
  return (
    <div
      className={
        "match-excerpts combat-excerpts " +
        (block ? "blocks " : damage ? "damage " : "attacks")
      }
    >
      <h1>
        {damage ? "Damage submission · UI excerpts" : "Design review board"}
      </h1>
      <div>
        {statuses.map((title, i) => {
          const pending = damage ? i === 0 : i === 3,
            required = !damage && i === 0,
            none = !damage && i === 1;
          return (
            <section key={title}>
              <h2>{title}</h2>
              <div className="combat-example">
                <TurnPhaseDisplay
                  turnLabel={block ? "Alex’s turn" : "Your turn"}
                  phase="combat"
                  phaseLabel="Combat"
                />
                {!damage && !none && (
                  <img
                    className="example-card"
                    src={cardImage(block ? "angel" : "bear").src}
                    alt={block ? "Serra Angel" : "Grizzly Bears"}
                  />
                )}
                {required && <span>Must {verb.toLowerCase()}</span>}
                {damage && i < 2 && (
                  <p>Serra Angel · 4 damage · 4 / 4 assigned</p>
                )}
                {(!damage || i < 2) && (
                  <MatchActions
                    primary={{
                      label: verb,
                      pending,
                      disabled: required || none,
                      onActivate: () =>
                        go(damage ? "DA10" : block ? "BL-07" : "AT-08"),
                    }}
                    {...(!required && !damage
                      ? {
                          secondary: {
                            label: block ? "No Blocks" : "No Attacks",
                            onActivate: () => go(block ? "BL-08" : "AT-09"),
                          },
                        }
                      : {})}
                  />
                )}
                {!damage && i === 2 && <i>{verb} selection changed.</i>}
                {((!damage && i === 4) || (damage && i === 1)) && (
                  <i>
                    {damage
                      ? "Couldn’t submit. Try again."
                      : "Couldn’t declare " +
                        (block ? "blockers." : "attackers.")}
                  </i>
                )}
                {damage && i === 0 && (
                  <div className="match-split-readout">
                    <span>− 4 +</span>
                    <span>− 0 +</span>
                  </div>
                )}
                {damage && i === 1 && (
                  <button
                    className="match-small-link"
                    onClick={() => go(origin)}
                  >
                    Return to assignment
                  </button>
                )}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}
function BlockCapacity({
  initial,
  go,
}: {
  initial: number;
  go: (s: string) => void;
}) {
  const [links, setLinks] = useState(() =>
    Array.from({ length: initial }, (_, i) => i),
  );
  const selected = links.length;
  function toggle(i: number) {
    setLinks((items) =>
      items.includes(i) ? items.filter((x) => x !== i) : [...items, i],
    );
  }
  return (
    <article>
      <h2>
        {
          [
            "No block assignments",
            "One attacker chosen",
            "Two attackers chosen",
          ][selected]
        }
      </h2>
      <TurnPhaseDisplay
        turnLabel="Alex’s turn"
        phase="combat"
        phaseLabel="Combat"
      />
      <MatchActions
        primary={{
          label: "Block",
          disabled: !selected,
          onActivate: () => go("BL-07"),
        }}
        secondary={{ label: "No Blocks", onActivate: () => go("BL-08") }}
      />
      <div className="capacity-targets">
        {(["chandra", "bear"] as const).map((c, i) => (
          <button
            key={c}
            type="button"
            aria-label={"Block " + cardImage(c).name}
            onClick={() => toggle(i)}
          >
            <img src={cardImage(c).src} alt={cardImage(c).name} />
          </button>
        ))}
        {links.map((i) => (
          <TargetConnection
            key={i}
            from={{ x: 175, y: 295 }}
            to={{ x: i ? 305 : 74, y: 90 }}
          />
        ))}
      </div>
      <button
        type="button"
        className="capacity-source"
        aria-label="Clear blocker assignments"
        onClick={() => setLinks([])}
      >
        <img src={cardImage("angel").src} alt="Serra Angel" />
      </button>
    </article>
  );
}
