import { useEffect, useRef, useState, type RefObject } from "react";
import { ActionButton, Checkbox, SelectMenu, TextField } from "@spellwake/ui";
import { X } from "lucide-react";
import {
  colors,
  sets,
  formats,
  cardTypes,
  rarities,
  toggleFacet,
  type Bounds,
  rangeError,
  emptyFilters,
  type Filters,
} from "./card-inventory";
import { Search } from "./common";
import white from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/W.svg?inline";
import blue from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/U.svg?inline";
import black from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/B.svg?inline";
import red from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/R.svg?inline";
import green from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/G.svg?inline";

export const options = (labels: readonly string[]) =>
  labels.map((label) => ({ label, value: label }));

export function CardFilters({
  filters,
  change,
  close,
  trigger,
}: {
  filters: Filters;
  change: (value: Filters) => void;
  close: (restore: boolean) => void;
  trigger: RefObject<HTMLButtonElement | null>;
}) {
  const [initialSets] = useState(filters.sets);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<Bounds>({
    min: filters.min,
    max: filters.max,
  });
  const panel = useRef<HTMLElement>(null);
  const error = rangeError(draft);
  useEffect(() => {
    setDraft({ min: filters.min, max: filters.max });
  }, [filters.min, filters.max]);
  useEffect(() => {
    function outside(e: PointerEvent) {
      const el = e.target;
      if (
        el instanceof Element &&
        !panel.current?.contains(el) &&
        !trigger.current?.contains(el) &&
        !el.closest("[data-radix-popper-content-wrapper]")
      )
        close(false);
    }
    function escape(e: KeyboardEvent) {
      if (
        e.key === "Escape" &&
        !e.defaultPrevented &&
        !document.querySelector('[role="listbox"]')
      ) {
        const details = panel.current?.querySelector("details[open]");
        if (details instanceof HTMLDetailsElement) {
          details.open = false;
          details.querySelector("summary")?.focus();
        } else close(true);
      }
    }
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [close, trigger]);
  const matchingSets = Object.entries(sets)
    .filter(([code, name]) =>
      (code + " " + name).toLowerCase().includes(query.trim().toLowerCase()),
    )
    .sort(
      (a, b) =>
        Number(initialSets.includes(b[0])) -
          Number(initialSets.includes(a[0])) || a[1].localeCompare(b[1]),
    );
  return (
    <aside
      className="cs-filter cs-library-filter"
      ref={panel}
      id="card-filters"
      aria-label="Card filters"
    >
      <header>
        <h2>Filters</h2>
        <button aria-label="Close filters" onClick={() => close(true)}>
          <X />
        </button>
      </header>
      <div className="cs-filter-body">
        <fieldset>
          <legend>Format</legend>
          <SelectMenu
            label="Format"
            value={filters.format || "All formats"}
            options={options(["All formats", ...formats])}
            onValueChange={(value) =>
              change({
                ...filters,
                format: value === "All formats" ? "" : value,
              })
            }
          />
        </fieldset>
        <fieldset>
          <legend>Set</legend>
          <Search label="Search sets" value={query} onChange={setQuery} />
          <div className="cs-owned-sets">
            {matchingSets.map(([code, name]) => (
              <Checkbox
                key={code}
                label={
                  <>
                    {name}
                    <small>{code}</small>
                  </>
                }
                checked={filters.sets.includes(code)}
                onCheckedChange={() =>
                  change(toggleFacet(filters, "sets", code))
                }
              />
            ))}
            {!matchingSets.length && <small>No sets found</small>}
          </div>
        </fieldset>
        <fieldset>
          <legend>Color</legend>
          <div
            className="cs-color-mode"
            role="group"
            aria-label="Match color by"
          >
            {["Color", "Color identity"].map((name, i) => (
              <button
                key={name}
                aria-pressed={filters.identity === Boolean(i)}
                onClick={() => {
                  if (filters.identity !== Boolean(i))
                    change({ ...filters, identity: Boolean(i), colors: [] });
                }}
              >
                {name}
              </button>
            ))}
          </div>
          <div className="cs-color-picks">
            {colors.map((name, i) => (
              <button
                key={name}
                aria-label={name}
                aria-pressed={filters.colors.includes(name)}
                onClick={() => change(toggleFacet(filters, "colors", name))}
              >
                <img src={[white, blue, black, red, green][i]} alt="" />
              </button>
            ))}
          </div>
          <Checkbox
            label="Colorless"
            checked={filters.colors.includes("Colorless")}
            onCheckedChange={() =>
              change(toggleFacet(filters, "colors", "Colorless"))
            }
          />
        </fieldset>
        <fieldset>
          <legend>Card type</legend>
          <details className="cs-type-menu">
            <summary>{filters.types.join(", ") || "Any card type"}</summary>
            <div>
              {cardTypes.map((type) => (
                <Checkbox
                  key={type}
                  label={type}
                  checked={filters.types.includes(type)}
                  onCheckedChange={() =>
                    change(toggleFacet(filters, "types", type))
                  }
                />
              ))}
            </div>
          </details>
        </fieldset>
        <fieldset>
          <legend>Rarity</legend>
          <div className="cs-checks">
            {rarities.map((v) => (
              <Checkbox
                key={v}
                label={v}
                checked={filters.rarities.includes(v)}
                onCheckedChange={() =>
                  change(toggleFacet(filters, "rarities", v))
                }
              />
            ))}
          </div>
        </fieldset>
        <fieldset>
          <legend>Mana value</legend>
          <div className="cs-bound-inputs">
            {(["min", "max"] as const).map((bound) => (
              <TextField
                key={bound}
                label={bound === "min" ? "Min" : "Max"}
                treatment="compact-form"
                inputMode="numeric"
                placeholder="Any"
                value={draft[bound]}
                aria-invalid={!!error}
                aria-describedby={error ? "card-range-error" : undefined}
                onValueChange={(value) => {
                  const next = { ...draft, [bound]: value };
                  setDraft(next);
                  if (!rangeError(next)) change({ ...filters, ...next });
                }}
              />
            ))}
          </div>
          {error && (
            <small id="card-range-error" role="status">
              {error}
            </small>
          )}
        </fieldset>
      </div>
      <footer>
        <ActionButton
          treatment="compact-form"
          onClick={() => {
            change(emptyFilters);
            setDraft({ min: "", max: "" });
            setQuery("");
          }}
        >
          Reset filters
        </ActionButton>
      </footer>
    </aside>
  );
}
