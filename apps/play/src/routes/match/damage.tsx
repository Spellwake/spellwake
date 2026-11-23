import { useEffect, useRef, useState, type ReactElement } from "react";
import { motion, useReducedMotion } from "motion/react";
import type { PlayerDecision } from "@spellwake/application-contracts";
import { targetKey } from "../../match/combat-draft";
import {
  adjustDamage,
  amountsOf,
  assignedOf,
  canConfirmDamage,
  canMinus,
  canPlus,
  damageAssignments,
  reconcileDamage,
  selectSource,
  startDamage,
  type DamageDomain,
  type DamageDraft,
} from "../../match/damage-draft";
import type { ReadyMatch } from "../../match/match-sources";
import { useAnchors } from "./anchors";
import type { CardMark, CombatUi } from "./combat";
import * as page from "./match.css";
import * as s from "./match-table.css";

/** One target's amount and its two buttons, hung from the card or portrait. */
export interface DamagePill {
  key: string;
  /** `card:<handle>` or `seat:<seatId>`. */
  anchor: string;
  /** Who or what receives it, for the buttons' names. */
  name: string;
  /** A player's name, shown above their pill. */
  caption: string | null;
  amount: number;
  minus: (() => void) | null;
  plus: (() => void) | null;
}
export interface DamageSourceView {
  source: string;
  name: string;
  face: string | undefined;
  total: number;
  assigned: number;
  active: boolean;
  select: () => void;
}
export interface DamageUi extends CombatUi {
  pills: readonly DamagePill[];
  sources: readonly DamageSourceView[];
}

interface Held {
  key: string;
  domain: DamageDomain;
  draft: DamageDraft;
}

/**
 * Combat damage being assigned, on the table itself: the active source and
 * its targets rimmed in gold, a pill of minus, amount and plus beside each
 * target, and Confirm, which sends every source together. Nothing is sent
 * before that, and a failed send leaves every amount as it was.
 */
export function useDamage(
  decision: PlayerDecision | null,
  source: ReadyMatch,
  describe: (handle: string) => { name: string; face: string | undefined },
): DamageUi | null {
  const domain =
    decision?.domain.kind === "combat-damage" ? decision.domain : null;
  const key = domain ? (decision?.decision ?? null) : null;
  const [held, setHeld] = useState<Held | null>(null);
  const [changed, setChanged] = useState(false);
  const [sentKey, setSentKey] = useState<string | null>(null);

  // The same decision offered again keeps every split that is still offered as it was.
  useEffect(() => {
    if (!domain || !key || !held || held.key !== key || held.domain === domain)
      return;
    const next = reconcileDamage(held.draft, held.domain, domain);
    setHeld({ key, domain, draft: next.draft });
    if (next.changed) setChanged(true);
  }, [domain, key, held]);
  useEffect(() => {
    setChanged(false);
  }, [key]);

  if (!domain || !key) return null;
  const draft =
    held && held.key === key && held.domain === domain
      ? held.draft
      : held && held.key === key
        ? reconcileDamage(held.draft, held.domain, domain).draft
        : startDamage(domain);
  const edit = (change: (draft: DamageDraft) => DamageDraft) =>
    setHeld((current) => {
      const base =
        current && current.key === key ? current.draft : startDamage(domain);
      return { key, domain, draft: change(base) };
    });

  const { commandPending, commandError, sendAction, seatNames } = source;
  const sending = commandPending && sentKey === key;
  const failed = sentKey === key && !commandPending && Boolean(commandError);
  const active = domain.sources.find((entry) => entry.source === draft.active);

  const marks = new Map<string, CardMark>();
  const seatMarks = new Map<string, "offered" | "assigned" | "marked">();
  for (const entry of domain.sources)
    marks.set(entry.source, entry === active ? "gold" : "eligible");
  const pills: DamagePill[] = [];
  if (active) {
    const amounts = amountsOf(draft, active);
    active.targets.forEach((target, index) => {
      if (target.kind === "stack") return;
      const seat = target.kind === "seat";
      if (seat) seatMarks.set(target.seatId, "marked");
      else marks.set(target.handle, "gold");
      const name = seat
        ? (seatNames.get(target.seatId) ?? "Player")
        : describe(target.handle).name;
      const press = (delta: 1 | -1) => () =>
        edit((current) =>
          adjustDamage(current, domain, active.source, index, delta),
        );
      pills.push({
        key: `${active.source}>${targetKey(target)}`,
        anchor: seat ? `seat:${target.seatId}` : `card:${target.handle}`,
        name,
        caption: seat ? name : null,
        amount: amounts[index] ?? 0,
        minus: !sending && canMinus(draft, active, index) ? press(-1) : null,
        plus: !sending && canPlus(draft, active, index) ? press(1) : null,
      });
    });
  }

  return {
    marks,
    seatMarks,
    lines: [],
    pills,
    sources: domain.sources.map((entry) => ({
      source: entry.source,
      ...describe(entry.source),
      total: Number(entry.total) || 0,
      assigned: assignedOf(draft, entry),
      active: entry === active,
      select: () =>
        edit((current) => selectSource(current, domain, entry.source)),
    })),
    // One of your own sources chooses whose targets carry the controls.
    onCard: (object) => {
      if (!domain.sources.some((entry) => entry.source === object.handle))
        return false;
      edit((current) => selectSource(current, domain, object.handle));
      return true;
    },
    // A portrait keeps moving the camera: its pill is the damage control.
    onSeat: () => false,
    primary: {
      label: "Confirm",
      disabled: !canConfirmDamage(draft, domain),
      pending: sending,
      onActivate: () => {
        setSentKey(key);
        setChanged(false);
        void sendAction({
          kind: "choose",
          decision: key,
          response: {
            kind: "combat-damage",
            assignments: damageAssignments(draft, domain),
          },
        });
      },
    },
    secondary: null,
    status: failed
      ? "Couldn’t assign damage."
      : changed
        ? "Damage assignment changed."
        : null,
  };
}

/** The turn panel, which no pill may cover. */
const SIDE = "frame:hud-side";
/** The players along the top, which cards' pills stay beneath. */
const PLAYERS = "frame:hud-players";
const coarsePointer = (): boolean =>
  typeof window !== "undefined" &&
  window.matchMedia("(pointer: coarse)").matches;

/** Keep neighbouring pills apart, and every pill on the table. */
function spread(centres: number[], pitch: number, low: number, high: number) {
  const right = [...centres];
  for (let i = 1; i < right.length; i += 1)
    right[i] = Math.max(right[i]!, right[i - 1]! + pitch);
  const left = [...centres];
  for (let i = left.length - 2; i >= 0; i -= 1)
    left[i] = Math.min(left[i]!, left[i + 1]! - pitch);
  // Pushed half each way, a crowded group stays centred on its cards.
  const placed = right.map((value, i) => (value + left[i]!) / 2);
  for (let i = placed.length - 1; i >= 0; i -= 1)
    placed[i] = Math.min(
      placed[i]!,
      i === placed.length - 1 ? high : placed[i + 1]! - pitch,
    );
  for (let i = 0; i < placed.length; i += 1)
    placed[i] = Math.max(placed[i]!, i === 0 ? low : placed[i - 1]! + pitch);
  return placed;
}

/**
 * The pills of the source being assigned, each led by a thin line to the exact
 * card or portrait it adds to. A target the camera does not show has no pill;
 * its amount stays in the readout until its board is in view.
 */
export function DamagePills({
  pills,
  follow,
  clearBottom = 0,
}: {
  pills: readonly DamagePill[];
  follow: unknown;
  clearBottom?: number;
}): ReactElement {
  const root = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();
  const { rects, width, height } = useAnchors(
    root,
    [...pills.map((pill) => pill.anchor), SIDE, PLAYERS],
    follow,
  );
  const coarse = coarsePointer();
  const pillWidth = coarse ? 136 : 112;
  const pillHeight = coarse ? 47 : 38;
  const drop = 24;
  const floor = height - clearBottom - 8;

  // Cards' pills stay under the players along the top.
  const ceiling = (rects.get(PLAYERS)?.bottom ?? 0) + 6;
  const hung = pills.flatMap((pill) => {
    const rect = rects.get(pill.anchor);
    if (!rect) return [];
    const centre = rect.left + rect.width / 2;
    const under = rect.bottom + drop + (pill.caption ? 16 : 0);
    const over = rect.top - drop - pillHeight;
    if (pill.caption || under + pillHeight <= floor)
      return [{ pill, centre, top: under, from: rect.bottom, below: true }];
    // With no room beneath its card, a pill sits above it instead.
    if (over >= ceiling)
      return [{ pill, centre, top: over, from: rect.top, below: false }];
    // With no room either way, it sits as low as the table allows.
    const top = Math.max(ceiling, floor - pillHeight);
    return [
      { pill, centre, top, from: Math.min(rect.bottom, top), below: true },
    ];
  });
  // Pills that would touch one another's height share a row, and a row is
  // spread as one group.
  const rows: (typeof hung)[] = [];
  for (const entry of [...hung].sort((a, b) => a.top - b.top)) {
    const row = rows.at(-1);
    if (row && entry.top - row[0]!.top < pillHeight + 8) row.push(entry);
    else rows.push([entry]);
  }
  // A row level with the turn panel stops short of it.
  const side = rects.get(SIDE);
  const placed = rows.flatMap((row) => {
    const ordered = [...row].sort((a, b) => a.centre - b.centre);
    const top = ordered[0]!.top;
    const bottom = Math.max(...ordered.map((entry) => entry.top)) + pillHeight;
    const beside =
      side && side.left > width * 0.3 && top < side.bottom && bottom > side.top;
    const edge = (beside ? side.left : width) - 8 - pillWidth / 2;
    const centres = spread(
      ordered.map((entry) => entry.centre),
      pillWidth + 10,
      8 + pillWidth / 2,
      Math.max(8 + pillWidth / 2, edge),
    );
    return ordered.map((entry, index) => ({ ...entry, x: centres[index]! }));
  });

  return (
    <div ref={root} className={s.pillLayer}>
      <svg className={s.lines} aria-hidden="true" focusable="false">
        {placed.map(({ pill, centre, from, x, top, below }) => (
          <path
            key={pill.key}
            className={s.leader}
            d={`M ${centre.toFixed(1)} ${from.toFixed(1)} L ${x.toFixed(1)} ${(below
              ? top - (pill.caption ? 18 : 0)
              : top + pillHeight
            ).toFixed(1)}`}
          />
        ))}
      </svg>
      {placed.map(({ pill, x, top }) => (
        <motion.div
          key={pill.key}
          role="group"
          aria-label={`Damage to ${pill.name}`}
          className={s.pill}
          style={{ x: x - pillWidth / 2, y: top }}
          initial={reduced ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{
            duration: reduced ? 0 : 0.24,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          {pill.caption && (
            <span className={s.pillCaption} aria-hidden="true">
              {pill.caption}
            </span>
          )}
          <button
            type="button"
            className={s.pillStep}
            aria-label={`Less damage to ${pill.name}`}
            disabled={!pill.minus}
            onClick={() => pill.minus?.()}
          >
            −
          </button>
          <output className={s.pillAmount} aria-label={`${pill.amount} damage`}>
            <motion.span
              key={pill.amount}
              style={{ display: "inline-block" }}
              initial={reduced ? false : { opacity: 0.5, y: 3 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: reduced ? 0 : 0.16 }}
            >
              {pill.amount}
            </motion.span>
          </output>
          <button
            type="button"
            className={s.pillStep}
            aria-label={`More damage to ${pill.name}`}
            disabled={!pill.plus}
            onClick={() => pill.plus?.()}
          >
            +
          </button>
        </motion.div>
      ))}
    </div>
  );
}

const thumb = (face: string | undefined) =>
  face ? { backgroundImage: `url("${face}")` } : undefined;

/**
 * Beside the turn panel: a chip for each source when there are several, then
 * the active source with how much of its damage is assigned.
 */
export function DamageSources({
  sources,
}: {
  sources: readonly DamageSourceView[];
}): ReactElement | null {
  const active = sources.find((entry) => entry.active);
  if (!active) return null;
  return (
    <>
      {sources.length > 1 && (
        <div
          className={page.damageSources}
          role="group"
          aria-label="Damage sources"
        >
          {sources.map((entry) => (
            <button
              key={entry.source}
              type="button"
              className={page.damageChip}
              aria-pressed={entry.active}
              aria-label={`${entry.name}, ${entry.assigned} of ${entry.total} assigned`}
              onClick={entry.select}
            >
              <span
                className={page.damageThumb}
                style={thumb(entry.face)}
                aria-hidden="true"
              />
              <span className={page.damageName}>{entry.name}</span>
              <span className={page.damageCount}>
                {entry.assigned}/{entry.total}
              </span>
            </button>
          ))}
        </div>
      )}
      <div className={page.damageReadout} role="status">
        <span
          className={page.damageThumb}
          style={thumb(active.face)}
          aria-hidden="true"
        />
        <span className={page.damageName}>
          {active.name} · {active.total} damage
        </span>
        <span className={page.damageCount}>
          {active.assigned} / {active.total} assigned
        </span>
      </div>
    </>
  );
}
