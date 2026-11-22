import { useRef, useState } from "react";
import { EmptyState, FeedbackMessages, type FeedbackItem } from "@spellwake/ui";
import { DeckNoMatches } from "./specimens";
import type { View } from "./references";
import type { ControlUse } from "./UsageMap";

export const emptyStateUse: ControlUse = {
  id: "empty",
  name: "Empty state",
  approvedCount: 29,
  description:
    "Empty collections, trade offers, filtered results, personal lists, standings and table conversations.",
  screens: [
    "CP-05",
    "DL-04:matches",
    "CM-05:empty",
    "TR09:empty-tray",
    "TR09:no-matches",
    "TR09:empty-category",
  ],
};
export const feedbackUses: ControlUse[] = [
  emptyStateUse,
  {
    id: "notice",
    name: "Feedback messages",
    approvedCount: 55,
    description:
      "Read failures, accepted actions and operation updates across Collection, Market, League, account entry and matches.",
    screens: ["BF-08:refresh", "BF-08", "DL-04:read", "CM-05:read"],
  },
];
function BetFeedback({ refreshFailed }: { refreshFailed: boolean }) {
  const replay = useRef<HTMLButtonElement>(null);
  const [hidden, setHidden] = useState<string[]>([]);
  const [run, setRun] = useState(0);
  const notices: FeedbackItem[] = [
    { id: "accepted", tone: "success", text: "Bet placed" },
  ];
  if (refreshFailed)
    notices.push({
      id: "refresh",
      tone: "warning",
      text: "Could not update predictions",
    });
  const visible = notices
    .filter((n) => !hidden.includes(n.id))
    .map((n) => ({
      ...n,
      onDismiss: () => setHidden((ids) => [...ids, n.id]),
    }));
  return (
    <>
      <div className="preference-stage feedback-stage bet-feedback">
        <div
          data-specimen
          className={
            "toast-space " + (refreshFailed ? "pair-space" : "single-space")
          }
        >
          <FeedbackMessages
            key={run}
            label="Prediction notifications"
            items={visible}
            returnFocusRef={replay}
          />
        </div>
      </div>
      <div className="readout">
        <span>
          {refreshFailed
            ? "Bet accepted · Board refresh failed"
            : "Bet accepted"}
        </span>
        <button
          type="button"
          ref={replay}
          onClick={() => {
            setHidden([]);
            setRun((i) => i + 1);
          }}
        >
          Replay example
        </button>
      </div>
    </>
  );
}
export function FeedbackSpecimen({
  view,
  clearFilters,
}: {
  view: View;
  clearFilters: () => void;
}) {
  if (view === "BF-08" || view === "BF-08:refresh")
    return <BetFeedback refreshFailed={view === "BF-08:refresh"} />;
  const chat = view.startsWith("CM-05"),
    packs = view === "CP-05";
  return (
    <div
      className={
        "preference-stage feedback-stage " +
        (chat ? "chat-feedback" : packs ? "packs-feedback" : "library-feedback")
      }
    >
      <div
        data-specimen
        className={
          packs
            ? "empty-packs"
            : view === "DL-04:read"
              ? "toast-space library-toast"
              : "message-example"
        }
      >
        {packs ? (
          <EmptyState
            treatment="panel"
            title="No packs yet"
            description="Your packs will appear here."
          />
        ) : view === "DL-04:matches" ? (
          <DeckNoMatches clearFilters={clearFilters} />
        ) : view === "CM-05:empty" ? (
          <EmptyState treatment="quiet" title="No messages yet." />
        ) : chat ? (
          <FeedbackMessages
            presentation="inline"
            label="Table chat feedback"
            items={[{ id: "read", text: "Couldn’t load chat." }]}
          />
        ) : (
          <FeedbackMessages
            label="Deck library notifications"
            items={[{ id: "read", text: "Couldn’t load your decks." }]}
          />
        )}
      </div>
    </div>
  );
}
