import { useEffect, useId, useState, type ReactElement } from "react";
import { SelectMenu, TextField } from "@spellwake/ui";
import type { CardQuery } from "../../collection/card-sources";
import {
  FINISH_LABELS,
  readWhole,
  type Finish,
  type ListingFacets,
  type ListingQuery,
} from "../../market/listing-sources";
import { CardFilters } from "../cards/CardFilters";
import * as s from "../cards/cards.css";

const NO_CARD_FACETS = {
  formats: null,
  color: false,
  type: false,
  rarity: false,
  manaValue: false,
} as const;

/**
 * Listings' filters, applied as they change. Cards reuse Collection's card
 * filters and add Finish; every kind has Set and the total price. Card-only
 * facets show only while Cards is chosen.
 */
export function ListingFilters({
  query,
  facets,
  sets,
  onChange,
}: {
  query: ListingQuery;
  facets: ListingFacets;
  sets: readonly { key: string; name: string; code: string | null }[];
  onChange: (next: Partial<ListingQuery>) => void;
}): ReactElement {
  const id = useId();
  const isCards = query.kind === "cards";
  const [min, setMin] = useState(query.priceMin?.toString() ?? "");
  const [max, setMax] = useState(query.priceMax?.toString() ?? "");
  // Reset filters, or a removed chip, clears the typed bounds too.
  useEffect(() => {
    if (query.priceMin === null && query.priceMax === null) {
      setMin((text) => (readWhole(text) === null ? text : ""));
      setMax((text) => (readWhole(text) === null ? text : ""));
    }
  }, [query.priceMin, query.priceMax]);
  const low = readWhole(min);
  const high = readWhole(max);
  const rangeError =
    low === "invalid" || high === "invalid"
      ? "Enter a whole number."
      : low !== null && high !== null && high < low
        ? "Maximum must be at least minimum."
        : null;
  const applyRange = (nextMin: string, nextMax: string) => {
    const a = readWhole(nextMin);
    const b = readWhole(nextMax);
    if (a === "invalid" || b === "invalid") return;
    if (a !== null && b !== null && b < a) return;
    onChange({ priceMin: a, priceMax: b });
  };
  const cardQuery: CardQuery = { ...query, sort: "name-asc" };

  return (
    <>
      <CardFilters
        query={cardQuery}
        facets={isCards ? facets : NO_CARD_FACETS}
        sets={sets}
        // Card filters never change the sort, which is Listings' own.
        onChange={(next) => onChange(next as Partial<Omit<CardQuery, "sort">>)}
      />
      {isCards && (
        <section className={s.facet} aria-labelledby={`${id}-finish`}>
          <h3 className={s.facetLabel} id={`${id}-finish`}>
            Finish
          </h3>
          <div>
            <SelectMenu
              label="Finish"
              value={query.finish}
              options={(Object.keys(FINISH_LABELS) as Finish[]).map(
                (value) => ({ value, label: FINISH_LABELS[value] }),
              )}
              onValueChange={(value) => onChange({ finish: value as Finish })}
            />
          </div>
        </section>
      )}
      <section className={s.facet} aria-labelledby={`${id}-price`}>
        <h3 className={s.facetLabel} id={`${id}-price`}>
          Total price
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
    </>
  );
}
