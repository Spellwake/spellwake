import { useState, useRef, useEffect } from "react";
import {
  PeopleRail,
  ChatMessage,
  ChatComposer,
  ConversationEvent,
} from "@spellwake/ui";
import { MoreVertical, X } from "lucide-react";
import { art, Avatar, Btn, Search, Shell, Window } from "./shared";
import type { ScreenProps } from "../../review-types";
import {
  shelfFor,
  formats,
  playDeck,
  editDeck,
} from "../collection/deck-library";
function Decks({ go }: { go: ScreenProps["go"] }) {
  const [search, set] = useState(""),
    [format, setFormat] = useState("All formats");
  return (
    <>
      <div className="soc-tools">
        <Search label="Search decks" value={search} set={set} />
        <select
          aria-label="Format"
          value={format}
          onChange={(event) => setFormat(event.target.value)}
        >
          <option value="All formats">Format</option>
          {formats.map((name) => (
            <option key={name}>{name}</option>
          ))}
        </select>
      </div>
      <div className="soc-decks">
        {shelfFor("DL-01")
          .filter(
            (deck) =>
              deck.name.toLowerCase().includes(search.toLowerCase()) &&
              (format === "All formats" || format === deck.format),
          )
          .map((deck) => (
            <article key={deck.id}>
              <button
                className="soc-backdrop-deck"
                aria-label={`Open ${deck.name}`}
                onClick={() =>
                  editDeck(go, {
                    source: "DL-01",
                    deckId: deck.id,
                    query: search,
                    formatFilter: format,
                    scrollTop: 0,
                  })
                }
              >
                {deck.artIndex >= 0 && (
                  <img
                    alt=""
                    src={
                      art[
                        "box-" +
                          ["red", "blue", "white", "white", "black", "red"][
                            deck.artIndex
                          ]
                      ]
                    }
                  />
                )}
                <h3>{deck.name}</h3>
                <span>{deck.format}</span>
              </button>
              <Btn
                onClick={() =>
                  playDeck(go, {
                    source: "DL-01",
                    deckId: deck.id,
                    query: search,
                    formatFilter: format,
                    scrollTop: 0,
                  })
                }
              >
                ▷ Play
              </Btn>
            </article>
          ))}
      </div>
    </>
  );
}
export function Messaging({ id, go }: { id: string; go: ScreenProps["go"] }) {
  const find = id === "MSG-S01" || id === "MSG-D11",
    requests = id === "MSG-D02" || id === "MSG-D13",
    blocked = id === "MSG-D10",
    accepted = id === "MSG-D12" || id === "MSG-D13",
    confirmed = id === "MSG-D07" || id === "MSG-D08";
  const defaultPerson =
    id === "MSG-D03" || id === "MSG-D12"
      ? "Arden"
      : id === "MSG-D04"
        ? "Nia"
        : id === "MSG-D05"
          ? "Lena"
          : "Mira";
  const [name, setName] = useState(
    id === "MSG-D01"
      ? sessionStorage.getItem("sw-review-contact") || defaultPerson
      : defaultPerson,
  );
  const [q, search] = useState(id === "MSG-D11" ? "zzzz" : "Nia");
  const [menu, setMenu] = useState(
    ["MSG-D04", "MSG-D06", "MSG-D10"].includes(id),
  );
  const [draft, setDraft] = useState(
    sessionStorage.getItem("sw-review-draft-" + name) ??
      (blocked || id === "MSG-D09" ? "Ready when you are." : ""),
  );
  const [messages, send] = useState<string[]>([]);
  const [declined, decline] = useState<string[]>([]);
  const [delivery, check] = useState(id === "MSG-D09");
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    function key(e: KeyboardEvent) {
      if (e.key === "Escape" && !confirmed) {
        if (menu) setMenu(false);
        else go("DL-01");
      }
    }
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [confirmed, go, menu]);
  function person(n: string, target: string) {
    sessionStorage.setItem("sw-review-contact", n);
    if (target === id) {
      setName(n);
      setDraft(sessionStorage.getItem("sw-review-draft-" + n) ?? "");
      send([]);
    } else go(target);
  }
  const friends = accepted
    ? ["Mira", "Sol", "Lena", "Arden"]
    : ["Mira", "Sol", "Lena"];
  const body = (n: string, text: string, target: string) => (
    <button
      key={n}
      className="soc-person soc-contact"
      onClick={() => person(n, target)}
    >
      <Avatar name={n} size={58} />
      <span>
        {n}
        <small>{text}</small>
      </span>
    </button>
  );
  const avatar = <Avatar name={name} size={40} />;
  const msg = (text: string, outgoing = false) => (
    <ChatMessage
      surface="direct"
      author={outgoing ? "You" : name}
      body={text}
      timestamp={outgoing ? "2:19 PM" : "2:14 PM"}
      outgoing={outgoing}
      avatar={avatar}
    />
  );
  const ownDraft = (s: string) => {
    setDraft(s);
    sessionStorage.setItem("sw-review-draft-" + name, s);
  };
  function eventProps(kind: string, target: string) {
    if (blocked) return { state: "blocked" as const };
    if (declined.includes(kind))
      return { state: "resolved" as const, title: "Declined" };
    return {
      state: "available" as const,
      onPrimary: () => go(target),
      onDecline: () => decline([...declined, kind]),
    };
  }
  return (
    <Shell area="Collection" current="DL-01" balance={1300} go={go}>
      <Decks go={go} />
      <aside className="soc-inbox">
        <PeopleRail
          friends={friends.map((n) => ({
            id: n,
            name: n,
            artwork: <img src={art[n.toLowerCase()]} alt="" />,
          }))}
          requests={
            id === "MSG-D05"
              ? null
              : {
                  attentionCount:
                    id === "MSG-D13"
                      ? 0
                      : find || id === "MSG-D04" || id === "MSG-D12"
                        ? 1
                        : 2,
                }
          }
          selected={
            find
              ? { kind: "find" }
              : requests || id === "MSG-D04" || id === "MSG-D03"
                ? { kind: "requests" }
                : { kind: "friend", id: name }
          }
          onSelect={(dest) => {
            if (dest.kind === "friend")
              person(
                dest.id,
                dest.id === "Lena"
                  ? "MSG-D05"
                  : dest.id === "Arden"
                    ? "MSG-D12"
                    : "MSG-D01",
              );
            else
              go(
                dest.kind === "find"
                  ? "MSG-S01"
                  : accepted
                    ? "MSG-D13"
                    : "MSG-D02",
              );
          }}
          focusThreadHeading={() => heading.current?.focus()}
        />
        <div className="soc-thread">
          <header>
            <div>
              <h2 ref={heading} tabIndex={-1}>
                {find ? "Find People" : requests ? "Requests" : name}
              </h2>
              {!find && !requests && (
                <small>
                  {blocked
                    ? "⊘ Blocked"
                    : id === "MSG-D03"
                      ? "Not a friend yet"
                      : id === "MSG-D04"
                        ? "Player"
                        : "Friend"}
                </small>
              )}
            </div>
            {!find && !requests && (
              <button
                className="soc-x"
                aria-label="Person menu"
                aria-expanded={menu}
                onClick={() => setMenu(!menu)}
              >
                <MoreVertical />
              </button>
            )}
            <button
              className="soc-x"
              aria-label="Close Messages"
              onClick={() => go("DL-01")}
            >
              <X />
            </button>
          </header>
          {menu && !confirmed && (
            <div className="soc-pop">
              {(blocked
                ? ["Remove friend", "Unblock"]
                : id === "MSG-D04"
                  ? ["Add friend", "Block"]
                  : ["Remove friend", "Block"]
              ).map((action) => (
                <button
                  key={action}
                  onClick={() => {
                    setMenu(false);
                    if (action === "Remove friend") go("MSG-D07");
                    else if (action === "Block") go("MSG-D08");
                    else if (action === "Unblock") go("MSG-D01");
                    else decline([...declined, "Requested"]);
                  }}
                >
                  {action}
                </button>
              ))}
            </div>
          )}
          {find ? (
            <>
              <Search label="Find People" value={q} set={search} />
              {q.trim()
                ? ["Nia", "Nia Storm"]
                    .filter((n) => n.toLowerCase().includes(q.toLowerCase()))
                    .map((n, i) =>
                      body(
                        n,
                        i ? "Friend" : "Player",
                        i ? "MSG-D01" : "MSG-D04",
                      ),
                    )
                : null}
              {q &&
                !["nia", "nia storm"].some((n) =>
                  n.includes(q.toLowerCase()),
                ) && (
                  <p className="soc-quiet">
                    No people found.<small>Try another name.</small>
                  </p>
                )}
            </>
          ) : requests ? (
            <>
              {!accepted && body("Arden", "Friend request", "MSG-D03")}
              {body("Nia", "Good game. Want a rematch?", "MSG-D04")}
            </>
          ) : (
            <>
              <div className="soc-history">
                {id === "MSG-D05" ? (
                  <p className="soc-quiet">Start a conversation.</p>
                ) : id === "MSG-D03" || id === "MSG-D12" ? (
                  <>
                    {msg("Great game earlier! Want to play again sometime?")}
                    {accepted ? (
                      <>
                        <ConversationEvent
                          kind="friend"
                          state="resolved"
                          title="You’re now friends."
                        />
                        <ConversationEvent
                          kind="game"
                          format="Modern"
                          arrangement="2v2"
                          state="resolved"
                          title="Game invite"
                          detail="Invitation expired."
                        />
                      </>
                    ) : (
                      <ConversationEvent
                        kind="friend"
                        {...eventProps("friend", "MSG-D12")}
                      />
                    )}
                  </>
                ) : id === "MSG-D04" ? (
                  msg("Good game. Want a rematch?")
                ) : (
                  <>
                    <small className="soc-date">Today, Jan 26</small>
                    {msg("Would you trade these?")}
                    <ConversationEvent
                      kind="trade"
                      {...eventProps("trade", "TR-D01")}
                    />
                    {msg("Let me take a look.", true)}
                    {msg("Want to play while you decide?")}
                    <ConversationEvent
                      kind="game"
                      format="Modern"
                      arrangement="2v2"
                      {...eventProps("game", "L-02-member")}
                    />
                  </>
                )}
                {messages.map((m, i) => (
                  <div key={i}>{msg(m, true)}</div>
                ))}
              </div>
              {delivery && (
                <div className="soc-line">
                  Couldn’t confirm delivery status.{" "}
                  <Btn
                    onClick={() => {
                      check(false);
                      send([...messages, draft]);
                      ownDraft("");
                    }}
                  >
                    Check delivery
                  </Btn>
                </div>
              )}
              <ChatComposer
                {...(blocked
                  ? { mode: "blocked" as const }
                  : {
                      mode: "editable" as const,
                      maxCharacters: 2000,
                      pending: delivery,
                      onChange: ownDraft,
                      onSend: (s: string) => {
                        send([...messages, s]);
                        ownDraft("");
                      },
                    })}
                surface="direct"
                value={draft}
                placeholder={"Message " + name + "…"}
              />
            </>
          )}
        </div>
      </aside>
      {confirmed && (
        <Window
          title={
            id === "MSG-D07"
              ? "Remove " + name + " from friends?"
              : "Block " + name + "?"
          }
          close={() => go("MSG-D06")}
        >
          <div className="soc-actions">
            <Btn onClick={() => go("MSG-D06")}>Cancel</Btn>
            <Btn
              danger={id === "MSG-D08"}
              onClick={() => go(id === "MSG-D08" ? "MSG-D10" : "MSG-D04")}
            >
              {id === "MSG-D08" ? "Block" : "Remove friend"}
            </Btn>
          </div>
        </Window>
      )}
    </Shell>
  );
}
