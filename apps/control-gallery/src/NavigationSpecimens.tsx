import { useState } from "react";
import { railAreaFor, SectionRail } from "./full-review/SectionRail";
import { PanelTabs } from "@spellwake/ui";
import type { ControlUse } from "./UsageMap";
import { references, type View } from "./references";
export const navigationUses: ControlUse[] = [
  {
    id: "section-nav",
    name: "Section navigation",
    approvedCount: 88,
    description:
      "Local destinations in Collection, Market, League, messaging and account screens.",
    screens: [
      "DL-01:rail",
      "CLC-01:rail",
      "CP-01:rail",
      "EX-01:rail",
      "ML-01:rail",
      "SP-01:rail",
      "LS-01:rail",
      "ST-01:rail",
    ],
  },
  {
    id: "tabs",
    name: "Panel tabs",
    approvedCount: 28,
    description:
      "Peer panels in Predictions, League standings, import, deck building, Collection, Trade, cosmetics, listings and lobby rules.",
    screens: ["EX-01:tabs", "PM-01:tabs", "ST-01:tabs", "ST-02:tabs"],
  },
];
const standings = [
  ["Modern", "Silver", "1,240"],
  ["Standard", "Gold", "1,420"],
  ["Commander", "Bronze", "1,040"],
  ["Legacy", "Diamond", "1,820"],
] as const;
function FormatTabs({
  view,
  navigate,
}: {
  view: View;
  navigate: (view: View) => void;
}) {
  const [other, setOther] = useState<{ preset: View; value: string } | null>(
    null,
  );
  const value =
    other?.preset === view
      ? other.value
      : view === "ST-02:tabs"
        ? "Standard"
        : "Modern";
  return (
    <div className="format-example" data-specimen>
      <PanelTabs
        label="Standings format"
        value={value}
        treatment="framed"
        activationMode="manual"
        onValueChange={(next) => {
          if (next === "Modern" || next === "Standard") {
            setOther(null);
            navigate(next === "Modern" ? "ST-01:tabs" : "ST-02:tabs");
          } else setOther({ preset: view, value: next });
        }}
        tabs={standings.map(([format, rank, rating]) => ({
          value: format,
          label: format,
          content: (
            <div className="nav-readout">
              {format} · Your rank · {rank} · {rating}
            </div>
          ),
        }))}
      />
    </div>
  );
}
export function NavigationSpecimen({
  view,
  navigate,
}: {
  view: View;
  navigate: (view: View) => void;
}) {
  if (view.endsWith(":rail")) {
    const area = railAreaFor(view);
    return (
      <div className="preference-stage navigation-stage">
        <div
          data-specimen
          className="rail-example"
          style={{ height: references[view].crop[3] }}
        >
          <SectionRail
            area={area}
            current={view.split(":")[0]!}
            hrefFor={(id) => `?view=${id}:rail`}
            go={(id) => navigate(`${id}:rail` as View)}
          />
        </div>
      </div>
    );
  }
  if (view.startsWith("ST-"))
    return (
      <div className="preference-stage navigation-stage standings-stage">
        <FormatTabs view={view} navigate={navigate} />
      </div>
    );
  return (
    <div className="preference-stage navigation-stage">
      <div data-specimen className="prediction-tabs-example">
        <PanelTabs
          label="Predictions"
          activationMode="manual"
          value={view === "PM-01:tabs" ? "personal" : "explore"}
          onValueChange={(next) =>
            navigate(next === "personal" ? "PM-01:tabs" : "EX-01:tabs")
          }
          tabs={[
            {
              value: "explore",
              label: "Explore",
              content: (
                <div className="nav-readout">
                  Lightning Bolt · Counterspell · Sol Ring
                </div>
              ),
            },
            {
              value: "personal",
              label: "My predictions",
              content: (
                <div className="nav-readout">Your bets · Created by you</div>
              ),
            },
          ]}
        />
      </div>
    </div>
  );
}
