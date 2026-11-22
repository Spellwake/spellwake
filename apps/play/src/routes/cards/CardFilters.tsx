import { useEffect, useId, useState, type ReactElement } from "react";
import { ChevronDown } from "lucide-react";
import { Checkbox, SelectMenu, TextField } from "@spellwake/ui";
import {
  CARD_COLORS,
  CARD_TYPES,
  COLOR_NAMES,
  RARITIES,
  RARITY_NAMES,
  type CardColor,
  type CardFacets,
  type CardQuery,
  type Rarity,
} from "../../collection/card-sources";
import { manaSymbolUrl } from "../../data/builder-catalogue";
import * as s from "./cards.css";

const ALL_FORMATS = "All formats";
const toggle = <T,>(list: readonly T[], value: T): T[] =>
  list.includes(value)
    ? list.filter((item) => item !== value)
    : [...list, value];
/** A bound typed into the mana value fields: a whole number, or blank for any. */
const readBound = (text: string): number | null | "invalid" => {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  return /^\d{1,2}$/.test(trimmed) ? Number(trimmed) : "invalid";
};

/**
 * The six facets, applied as they change. Facets the source cannot honour are
 * left out. A mana value range whose maximum is below its minimum says so and
 * keeps the last valid results until it is corrected.
 */
export function CardFilters({
  query,
  facets,
  sets,
  onChange,
}: {
  query: CardQuery;
  facets: CardFacets;
  sets: readonly { key: string; name: string; code: string | null }[];
  onChange: (next: Partial<CardQuery>) => void;
}): ReactElement {
  const id = useId();
  const [setSearch, setSetSearch] = useState("");
  const [isTypeOpen, setIsTypeOpen] = useState(false);
  const [min, setMin] = useState(query.manaMin?.toString() ?? "");
  const [max, setMax] = useState(query.manaMax?.toString() ?? "");
  // Reset filters, or a removed chip, clears the typed bounds too.
  useEffect(() => {
    if (query.manaMin === null && query.manaMax === null) {
      setMin((text) => (readBound(text) === null ? text : ""));
      setMax((text) => (readBound(text) === null ? text : ""));
    }
  }, [query.manaMin, query.manaMax]);
  const low = readBound(min);
  const high = readBound(max);
  const rangeError =
    low === "invalid" || high === "invalid"
      ? "Enter a whole number."
      : low !== null && high !== null && high < low
        ? "Maximum must be at least minimum."
        : null;
  const applyRange = (nextMin: string, nextMax: string) => {
    const a = readBound(nextMin);
    const b = readBound(nextMax);
    if (a === "invalid" || b === "invalid") return;
    if (a !== null && b !== null && b < a) return;
    onChange({ manaMin: a, manaMax: b });
  };
  const needle = setSearch.trim().toLowerCase();
  const shownSets = sets.filter(
    (set) =>
      set.name.toLowerCase().includes(needle) ||
      (set.code ?? "").toLowerCase().includes(needle),
  );

  return (
    <>
      {facets.formats && (
        <section className={s.facet} aria-labelledby={`${id}-format`}>
          <h3 className={s.facetLabel} id={`${id}-format`}>
            Format
          </h3>
          <div>
            <SelectMenu
              label="Format"
              value={query.format ?? ALL_FORMATS}
              options={[ALL_FORMATS, ...facets.formats].map((value) => ({
                value,
                label: value,
              }))}
              onValueChange={(value) =>
                onChange({ format: value === ALL_FORMATS ? null : value })
              }
            />
          </div>
        </section>
      )}
      <section className={s.facet} aria-labelledby={`${id}-set`}>
        <h3 className={s.facetLabel} id={`${id}-set`}>
          Set
        </h3>
        <TextField
          type="search"
          label="Find sets"
          placeholder="Find sets…"
          value={setSearch}
          onValueChange={setSetSearch}
          hideLabel
          treatment="compact-form"
        />
        {shownSets.length > 0 ? (
          <div className={s.setList} role="group" aria-label="Sets">
            {shownSets.map((set) => (
              <span key={set.key} className={s.setOption}>
                <Checkbox
                  checked={query.sets.includes(set.key)}
                  label={set.name}
                  onCheckedChange={() =>
                    onChange({ sets: toggle(query.sets, set.key) })
                  }
                />
                {set.code && set.code !== set.name && (
                  <span className={s.setCode}>{set.code}</span>
                )}
              </span>
            ))}
          </div>
        ) : (
          <p className={s.quiet}>No sets found</p>
        )}
      </section>
      {facets.color && (
        <section className={s.facet} aria-labelledby={`${id}-color`}>
          <div className={s.facetHead}>
            <h3 className={s.facetLabel} id={`${id}-color`}>
              Color
            </h3>
            <div className={s.segments} role="radiogroup" aria-label="Match by">
              {(
                [
                  ["color", "Color"],
                  ["identity", "Color identity"],
                ] as const
              ).map(([mode, label]) => (
                <button
                  key={mode}
                  type="button"
                  role="radio"
                  aria-checked={query.colorMode === mode}
                  className={s.segment}
                  onClick={() => onChange({ colorMode: mode })}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
          <div className={s.symbols} role="group" aria-label="Colors">
            {CARD_COLORS.map((color: CardColor) => (
              <button
                key={color}
                type="button"
                className={s.symbol}
                aria-pressed={query.colors.includes(color)}
                aria-label={COLOR_NAMES[color]}
                onClick={() =>
                  onChange({ colors: toggle(query.colors, color) })
                }
              >
                <img src={manaSymbolUrl(color)} alt="" />
              </button>
            ))}
          </div>
          {query.colors.length > 0 && (
            <p className={s.quiet}>Contains any selected color</p>
          )}
          <Checkbox
            checked={query.colorless}
            label="Colorless"
            onCheckedChange={(checked) => onChange({ colorless: checked })}
          />
        </section>
      )}
      {facets.type && (
        <section className={s.facet} aria-labelledby={`${id}-type`}>
          <h3 className={s.facetLabel} id={`${id}-type`}>
            Card type
          </h3>
          <button
            type="button"
            className={s.typeTrigger}
            aria-expanded={isTypeOpen}
            aria-controls={`${id}-types`}
            onClick={() => setIsTypeOpen((open) => !open)}
          >
            <span className={s.typeChosen}>
              {query.types.length > 0 ? (
                query.types.map((type) => (
                  <span key={type} className={s.typePill}>
                    {type}
                  </span>
                ))
              ) : (
                <span className={s.typeAny}>Any type</span>
              )}
            </span>
            <ChevronDown className={s.chevron} aria-hidden="true" />
          </button>
          {isTypeOpen && (
            <div
              className={s.typeList}
              id={`${id}-types`}
              role="group"
              aria-label="Card types"
            >
              {CARD_TYPES.map((type) => (
                <Checkbox
                  key={type}
                  checked={query.types.includes(type)}
                  label={type}
                  onCheckedChange={() =>
                    onChange({ types: toggle(query.types, type) })
                  }
                />
              ))}
            </div>
          )}
        </section>
      )}
      {facets.rarity && (
        <section className={s.facet} aria-labelledby={`${id}-rarity`}>
          <h3 className={s.facetLabel} id={`${id}-rarity`}>
            Rarity
          </h3>
          <div className={s.rarities}>
            {RARITIES.map((rarity: Rarity) => (
              <Checkbox
                key={rarity}
                checked={query.rarities.includes(rarity)}
                label={RARITY_NAMES[rarity]}
                onCheckedChange={() =>
                  onChange({ rarities: toggle(query.rarities, rarity) })
                }
              />
            ))}
          </div>
        </section>
      )}
      {facets.manaValue && (
        <section className={s.facet} aria-labelledby={`${id}-mana`}>
          <h3 className={s.facetLabel} id={`${id}-mana`}>
            Mana value
          </h3>
          <div className={s.range}>
            <TextField
              label="Min"
              placeholder="Any"
              inputMode="numeric"
              value={min}
              onValueChange={(value) => {
                setMin(value);
                applyRange(value, max);
              }}
              treatment="compact-form"
              {...(rangeError ? { "aria-invalid": true } : {})}
            />
            <TextField
              label="Max"
              placeholder="Any"
              inputMode="numeric"
              value={max}
              onValueChange={(value) => {
                setMax(value);
                applyRange(min, value);
              }}
              treatment="compact-form"
              {...(rangeError ? { "aria-invalid": true } : {})}
            />
          </div>
          {rangeError && (
            <p className={s.rangeError} role="alert">
              {rangeError}
            </p>
          )}
        </section>
      )}
    </>
  );
}
