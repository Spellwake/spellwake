import { useRef, useState } from "react";
import { AnimatePresence } from "motion/react";
import { Checkbox, FilterChip } from "@spellwake/ui";
import type { View } from "./references";

function ChipExample({ view }: { view: View }) {
  const initial =
    view === "CLC-03" || view === "CLC-07:chips"
      ? ["M11", "Red", "Instant", "Common", "MV0–3"]
      : view === "CLC-04"
        ? ["Red", "Instant"]
        : ["M11"];
  const [values, setValues] = useState(initial);
  const group = useRef<HTMLDivElement>(null);
  const reset = useRef<HTMLButtonElement>(null);
  function remove(value: string) {
    const buttons = Array.from(
      group.current?.querySelectorAll<HTMLButtonElement>("button:enabled") ??
        [],
    );
    const i = values.indexOf(value);
    if (buttons[i] === document.activeElement)
      (buttons[i + 1] ?? buttons[i - 1] ?? reset.current)?.focus();
    setValues((current) => current.filter((item) => item !== value));
  }
  return (
    <>
      <div className="preference-stage filter-stage">
        <div
          className={"chip-row " + (view === "CLC-03" ? "wide-chips" : "")}
          ref={group}
          data-specimen
        >
          <AnimatePresence initial={false}>
            {values.map((value) => (
              <FilterChip
                key={value}
                treatment={
                  view === "CLC-03"
                    ? "pill"
                    : view === "ML-10"
                      ? "outlined"
                      : "compact"
                }
                label={value}
                onRemove={() => remove(value)}
              />
            ))}
          </AnimatePresence>
        </div>
      </div>
      <div className="readout">
        <span role="status">
          {values.length} active {values.length === 1 ? "filter" : "filters"}
        </span>
        <button type="button" ref={reset} onClick={() => setValues(initial)}>
          Reset example
        </button>
      </div>
    </>
  );
}

const types = [
  "Instant",
  "Creature",
  "Artifact",
  "Enchantment",
  "Land",
  "Planeswalker",
  "Sorcery",
  "Battle",
];
function FilterChoices({ view }: { view: View }) {
  const listing = ["ML-01", "ML-02", "ML-10:own"].includes(view);
  const choices = listing
    ? ["Only my listings"]
    : view === "ML-03"
      ? ["Foundations", "Bloomburrow"]
      : view === "CLC-03:set"
        ? ["Magic 2011"]
        : types;
  const [selected, setSelected] = useState<string[]>(
    listing ? (view === "ML-01" ? [] : choices) : choices.slice(0, 2),
  );
  return (
    <>
      <div className="preference-stage filter-stage">
        <div
          className={view === "CLC-07" ? "type-choices" : "filter-choices"}
          data-specimen
        >
          {choices.map((label) => (
            <Checkbox
              key={label}
              label={
                view === "CLC-03:set" ? (
                  <span>
                    {label}
                    <small>M11</small>
                  </span>
                ) : (
                  label
                )
              }
              className={
                view === "CLC-03:set"
                  ? "set-with-code"
                  : view === "ML-10:own"
                    ? "board-own"
                    : ""
              }
              checked={selected.includes(label)}
              treatment={
                ["ML-03", "CLC-03:set", "ML-10:own"].includes(view)
                  ? "filled"
                  : "outline"
              }
              onCheckedChange={(on) =>
                setSelected((current) =>
                  on ? [...current, label] : current.filter((v) => v !== label),
                )
              }
            />
          ))}
        </div>
      </div>
      <div className="readout" role="status">
        {listing
          ? selected.length
            ? "Showing your listings"
            : "Showing public listings"
          : selected.join(" · ") || "All choices"}
      </div>
    </>
  );
}

const sets = [
  ["FDN", "Foundations"],
  ["MH3", "Modern Horizons 3"],
  ["BLB", "Bloomburrow"],
  ["DSK", "Duskmourn"],
  ["DMR", "Dominaria Remastered"],
  ["ISD", "Innistrad"],
  ["KTK", "Khans of Tarkir"],
  ["KHM", "Kaldheim"],
  ["ELD", "Throne of Eldraine"],
  ["THB", "Theros Beyond Death"],
  ["BRO", "The Brothers’ War"],
] as const;
function SupplyChoices({ view }: { view: View }) {
  const limit = view === "SP-07";
  const closed = view === "SP-09";
  const options = limit ? sets : sets.slice(0, 3);
  const [selected, setSelected] = useState<string[]>(
    sets.slice(0, limit ? 10 : 2).map(([code]) => code),
  );
  return (
    <>
      <div className="preference-stage filter-stage">
        <div
          data-specimen
          className={
            "ballot-tiles " +
            (limit ? "limit-board" : closed ? "closed-board" : "")
          }
        >
          {options.map(([code, label]) => (
            <Checkbox
              key={code}
              className="ballot-tile"
              aria-label={label}
              label={
                <span>
                  <small>{code}</small>
                  <span>
                    {label === "Theros Beyond Death" ? (
                      <>
                        Theros
                        <br />
                        Beyond Death
                      </>
                    ) : (
                      label
                    )}
                  </span>
                </span>
              }
              checked={selected.includes(code)}
              disabled={
                closed || (selected.length === 10 && !selected.includes(code))
              }
              onCheckedChange={(on) =>
                setSelected((current) =>
                  on
                    ? current.length < 10
                      ? [...current, code]
                      : current
                    : current.filter((v) => v !== code),
                )
              }
            />
          ))}
        </div>
      </div>
      <div className="readout" role="status">
        <strong>{selected.length} selected</strong>
        <span>{closed ? "Results pending" : "Ballot draft"}</span>
      </div>
    </>
  );
}
export function FilterSpecimen({ view }: { view: View }) {
  if (view.startsWith("SP-")) return <SupplyChoices view={view} />;
  if (["CLC-03", "CLC-07:chips", "CLC-04", "ML-10"].includes(view))
    return <ChipExample view={view} />;
  return <FilterChoices view={view} />;
}

export const filterUses = [
  {
    name: "Checkbox",
    id: "checkbox",
    description: "Independent filters and set selections on a ballot.",
    screens: [
      "CLC-03:set",
      "CLC-07",
      "ML-01",
      "ML-02",
      "ML-03",
      "ML-10:own",
      "SP-06",
      "SP-07",
      "SP-09",
    ],
  },
  {
    name: "Active filter chip",
    id: "facet-chip",
    description: "Remove one applied set, color, type, rarity, or range.",
    screens: ["CLC-03", "CLC-04", "CLC-07:chips", "ML-10"],
  },
] as const;
