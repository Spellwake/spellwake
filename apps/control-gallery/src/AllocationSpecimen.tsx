import { useState } from "react";
import { DamageAssignmentReadout } from "@spellwake/ui";
import { DamagePairControls, type DamagePair } from "./DamagePairControls";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";
export const allocationUses: ControlUse[] = [
  {
    id: "damage-allocation",
    name: "Divided damage readout",
    description:
      "Arc Lightning shows how many of its three points are assigned across the chosen targets.",
    screens: ["DC03:allocation", "DC05:allocation"],
  },
];
export function AllocationSpecimen({ view }: { view: View }) {
  const [values, setValues] = useState<DamagePair>(
    view === "DC05:allocation" ? [1, 1] : [2, 1],
  );
  const [, , width, height] = references[view].crop;
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene allocation-scene divided"
          data-specimen
          style={{ width, height }}
        >
          <DamageAssignmentReadout total={3} assigned={values[0] + values[1]} />
        </div>
      </div>
      <div className="allocation-edits">
        <DamagePairControls
          showNames
          values={values}
          total={3}
          minimum={1}
          names={["Elf", "Grizzly Bears"]}
          onChange={setValues}
        />
      </div>
      <a href="?walkthrough=combat-damage">
        Open shared combat damage assignment ↗
      </a>
    </>
  );
}
