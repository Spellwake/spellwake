import { useState } from "react";
import {
  RankIdentity,
  RankProgress,
  LoadingState,
  LoadingShape,
  type RankIdentityProps,
} from "@spellwake/ui";
import type { ControlUse } from "./UsageMap";
import { references, type View } from "./references";

import bronze from "../../../assets/rank-icons/bronze.svg?inline";
import silver from "../../../assets/rank-icons/silver.svg?inline";
import gold from "../../../assets/rank-icons/gold.svg?inline";
import platinum from "../../../assets/rank-icons/platinum.svg?inline";
import diamond from "../../../assets/rank-icons/diamond.svg?inline";
import mythic from "../../../assets/rank-icons/mythic.svg?inline";

// Approved rows identify the tier only. Use its authored base symbol.
const symbols = {
  Bronze: bronze,
  Silver: silver,
  Gold: gold,
  Platinum: platinum,
  Diamond: diamond,
  Mythic: mythic,
} as const;
interface RankExample {
  tier: keyof typeof symbols;
  presentation: RankIdentityProps["presentation"];
  position: readonly [number, number];
  rating?: number;
  next?: { tier: string; lower: number; upper: number; width: number };
}
const ranks = {
  "LS-01:silver": {
    tier: "Silver",
    presentation: "season",
    rating: 1240,
    position: [502, 316],
    next: { tier: "Gold", lower: 1100, upper: 1300, width: 272 },
  },
  "LS-01:gold": {
    tier: "Gold",
    presentation: "season",
    rating: 1420,
    position: [502, 420],
    next: { tier: "Platinum", lower: 1300, upper: 1500, width: 274 },
  },
  "LS-01:diamond": {
    tier: "Diamond",
    presentation: "season",
    rating: 1820,
    position: [502, 619],
    next: { tier: "Mythic", lower: 1700, upper: 1900, width: 274 },
  },
  "LS-04:platinum": {
    tier: "Platinum",
    presentation: "season",
    rating: 1620,
    position: [503, 631],
    next: { tier: "Diamond", lower: 1500, upper: 1700, width: 275 },
  },
  "LS-02:mythic": {
    tier: "Mythic",
    presentation: "season",
    rating: 2040,
    position: [502, 313],
  },
  "LS-02:reset": {
    tier: "Silver",
    presentation: "season",
    rating: 1120,
    position: [502, 408],
    next: { tier: "Gold", lower: 1100, upper: 1300, width: 260 },
  },
  "LS-03:first": {
    tier: "Bronze",
    presentation: "season",
    rating: 1000,
    position: [502, 314],
    next: { tier: "Silver", lower: 0, upper: 1100, width: 270 },
  },
  "ST-01:rank": {
    tier: "Mythic",
    presentation: "standings",
    position: [847, 375],
  },
  "PP-01:rank": {
    tier: "Mythic",
    presentation: "profile",
    position: [595, 551],
  },
} satisfies Partial<Record<View, RankExample>>;
const seasonViews: View[] = [
  "LS-01:silver",
  "LS-01:gold",
  "LS-01:diamond",
  "LS-04:platinum",
  "LS-02:mythic",
  "LS-02:reset",
  "LS-03:first",
];
export const rankUses: ControlUse[] = [
  {
    id: "rank-identity",
    name: "Rank identity and progress",
    description:
      "Season rows include rating and tier progress. Standings and public profiles use the compact tier identity beside a separate rating column.",
    approvedCount: 10,
    screens: [...seasonViews, "ST-01:rank", "PP-01:rank"],
  },
  {
    id: "loading",
    name: "Loading state",
    description:
      "The season table reserves space for ranks and progress. Public profiles reserve the portrait and name.",
    approvedCount: 40,
    screens: ["LS-05:loading", "PP-04:loading"],
  },
];

function RankRow({ rank, view }: { rank: RankExample; view: View }) {
  const [x, y] = rank.position;
  const [left, top] = references[view].crop;
  const next = rank.next;
  return (
    <>
      <div
        className="rank-sample-identity"
        style={{ left: x - left, top: y - top }}
      >
        <RankIdentity
          tier={rank.tier}
          presentation={rank.presentation}
          {...(rank.rating === undefined
            ? {}
            : { ratingLabel: "Rating " + rank.rating.toLocaleString("en-US") })}
          emblemSrc={symbols[rank.tier]}
        />
      </div>
      {rank.presentation === "season" && rank.rating !== undefined && (
        <div
          className="rank-sample-progress"
          style={{
            width: next?.width,
            left: 794 - left,
            top: next ? "calc(50% + 6px)" : "calc(50% - 3px)",
          }}
        >
          {next ? (
            <RankProgress
              rating={rank.rating}
              lowerBound={next.lower}
              nextBound={next.upper}
              label={`${next.upper - rank.rating} to ${next.tier}`}
            />
          ) : (
            <RankProgress highest label="Highest rank" />
          )}
        </div>
      )}
    </>
  );
}

function LoadingSample({ profile }: { profile: boolean }) {
  return (
    <LoadingState
      label={profile ? "Loading player profile" : "Loading season ranks"}
    >
      {profile ? (
        <div className="rank-loading-profile">
          <LoadingShape shape="portrait" width={110} height={110} />
          <LoadingShape tone="bright" width={260} height={34} />
        </div>
      ) : (
        <div className="rank-loading-table">
          {[0, 1, 2, 3].map((i) => (
            <div className="rank-loading-row" key={i}>
              <LoadingShape shape="emblem" width={35} height={35} />
              <LoadingShape width={94} height={18} />
              <LoadingShape width={224} height={18} />
            </div>
          ))}
        </div>
      )}
    </LoadingState>
  );
}
function isRankView(view: View): view is keyof typeof ranks {
  return Object.hasOwn(ranks, view);
}
export function RankSpecimen({ view }: { view: View }) {
  const [run, replay] = useState(0);
  const [, , width, height] = references[view].crop;
  return (
    <>
      <div className="popup-stage">
        <div
          data-specimen
          className="popup-scene rank-scene"
          data-view={view}
          style={{ width, height }}
          key={run}
        >
          {isRankView(view) ? (
            <RankRow rank={ranks[view]} view={view} />
          ) : (
            <LoadingSample key={view} profile={view === "PP-04:loading"} />
          )}
        </div>
      </div>
      <div className="readout">
        <button type="button" onClick={() => replay((n) => n + 1)}>
          Replay motion
        </button>
      </div>
    </>
  );
}
