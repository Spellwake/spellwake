import { useRef, useState } from "react";
import { PeopleRail, type PeopleDestination } from "@spellwake/ui";
import { originalUrl, references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

const states = {
  "MSG01:rail": {
    selected: { kind: "friend", id: "Mira" },
    count: 2,
    x: 1128,
    centers: [157, 238, 313],
  },
  "MSG02:rail": {
    selected: { kind: "requests" },
    count: 2,
    x: 1129,
    centers: [157, 241, 324],
  },
  "MSGS01:rail": {
    selected: { kind: "find" },
    count: 1,
    x: 1128,
    centers: [157, 239, 319],
  },
  "MSG05:rail": {
    selected: { kind: "friend", id: "Lena" },
    count: null,
    x: 1128,
    centers: [157, 238, 313],
  },
  "MSG12:rail": {
    selected: { kind: "friend", id: "Arden" },
    count: 1,
    x: 1128,
    centers: [157, 237, 313, 384],
  },
  "MSG13:rail": {
    selected: { kind: "requests" },
    count: 0,
    x: 1129,
    centers: [157, 241, 324, 400],
  },
} as const;
type RailView = keyof typeof states;
export function isRailView(view: View): view is RailView {
  return view in states;
}
export const railUses: ControlUse[] = [
  {
    name: "Friends rail",
    id: "people-rail",
    mapSection: "component",
    description:
      "Friend conversations, Requests and Find People in the messaging panel.",
    screens: Object.keys(states) as RailView[],
  },
  {
    name: "Attention badge",
    id: "people-list",
    mapSection: "component",
    description:
      "A count of people needing attention on Requests. A dot marks Nia’s unread conversation in the requests list.",
    screens: ["MSG02:rail", "MSGS01:rail", "MSG13:rail", "MSG-D02:identity"],
  },
];

export function PeopleRailSpecimen({ view }: { view: RailView }) {
  const initial = states[view],
    [, , width, height] = references[view].crop;
  const names = ["Mira", "Sol", "Lena", "Arden"];
  const [selected, select] = useState<PeopleDestination | null>(
    initial.selected,
  );
  const [niaRead, setNiaRead] = useState(false);
  const destination =
    selected === null
      ? "Nia"
      : selected.kind === "friend"
        ? selected.id
        : selected.kind === "find"
          ? "Find People"
          : "Requests";
  const heading = useRef<HTMLHeadingElement>(null);
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene people-stage"
          style={{ width, height }}
          data-specimen
        >
          <PeopleRail
            friends={initial.centers.map((y, i) => ({
              id: names[i]!,
              name: names[i]!,
              artwork: (
                <span
                  className="people-portrait"
                  style={{
                    backgroundImage: `url("${originalUrl(view)}")`,
                    backgroundPosition: `${29 - initial.x}px ${29 - y}px`,
                  }}
                />
              ),
            }))}
            selected={selected}
            requests={
              initial.count === null
                ? null
                : { attentionCount: niaRead ? 0 : initial.count }
            }
            onSelect={select}
            focusThreadHeading={() => heading.current?.focus()}
          />
        </div>
      </div>
      <h3
        ref={heading}
        tabIndex={-1}
        className="people-destination"
        aria-live="polite"
      >
        {destination}
      </h3>
      {view === "MSG12:rail" && (
        <button
          type="button"
          onClick={() => {
            setNiaRead(!niaRead);
            select(niaRead ? initial.selected : null);
          }}
        >
          {niaRead ? "Reset example" : "Read Nia’s message"}
        </button>
      )}
    </>
  );
}
