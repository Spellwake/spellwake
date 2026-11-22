import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ChatComposer, ChatMessage, PlayerAvatar } from "@spellwake/ui";
import type { ControlUse } from "./UsageMap";
import { references, originalUrl, type View } from "./references";

export const conversationUses: ControlUse[] = [
  {
    id: "conversation-panel",
    name: "Message composer",
    description:
      "A table draft and an eligible direct conversation can send. Spectators read table chat; a blocked conversation retains its draft.",
    screens: [
      "CM03:composer",
      "CM04:composer",
      "CM11:watching",
      "MSG01:composer",
      "MSG10:blocked",
    ],
  },
  {
    id: "thread",
    mapSection: "component",
    name: "Chat message",
    description:
      "Table chat shows a speaker inline. Direct messages group a portrait or outgoing bubble with its timestamp.",
    screens: [
      "CM04:history",
      "CM11:watching",
      "MSG01:incoming",
      "MSG01:outgoing",
      "MSG04:incoming",
    ],
  },
];
const tableHistory = [
  ["You", "Have fun."],
  ["Sam", "Nice opening."],
  ["Riley", "Your turn, Alex."],
  ["Jordan", "Ready."],
  ["You", "Good game, everyone."],
] as const;
function History({ view }: { view: View }) {
  if (view === "CM04:history" || view === "CM11:watching") {
    const watching = view === "CM11:watching";
    return (
      <div className={watching ? "watch-chat" : "table-history"}>
        {(watching ? tableHistory.slice(-3) : tableHistory).map(
          ([author, body]) => (
            <ChatMessage
              key={body}
              surface="table"
              author={author}
              body={body}
            />
          ),
        )}
        {watching && <ChatComposer mode="watching" />}
      </div>
    );
  }
  const nia = view === "MSG04:incoming",
    outgoing = view === "MSG01:outgoing";
  const portrait = nia ? "1192 256 37 37" : "1190 213 36 36";
  return (
    <div className="direct-row">
      <ChatMessage
        surface="direct"
        outgoing={outgoing}
        author={outgoing ? "You" : nia ? "Nia" : "Mira"}
        timestamp={nia ? "2 min ago" : outgoing ? "2:19 PM" : "2:14 PM"}
        avatar={
          <PlayerAvatar diameter={42}>
            <svg viewBox={portrait} preserveAspectRatio="xMidYMid slice">
              <image href={originalUrl(view)} width={1672} height={941} />
            </svg>
          </PlayerAvatar>
        }
        body={
          nia
            ? "Good game. Want a rematch?"
            : outgoing
              ? "Let me take a look."
              : "Would you trade these?"
        }
      />
    </div>
  );
}

export function ConversationSpecimen({ view }: { view: View }) {
  const table = view.startsWith("CM"),
    blocked = view === "MSG10:blocked",
    isComposer = view.endsWith(":composer") || blocked;
  const [draft, setDraft] = useState(
    view === "CM03:composer"
      ? "Good game, everyone."
      : blocked
        ? "Ready when you are."
        : "",
  );
  const [accepted, setAccepted] = useState<string[]>([]),
    [pending, setPending] = useState(false);
  const timer = useRef<number | undefined>(undefined),
    revision = useRef(0),
    input = useRef<HTMLTextAreaElement>(null);
  const [, , width, height] = references[view].crop;
  useLayoutEffect(() => {
    if (view === "CM03:composer") {
      input.current?.focus({ preventScroll: true });
      const end = input.current?.value.length ?? 0;
      input.current?.setSelectionRange(end, end);
    }
  }, [view]); // Only the draft preset starts focused.
  useEffect(() => () => window.clearTimeout(timer.current), []);
  function send(body: string) {
    if (timer.current !== undefined) return;
    const sentRevision = revision.current;
    setPending(true);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      setPending(false);
      setAccepted((history) => [...history, body]);
      if (revision.current === sentRevision) setDraft("");
    }, 320);
    input.current?.focus({ preventScroll: true });
  }
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene chat-scene"
          data-surface={table ? "table" : "direct"}
          data-example={view}
          data-specimen
          style={{ width, height }}
        >
          {isComposer ? (
            <div className="chat-compose-slot">
              {blocked ? (
                <ChatComposer
                  mode="blocked"
                  surface="direct"
                  value={draft}
                  placeholder="Message Mira..."
                />
              ) : (
                <ChatComposer
                  mode="editable"
                  surface={table ? "table" : "direct"}
                  placeholder={
                    table ? "Message the table..." : "Message Mira..."
                  }
                  inputRef={input}
                  value={draft}
                  onChange={(text) => {
                    revision.current++;
                    setDraft(text);
                  }}
                  pending={pending}
                  onSend={send}
                  maxCharacters={4000}
                />
              )}
            </div>
          ) : (
            <History view={view} />
          )}
        </div>
      </div>
      {isComposer && !blocked && (
        <div
          role="log"
          aria-label="Messages sent in this example"
          className="chat-demo-log"
        >
          {accepted.map((body, i) =>
            table ? (
              <ChatMessage
                key={i}
                surface="table"
                author="You"
                body={body}
                isNew
              />
            ) : (
              <ChatMessage
                key={i}
                surface="direct"
                outgoing
                timestamp="Just now"
                author="You"
                body={body}
                isNew
              />
            ),
          )}
        </div>
      )}
    </>
  );
}
