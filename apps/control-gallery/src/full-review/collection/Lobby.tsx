import { useState } from "react";
import { Check, Sparkle, UserRound, Users } from "lucide-react";
import { ReviewHeader } from "../ReviewHeader";
import {
  Art,
  Board,
  Btn,
  Choice,
  DeckArt,
  Example,
  Modal,
  Search,
  type Jump,
} from "./common";
import { decks, formats, shelfFor, type DeckLaunch } from "./deck-library";
const ruleFields = [
  "Format",
  "Table rules",
  "Players",
  "Match",
  "Planechase",
  "Range of influence",
];
const ruleChoices = [
  [...formats],
  ["Free-for-All", "Emperor", "Two-Headed Giant"],
  ["4", "2", "3", "5", "6"],
  ["Single game", "Best of three"],
  ["Off", "On"],
  ["Unlimited", "1", "2"],
];
export function Lobby({
  id,
  go,
  launch,
}: {
  id: string;
  go: Jump;
  launch?: DeckLaunch | undefined;
}) {
  const isMember = id.includes("member") || id.includes("no-deck"),
    teams = id === "L-13-teams",
    filled =
      id === "L-01-filled" || id.startsWith("L-08") || id === "L-12-starting",
    countdown = id.startsWith("L-08"),
    starting = id === "L-12-starting";
  const library = launch ? shelfFor(launch.source) : decks;
  const initialDeck = launch
    ? library.find((item) => item.id === launch.deckId)
    : id === "L-02-no-deck"
      ? undefined
      : decks[isMember ? 3 : 0];
  const [ready, setReady] = useState(filled),
    [localId, setId] = useState(id),
    [deck, setDeck] = useState(initialDeck),
    [aiDeck, setAiDeck] = useState("Random Deck"),
    [rules, setRules] = useState(
      ruleChoices.map((v, i) =>
        i === 0
          ? (initialDeck?.format ?? "Standard")
          : teams && i === 1
            ? "Emperor"
            : teams && i === 2
              ? "6"
              : (v[0] ?? ""),
      ),
    ),
    [notice, setNotice] = useState("");
  if (id.endsWith("feedback")) return <LobbyFeedback id={id} go={go} />;
  const eligible = Boolean(deck && deck.format === rules[0]);
  const display = (next: string) => setId(next);
  const back = () => display(isMember ? "L-02-member" : "L-01-host");
  const people = teams
    ? ["Mira", "You · Host", "Alden", "Sora", "Rowan", "Nia"]
    : [
        isMember ? "Planeswalker · Host" : "You · Host",
        isMember ? "You · Mira" : "Mira",
        "AI player",
        filled ? "Sol" : "Open seat",
      ];
  const modalName =
    localId === "L-05-own-deck" || localId === "L-06-ai-deck"
      ? "Choose Deck"
      : localId === "L-07-invite"
        ? "Invite players"
        : localId === "L-04-rules"
          ? "Game rules"
          : localId.startsWith("L-09")
            ? isMember
              ? "Leave lobby?"
              : "Close lobby?"
            : "";
  return (
    <>
      <ReviewHeader active="Open Play" go={go} coins={0} />
      {id === "L-11-notice" ? (
        <div className="cs-center">
          <section className="cs-panel cs-admission">
            <h2>Lobby full</h2>
            <p>Ask the host to add a player slot.</p>
            <Btn onClick={() => go("DL-05")}>Back</Btn>
          </section>
        </div>
      ) : (
        <main className="cs-lobby">
          <header>
            <Btn
              onClick={() => display(isMember ? "L-09-member" : "L-09-host")}
            >
              ‹ Back
            </Btn>
            <div>
              <h1>Lobby</h1>
              <p>
                {rules[0]} · {rules[1]} · {rules[2]} players
              </p>
            </div>
          </header>
          <div className="cs-lobby-columns">
            <section className="cs-panel cs-players">
              <header>
                <h2>
                  Players
                  <small>{teams ? "6/6" : filled ? "4/4" : "3/4"}</small>
                </h2>
                {!isMember && (
                  <Btn
                    disabled={countdown || starting || filled}
                    onClick={() => display("L-07-invite")}
                  >
                    <Users /> Invite players
                  </Btn>
                )}
              </header>
              {people.map((name, i) => (
                <div key={name}>
                  {teams && i % 3 === 0 && <h4>Team {i / 3 + 1}</h4>}
                  <div className="cs-seat">
                    <div className="cs-seat-name">
                      {i < 2 || teams || name === "Sol" ? (
                        <Art
                          source="L-01-host"
                          rect={
                            i % 2 === 0
                              ? [104, 328, 80, 79]
                              : [104, 441, 80, 80]
                          }
                          className="cs-avatar"
                        />
                      ) : (
                        <span className="cs-avatar">
                          {i === 2 ? <Sparkle /> : <UserRound />}
                        </span>
                      )}
                      {name}
                    </div>
                    <div className="cs-seat-controls">
                      {teams ? (
                        <>
                          <Btn onClick={() => display("L-05-own-deck")}>
                            {i === 1 ? "Emperor" : "Left general"}
                          </Btn>
                          <small>Deck selected</small>
                        </>
                      ) : i === (isMember ? 1 : 0) ? (
                        <Btn
                          disabled={countdown || starting}
                          onClick={() => display("L-05-own-deck")}
                        >
                          {deck
                            ? deck.name + " · " + deck.format
                            : "Choose deck"}
                          ⌄
                        </Btn>
                      ) : i === 2 ? (
                        <>
                          <Choice
                            value="AI"
                            items={["Human", "AI", "None"]}
                            change={() => display("L-03-controller")}
                            open={localId === "L-03-controller"}
                          />
                          <Choice
                            value="Easy"
                            items={["Easy", "Normal", "Hard"]}
                            change={(v) => setNotice("AI difficulty: " + v)}
                          />
                          <Btn
                            disabled={isMember || countdown}
                            onClick={() => display("L-06-ai-deck")}
                          >
                            {library.find((item) => item.id === aiDeck)?.name ??
                              "Random Deck"}
                            ⌄
                          </Btn>
                        </>
                      ) : (
                        <>
                          <span>Human</span>
                          {name !== "Open seat" && <small>Deck selected</small>}
                        </>
                      )}
                    </div>
                    {name === "Open seat" ? (
                      <i>Waiting for player</i>
                    ) : (
                      <small
                        className={
                          i === 2 || (ready && i === 0) ? "cs-ready" : ""
                        }
                      >
                        {i === 2 || (ready && i === 0)
                          ? "✓ Ready"
                          : "◯ Not ready"}
                      </small>
                    )}
                  </div>
                </div>
              ))}
            </section>
            <aside>
              <section className="cs-panel cs-lobby-deck">
                <p>Your deck</p>
                {!deck ? (
                  <Btn onClick={() => display("L-05-own-deck")}>
                    Choose your deck
                  </Btn>
                ) : (
                  <>
                    <div>
                      <h2>{deck.name}</h2>
                      <small>{deck.format}</small>
                    </div>
                    <DeckArt index={deck.artIndex} name={deck.name} />
                  </>
                )}
              </section>
              <section className="cs-panel cs-rules">
                <dl>
                  {ruleFields.map((field, i) => (
                    <div key={field}>
                      <dt>
                        {field === "Players"
                          ? "Players"
                          : field === "Range of influence"
                            ? "Influence"
                            : field}
                      </dt>
                      <dd>{rules[i]}</dd>
                    </div>
                  ))}
                </dl>
                {!isMember && (
                  <Btn
                    disabled={countdown || starting}
                    onClick={() => display("L-04-rules")}
                  >
                    ✎ Edit rules
                  </Btn>
                )}
              </section>
            </aside>
          </div>
          <footer>
            {starting ? (
              <p>◌ Starting game…</p>
            ) : (
              <>
                <Btn disabled={!eligible} onClick={() => setReady(!ready)}>
                  {ready ? "Not ready" : "Ready"}
                </Btn>
                {countdown ? (
                  <div className="cs-countdown">Starting in 10 seconds</div>
                ) : (
                  !isMember && (
                    <Btn
                      disabled={!eligible || (!filled && !teams)}
                      primary
                      onClick={() => go("L-08-host")}
                    >
                      Start game
                    </Btn>
                  )
                )}
              </>
            )}
          </footer>
        </main>
      )}
      {notice && (
        <button className="cs-toast" onClick={() => setNotice("")}>
          {notice}
        </button>
      )}
      {modalName === "Choose Deck" || modalName === "Invite players" ? (
        <LobbyPicker
          title={modalName}
          options={
            modalName === "Invite players"
              ? ["Sol", "Nia", "Rowan"].map((name) => ({ id: name, name }))
              : [
                  ...(localId === "L-06-ai-deck"
                    ? [{ id: "Random Deck", name: "Random Deck" }]
                    : []),
                  ...library.filter((item) => item.format === rules[0]),
                ]
          }
          initial={
            modalName === "Invite players"
              ? "Sol"
              : localId === "L-06-ai-deck"
                ? aiDeck
                : deck?.id
          }
          format={rules[0]!}
          close={back}
          onPick={(v) => {
            if (modalName === "Invite players")
              setNotice("Invitation sent to " + v);
            else if (localId === "L-06-ai-deck") setAiDeck(v);
            else {
              const next = library.find((item) => item.id === v);
              if (next && next.id !== deck?.id) {
                setDeck(next);
                setReady(false);
              }
            }
            back();
          }}
        />
      ) : modalName === "Game rules" ? (
        <RuleEditor
          rules={rules}
          close={back}
          save={(v) => {
            if (v[0] !== rules[0]) setReady(false);
            setRules(v);
            back();
          }}
        />
      ) : modalName ? (
        <Modal
          title={modalName}
          close={back}
          className="cs-compact-modal"
          footer={
            <>
              <Btn onClick={back}>{isMember ? "Stay" : "Cancel"}</Btn>
              <Btn onClick={() => go(launch?.source ?? "DL-05", launch)}>
                {isMember ? "Leave" : "Close lobby"}
              </Btn>
            </>
          }
        >
          <p>
            {isMember
              ? "Only you will leave. The lobby stays open."
              : "This lobby will close for everyone."}
          </p>
        </Modal>
      ) : null}
    </>
  );
}
type PickerOption = { id: string; name: string; artIndex?: number };
function LobbyPicker({
  title,
  options,
  initial,
  format,
  close,
  onPick,
}: {
  title: string;
  options: PickerOption[];
  initial: string | undefined;
  format: string;
  close: () => void;
  onPick: (id: string) => void;
}) {
  const invite = title === "Invite players";
  const [q, search] = useState(invite ? "Sol" : ""),
    [selected, select] = useState(initial);
  const visible = options.filter((item) =>
    item.name.toLowerCase().includes(q.trim().toLowerCase()),
  );
  const choice = visible.find((item) => item.id === selected);
  return (
    <Modal
      title={title}
      {...(!invite ? { subtitle: format } : {})}
      close={close}
      className="cs-lobby-picker"
      footer={
        <>
          <Btn onClick={close}>Cancel</Btn>
          <Btn
            primary
            disabled={!choice}
            onClick={() => choice && onPick(choice.id)}
          >
            {invite ? "Invite" : "Select"}
          </Btn>
        </>
      }
    >
      <Search
        label={invite ? "Search players" : "Search decks"}
        value={q}
        onChange={search}
      />
      <div className="cs-pick-list">
        {visible.map(({ id, name, artIndex }) => (
          <button
            aria-pressed={id === selected}
            key={id}
            onClick={() => select(id)}
          >
            {invite ? (
              <Art
                source="L-07-invite"
                rect={[527, 386, 80, 80]}
                alt={name}
                className="cs-avatar"
              />
            ) : (
              <DeckArt index={artIndex ?? 0} name={name} />
            )}
            <span>
              {name}
              {!invite && <small>{format}</small>}
            </span>
            {id === selected ? <Check /> : "○"}
          </button>
        ))}
      </div>
    </Modal>
  );
}
function RuleEditor({
  rules,
  close,
  save,
}: {
  rules: string[];
  close: () => void;
  save: (r: string[]) => void;
}) {
  const [draft, edit] = useState(rules);
  return (
    <Modal
      title="Game rules"
      close={close}
      className="cs-lobby-picker cs-rule-editor"
      footer={
        <>
          <Btn onClick={close}>Cancel</Btn>
          <Btn onClick={() => save(draft)}>Apply changes</Btn>
        </>
      }
    >
      {ruleFields.map((f, i) => (
        <div className="cs-rule-field" key={f}>
          <label>{f}</label>
          <Choice
            value={draft[i] ?? ""}
            items={ruleChoices[i] ?? []}
            change={(v) =>
              edit((old) => old.map((item, j) => (j === i ? v : item)))
            }
          />
        </div>
      ))}
    </Modal>
  );
}
function LobbyFeedback({ id, go }: { id: string; go: Jump }) {
  const list = id === "list-feedback",
    titles = list
      ? [
          "Loading decks…",
          "No matching decks",
          "No eligible decks",
          "Couldn’t load your decks",
          "Players couldn’t be loaded",
          "Invitation sent to Sol",
        ]
      : [
          "Closing lobby…",
          "Closure unconfirmed",
          "Lobby not closed",
          "Starting game…",
          "Start unconfirmed",
          "AI deck unavailable",
        ];
  return (
    <Board
      columns={3}
      title={
        list ? "List and invitation feedback" : "Spellwake │ Operation feedback"
      }
    >
      {titles.map((title, i) => (
        <Example key={title} title="" width={480} height={300}>
          <div className="cs-center">
            <h2>{title}</h2>
            <p>
              {list
                ? i === 1
                  ? "Try another name."
                  : ""
                : [
                    "This closes the lobby for everyone.",
                    "The lobby closure is not confirmed.",
                    "The lobby remains open.",
                    "",
                    "The game start is not confirmed.",
                    "Choose another deck.",
                  ][i]}
            </p>
            {(list ? i === 3 || i === 4 : i !== 3) && (
              <Btn
                disabled={!list && i === 0}
                onClick={() =>
                  go(
                    list
                      ? "L-05-own-deck"
                      : i === 5
                        ? "L-06-ai-deck"
                        : "L-01-host",
                  )
                }
              >
                {list
                  ? "Try again"
                  : [
                      "Closing…",
                      "Check status",
                      "Back to lobby",
                      "",
                      "Check status",
                      "Choose Deck",
                    ][i]}
              </Btn>
            )}
          </div>
        </Example>
      ))}
    </Board>
  );
}
