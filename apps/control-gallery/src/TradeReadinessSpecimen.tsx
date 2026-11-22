import {
  TradeReadinessBadge,
  TradeStatusLine,
  type TradeParticipantStatus,
  type TradeRoomStatus,
} from "@spellwake/ui";
import { originalUrl, references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

const badges = {
  "TR01:editing": "editing",
  "TR05:ready": "ready",
  "TR06:confirmed": "confirmed",
} as const satisfies Partial<Record<View, TradeParticipantStatus>>;
const roomStates = {
  "TR05:waiting": "waiting",
  "TR04:both-ready": "ready",
  "TR06:waiting-confirm": "awaitingConfirmation",
} as const satisfies Partial<Record<View, TradeRoomStatus>>;

type BadgeView = keyof typeof badges;
type ReadinessView = BadgeView | keyof typeof roomStates;
function isBadgeView(view: View): view is BadgeView {
  return Object.hasOwn(badges, view);
}
export function isReadinessView(view: View): view is ReadinessView {
  return isBadgeView(view) || Object.hasOwn(roomStates, view);
}

export const readinessUses: ControlUse[] = [
  {
    id: "trade-trays",
    name: "Participant readiness badge",
    mapSection: "component",
    description:
      "Each player's tray shows whether they are editing, ready or have confirmed the trade.",
    screens: ["TR01:editing", "TR05:ready", "TR06:confirmed"],
  },
  {
    id: "trade-consent",
    name: "Trade status line",
    mapSection: "component",
    description:
      "The line beneath both trays shows the current stage of the trade.",
    screens: ["TR05:waiting", "TR04:both-ready", "TR06:waiting-confirm"],
  },
];
export function TradeReadinessSpecimen({ view }: { view: ReadinessView }) {
  const badge = isBadgeView(view);
  const [x, y, width, height] = references[view].crop;
  return (
    <div className="popup-stage">
      <div
        className="popup-scene trade-readiness-stage"
        data-specimen
        style={{
          width,
          height,
          // Use an adjacent strip of room scenery behind the live status line.
          ...(badge
            ? {}
            : {
                backgroundImage: `url("${originalUrl(view)}")`,
                backgroundPosition: `-${x}px -${y + 55}px`,
              }),
        }}
      >
        {badge ? (
          <TradeReadinessBadge status={badges[view]} />
        ) : (
          <TradeStatusLine status={roomStates[view]} />
        )}
      </div>
    </div>
  );
}
