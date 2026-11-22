import { useRef, useState } from "react";
import {
  ConversationEvent,
  tradeClosureTitle,
  type ConversationEventProps,
} from "@spellwake/ui";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

export const eventUses: ControlUse[] = [
  {
    name: "Conversation event card",
    id: "thread-event",
    mapSection: "component",
    description:
      "One card for trade offers, game invitations and friend requests. A blocked thread keeps the decisions disabled. A settled event shows the recorded outcome.",
    screens: [
      "MSG01:trade",
      "MSG01:game",
      "MSG03:friend",
      "MSG10:trade",
      "MSG10:game",
      "MSG12:friend",
      "MSG12:game",
    ],
  },
];

export function ConversationEventSpecimen({
  view,
  openTrade,
  completedTrade,
  closure,
}: {
  view: View;
  openTrade: () => void;
  completedTrade: boolean;
  closure: unknown;
}) {
  const friend = view.endsWith(":friend"),
    game = view.endsWith(":game");
  const initial = view.startsWith("MSG12")
    ? friend
      ? "friends"
      : "expired"
    : undefined;
  const [result, setResult] = useState<
    "friends" | "declined" | "expired" | undefined
  >(initial);
  const scene = useRef<HTMLDivElement>(null);
  const [, , width, height] = references[view].crop;
  const content = game
    ? ({ kind: "game", format: "Modern", arrangement: "2v2" } as const)
    : ({ kind: friend ? "friend" : "trade" } as const);
  function finish(next: "friends" | "declined") {
    setResult(next);
    scene.current?.focus({ preventScroll: true });
  }
  let state: ConversationEventProps;
  if (view === "MSG01:trade" && completedTrade) {
    state = { kind: "trade", state: "resolved", title: "Trade complete." };
  } else if (
    view === "MSG01:trade" &&
    (closure === "canceled" || closure === "unavailable")
  ) {
    state = {
      kind: "trade",
      state: "resolved",
      title: tradeClosureTitle[closure],
    };
  } else if (result) {
    const title =
      result === "friends"
        ? "You’re now friends."
        : friend
          ? "Friend request"
          : game
            ? "Game invite"
            : "Trade offer";
    state = {
      ...content,
      state: "resolved",
      title,
      ...(result === "friends"
        ? {}
        : {
            detail: result === "expired" ? "Invitation expired." : "Declined.",
          }),
    };
  } else if (view.startsWith("MSG10")) {
    state = { ...content, state: "blocked" };
  } else {
    state = {
      ...content,
      state: "available",
      onDecline: () => finish("declined"),
      onPrimary: () => {
        if (friend) finish("friends");
        else if (!game) openTrade();
        else
          window.open(
            "../../lobby-details-2026-11-03/approved/decks-controls/member-no-deck.png",
            "_blank",
            "noopener",
          );
      },
    };
  }
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene event-scene"
          id="conversation-event"
          ref={scene}
          tabIndex={-1}
          role="group"
          aria-label="Conversation event example"
          style={{ width, minHeight: height }}
          data-specimen
        >
          <ConversationEvent {...state} />
        </div>
      </div>
      <div className="event-demo-feedback" aria-live="polite">
        {result &&
          result !== initial &&
          (result === "friends" ? "Friend request accepted." : "Declined.")}
      </div>
      {result !== initial && (
        <button
          type="button"
          onClick={() => {
            setResult(initial);
            scene.current?.focus();
          }}
        >
          Reset example
        </button>
      )}
    </>
  );
}
