import { CombatDamageAssignment, type AssignedDamage } from "@spellwake/ui";
import { useRef, useState, useEffect } from "react";
import table from "../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-13/two-attackers/table.webp?inline";
import { attackers } from "./combat-fixture";
export default function CombatDamageWalkthrough() {
  const [confirmed, setConfirmed] = useState<AssignedDamage[] | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    document.title = "Combat damage · Spellwake";
    if (confirmed) heading.current?.focus();
  }, [confirmed]);
  return (
    <main
      className="combat-preview"
      style={{ backgroundImage: "url(" + table + ")" }}
    >
      {confirmed ? (
        <section className="combat-receipt">
          <h1 ref={heading} tabIndex={-1}>
            Combat damage assigned
          </h1>
          <dl>
            {attackers.map((source, i) => (
              <div key={source.name}>
                <dt>{source.name}</dt>
                <dd>
                  {source.blockers.map((blocker, j) => (
                    <span key={blocker.image.name}>
                      {blocker.image.name}{" "}
                      <b>{confirmed[i]!.targets[j]!.amount}</b>
                    </span>
                  ))}
                </dd>
              </div>
            ))}
          </dl>
          <button
            type="button"
            className="combat-back"
            onClick={() => {
              setConfirmed(null);
            }}
          >
            Replay walkthrough
          </button>
        </section>
      ) : (
        <CombatDamageAssignment
          decisionId="two-attackers"
          attackers={attackers}
          onConfirm={setConfirmed}
        />
      )}
    </main>
  );
}
