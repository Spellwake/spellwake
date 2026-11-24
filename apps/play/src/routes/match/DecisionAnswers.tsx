import { useState, type ReactElement, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Check, Minus, Plus } from "lucide-react";
import { ActionButton } from "@spellwake/ui";
import {
  playerTargetKey,
  type PlayerChoice,
  type PlayerDecision,
  type PlayerTarget,
} from "@spellwake/application-contracts";
import {
  attackCostPayment,
  counterLeft,
  counterReply,
  counterStep,
  modesReady,
  modesReply,
  modeTargetOpen,
  moved,
  preventionLeft,
  preventionReply,
  preventionStep,
  targetListReady,
  toggledMode,
  toggledModeTarget,
  toggledTarget,
  type CounterDraft,
  type ModesDraft,
  type PreventionDraft,
} from "../../match/decision-drafts";
import * as s from "./match.css";

type Domain<Kind extends PlayerDecision["domain"]["kind"]> = Extract<
  PlayerDecision["domain"],
  { kind: Kind }
>;
type Shared = {
  /** What the question is about, where the game says. */
  subject: string | undefined;
  pending: boolean;
  onChoose: (response: PlayerChoice) => void;
};
type Names = {
  nameOf: (handle: string) => string;
  labelOf: (target: PlayerTarget) => string;
};

/** Counter kinds as printed on cards. */
const COUNTER_NAMES: Record<string, string> = {
  "plus-one": "+1/+1",
  "minus-one": "−1/−1",
};
const counterName = (kind: string): string =>
  COUNTER_NAMES[kind] ?? kind.charAt(0).toUpperCase() + kind.slice(1);
const plural = (count: number, one: string, many = `${one}s`): string =>
  `${count} ${count === 1 ? one : many}`;
const range = (least: number, most: number): string =>
  least === most
    ? String(most)
    : least === 0
      ? `up to ${most}`
      : `${least} to ${most}`;

function Card({
  label,
  title,
  subject,
  children,
}: {
  label: string;
  title: string;
  subject: string | undefined;
  children: ReactNode;
}): ReactElement {
  return (
    <div className={s.decisionCard} role="region" aria-label={label}>
      <p className={s.decisionTitle}>
        {title}
        {subject ? ` · ${subject}` : ""}
      </p>
      {children}
    </div>
  );
}

/** One amount with a button either side, said as "2 of 5". */
function Stepper({
  label,
  value,
  most,
  canAdd,
  disabled,
  onStep,
}: {
  label: string;
  value: number;
  most: number;
  canAdd: boolean;
  disabled: boolean;
  onStep: (delta: 1 | -1) => void;
}): ReactElement {
  return (
    <div className={s.decisionRow} role="group" aria-label={label}>
      <span className={s.decisionLabel}>{label}</span>
      <span className={s.decisionControls}>
        <ActionButton
          aria-label={`One fewer: ${label}`}
          icon={<Minus />}
          disabled={disabled || value <= 0}
          onClick={() => onStep(-1)}
        />
        <span className={s.decisionAmount} aria-live="polite">
          {value} of {most}
        </span>
        <ActionButton
          aria-label={`One more: ${label}`}
          icon={<Plus />}
          disabled={disabled || !canAdd || value >= most}
          onClick={() => onStep(1)}
        />
      </span>
    </div>
  );
}

function Ordering({
  domain,
  kind,
  nameOf,
  subject,
  pending,
  onChoose,
}: { domain: Domain<"ordering">; kind: PlayerDecision["kind"] } & Names &
  Shared): ReactElement {
  const [order, setOrder] = useState<string[]>(() => [...domain.objects]);
  const described = new Map(
    domain.labels.map((entry) => [entry.handle, entry.label]),
  );
  const title =
    kind === "order-triggers"
      ? "Order your triggers"
      : kind === "order-graveyard" || kind === "order-resolution-graveyard"
        ? "Order these cards"
        : "Choose the order";
  return (
    <Card label="Choose the order" title={title} subject={subject}>
      <ol className={s.decisionList}>
        {order.map((handle, index) => {
          const name = described.get(handle) ?? nameOf(handle);
          return (
            <li key={handle} className={s.decisionRow}>
              <span className={s.decisionLabel}>
                {index + 1}. {name}
              </span>
              <span className={s.decisionControls}>
                <ActionButton
                  aria-label={`Move ${name} earlier`}
                  icon={<ArrowUp />}
                  disabled={pending || index === 0}
                  onClick={() => setOrder(moved(order, index, index - 1))}
                />
                <ActionButton
                  aria-label={`Move ${name} later`}
                  icon={<ArrowDown />}
                  disabled={pending || index === order.length - 1}
                  onClick={() => setOrder(moved(order, index, index + 1))}
                />
              </span>
            </li>
          );
        })}
      </ol>
      <div className={s.decisionRow}>
        <ActionButton
          emphasis="primary"
          disabled={pending}
          onClick={() => onChoose({ kind: "ordering", ordered: order })}
        >
          Confirm order
        </ActionButton>
      </div>
    </Card>
  );
}

function TargetList({
  domain,
  kind,
  labelOf,
  subject,
  pending,
  onChoose,
}: { domain: Domain<"target-list">; kind: PlayerDecision["kind"] } & Names &
  Shared): ReactElement {
  const [chosen, setChosen] = useState<PlayerTarget[]>([]);
  const least = Number(domain.minimum);
  const most = Number(domain.maximum);
  const what = kind === "entry-players" ? "players" : "targets";
  return (
    <Card
      label="Choose targets"
      title={
        kind === "proliferate"
          ? "Choose what to proliferate"
          : `Choose ${range(least, most)} ${what}`
      }
      subject={subject}
    >
      <div className={s.decisionRow}>
        {domain.targets.map((target) => {
          const key = playerTargetKey(target);
          const isChosen = chosen.some(
            (other) => playerTargetKey(other) === key,
          );
          return (
            <ActionButton
              key={key}
              aria-pressed={isChosen}
              {...(isChosen ? { icon: <Check /> } : {})}
              disabled={pending || (!isChosen && chosen.length >= most)}
              onClick={() => setChosen(toggledTarget(domain, chosen, target))}
            >
              {labelOf(target)}
            </ActionButton>
          );
        })}
      </div>
      <div className={s.decisionRow}>
        <ActionButton
          emphasis="primary"
          disabled={pending || !targetListReady(domain, chosen)}
          onClick={() => onChoose({ kind: "target-list", targets: chosen })}
        >
          {chosen.length === 0 ? "Choose none" : `Confirm ${chosen.length}`}
        </ActionButton>
      </div>
    </Card>
  );
}

function CounterRemoval({
  domain,
  nameOf,
  subject,
  pending,
  onChoose,
}: { domain: Domain<"counter-removal"> } & Names & Shared): ReactElement {
  const [draft, setDraft] = useState<CounterDraft>({});
  const left = counterLeft(domain, draft);
  const removing = domain.maximum - left;
  return (
    <Card
      label="Remove counters"
      title={`Remove up to ${plural(domain.maximum, "counter")} from ${nameOf(domain.permanent)}`}
      subject={subject}
    >
      {domain.available.map((entry) => (
        <Stepper
          key={entry.kind}
          label={`${counterName(entry.kind)} counters`}
          value={draft[entry.kind] ?? 0}
          most={entry.count}
          canAdd={left > 0}
          disabled={pending}
          onStep={(delta) =>
            setDraft(counterStep(domain, draft, entry.kind, delta))
          }
        />
      ))}
      <div className={s.decisionRow}>
        <ActionButton
          emphasis="primary"
          disabled={pending}
          onClick={() => onChoose(counterReply(draft))}
        >
          {removing === 0 ? "Remove none" : `Remove ${removing}`}
        </ActionButton>
      </div>
    </Card>
  );
}

function Prevention({
  domain,
  subject,
  pending,
  onChoose,
}: { domain: Domain<"prevention"> } & Shared): ReactElement {
  const [draft, setDraft] = useState<PreventionDraft>({});
  const left = preventionLeft(domain, draft);
  return (
    <Card
      label="Prevent damage"
      title={`Prevent ${domain.required} damage`}
      subject={subject}
    >
      {domain.attempts.map((attempt) => (
        <Stepper
          key={attempt.attempt}
          label={attempt.label}
          value={draft[attempt.attempt] ?? 0}
          most={Number(attempt.maximum)}
          canAdd={left > 0}
          disabled={pending}
          onStep={(delta) =>
            setDraft(preventionStep(domain, draft, attempt.attempt, delta))
          }
        />
      ))}
      <div className={s.decisionRow}>
        <ActionButton
          emphasis="primary"
          disabled={pending || left !== 0}
          onClick={() => onChoose(preventionReply(draft))}
        >
          {left === 0 ? "Prevent" : `${left} left to place`}
        </ActionButton>
      </div>
    </Card>
  );
}

function TriggerModes({
  domain,
  labelOf,
  subject,
  pending,
  onChoose,
}: { domain: Domain<"trigger-modes"> } & Names & Shared): ReactElement {
  const [draft, setDraft] = useState<ModesDraft>([]);
  return (
    <Card
      label="Choose modes"
      title={`Choose ${range(domain.minimum, domain.maximum)} ${domain.maximum === 1 ? "mode" : "modes"}`}
      subject={subject}
    >
      {domain.modes.map((mode) => {
        const entry = draft.find((chosen) => chosen.mode === mode.index);
        const name = `Mode ${mode.index + 1}`;
        return (
          <div
            key={mode.index}
            className={s.decisionRow}
            role="group"
            aria-label={name}
          >
            <ActionButton
              aria-pressed={!!entry}
              {...(entry ? { icon: <Check /> } : {})}
              disabled={pending || (!entry && draft.length >= domain.maximum)}
              onClick={() => setDraft(toggledMode(domain, draft, mode.index))}
            >
              {name}
            </ActionButton>
            {entry &&
              mode.targets.map((target) => {
                const key = playerTargetKey(target);
                const isChosen = entry.targets.some(
                  (other) => playerTargetKey(other) === key,
                );
                return (
                  <ActionButton
                    key={key}
                    treatment="compact-form"
                    aria-pressed={isChosen}
                    {...(isChosen ? { icon: <Check /> } : {})}
                    disabled={
                      pending ||
                      (!isChosen &&
                        !modeTargetOpen(domain, draft, mode.index, target))
                    }
                    onClick={() =>
                      setDraft(
                        toggledModeTarget(domain, draft, mode.index, target),
                      )
                    }
                  >
                    {labelOf(target)}
                  </ActionButton>
                );
              })}
          </div>
        );
      })}
      <div className={s.decisionRow}>
        <ActionButton
          emphasis="primary"
          disabled={pending || !modesReady(domain, draft)}
          onClick={() => onChoose(modesReply(domain, draft))}
        >
          Confirm
        </ActionButton>
      </div>
    </Card>
  );
}

function AttackCost({
  domain,
  pool,
  subject,
  pending,
  onChoose,
}: {
  domain: Domain<"attack-cost">;
  pool: readonly { color: string; amount: number }[];
} & Shared): ReactElement {
  const symbols = Number(domain.whitePhyrexian);
  const generic = Number(domain.generic);
  const withMana = attackCostPayment(domain, pool, 0);
  const withLife =
    symbols > 0 ? attackCostPayment(domain, pool, symbols) : null;
  const cost = [
    generic > 0 ? `${generic} mana` : null,
    symbols > 0
      ? `${plural(symbols, "white mana", "white mana")} or ${symbols * 2} life`
      : null,
  ]
    .filter(Boolean)
    .join(" and ");
  return (
    <Card label="Attack cost" title={`Pay ${cost} to attack`} subject={subject}>
      <div className={s.decisionRow}>
        <ActionButton
          emphasis="primary"
          disabled={pending || !withMana}
          onClick={() => withMana && onChoose(withMana)}
        >
          Pay with mana
        </ActionButton>
        {symbols > 0 && (
          <ActionButton
            disabled={pending || !withLife}
            onClick={() => withLife && onChoose(withLife)}
          >
            Pay {symbols * 2} life{generic > 0 ? " and mana" : ""}
          </ActionButton>
        )}
        <ActionButton
          disabled={pending}
          onClick={() => onChoose({ kind: "redeclare-keeping-mana" })}
        >
          Choose attackers again
        </ActionButton>
        <ActionButton
          disabled={pending}
          onClick={() => onChoose({ kind: "decline" })}
        >
          Don’t pay
        </ActionButton>
      </div>
    </Card>
  );
}

/** The kinds of question this file answers. */
export const BUILT_ANSWERS: ReadonlySet<PlayerDecision["domain"]["kind"]> =
  new Set([
    "ordering",
    "target-list",
    "counter-removal",
    "prevention",
    "trigger-modes",
    "attack-cost",
  ]);

/**
 * The questions answered by building something up before sending it: an
 * order, several targets, counters, prevented damage, a trigger's modes and
 * an attack cost. Null for any other question.
 */
export function DecisionAnswer({
  decision,
  pool,
  nameOf,
  labelOf,
  pending,
  onChoose,
}: {
  decision: PlayerDecision;
  /** The answering player's unspent mana. */
  pool: readonly { color: string; amount: number }[];
  pending: boolean;
  onChoose: (response: PlayerChoice) => void;
} & Names): ReactElement | null {
  const domain = decision.domain;
  const shared = { subject: decision.subjectLabel, pending, onChoose };
  const names = { nameOf, labelOf };
  switch (domain.kind) {
    case "ordering":
      return (
        <Ordering domain={domain} kind={decision.kind} {...names} {...shared} />
      );
    case "target-list":
      return (
        <TargetList
          domain={domain}
          kind={decision.kind}
          {...names}
          {...shared}
        />
      );
    case "counter-removal":
      return <CounterRemoval domain={domain} {...names} {...shared} />;
    case "prevention":
      return <Prevention domain={domain} {...shared} />;
    case "trigger-modes":
      return <TriggerModes domain={domain} {...names} {...shared} />;
    case "attack-cost":
      return <AttackCost domain={domain} pool={pool} {...shared} />;
    default:
      return null;
  }
}
