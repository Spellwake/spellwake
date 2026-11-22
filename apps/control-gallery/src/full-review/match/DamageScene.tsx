import { useEffect, useState } from "react";
import {
  CombatDamageAssignment,
  CompactSpellStack,
  DamageAssignmentReadout,
  MatchActions,
  OriginalCardFace,
  QuantityControl,
  TargetConnection,
  adjustDamageAllocation,
  type AssignedDamage,
} from "@spellwake/ui";
import { attackers } from "../../combat-fixture";
import { cardImage, portrait, seats } from "./assets";
import { FeedbackBoard } from "./panels";
import "./damage-scene.css";

type Go = (id: string, data?: unknown) => void;
type SceneProps = { id: string; go: Go; data?: unknown };
type Split = { id: string; amount: number }[];
const anchors: Record<string, [x: number, y: number, name: string]> = {
  elf: [637, 481, "Llanowar Elves"],
  bear: [780, 481, "Grizzly Bears"],
  "other-angel": [917, 481, "Serra Angel"],
  angel: [259, 680, "Serra Angel"],
  chandra: [1197, 481, "Chandra Nalaar"],
  thopter: [1541, 680, "Ornithopter"],
  ...Object.fromEntries(
    seats.map((name, i) => [name, [267 + 190 * i, 48, name]]),
  ),
};
function preset(id: string): Split {
  const offsets: Record<string, number[]> = {
    DC02: [3],
    DC03: [2, 1],
    DC04: [1, 2],
    DC05: [1, 1],
    DC06: [1, 1, 1],
    DC08: [2],
    DC10: [2, 1],
    DC11: [2, 1],
  };
  if (id === "DC07") return [{ id: "bear", amount: 1 }];
  if (id === "DC09")
    return [
      { id: "Alex", amount: 2 },
      { id: "chandra", amount: 1 },
    ];
  return (offsets[id] ?? []).map((amount, i) => ({
    id: ["elf", "bear", "other-angel"][i]!,
    amount,
  }));
}
function readSplit(value: unknown): Split | undefined {
  if (!Array.isArray(value) || value.length > 3) return;
  const rows: Split = [];
  for (const row of value) {
    if (
      !row ||
      typeof row.id !== "string" ||
      !Object.hasOwn(anchors, row.id) ||
      rows.some((r) => r.id === row.id) ||
      !Number.isInteger(row.amount) ||
      row.amount < 1
    )
      return;
    rows.push({ id: row.id, amount: row.amount });
  }
  if (rows.reduce((sum, row) => sum + row.amount, 0) <= 3) return rows;
}
function toggle(split: Split, id: string): Split {
  if (!Object.hasOwn(anchors, id)) return split;
  if (split.some((row) => row.id === id))
    return split.filter((row) => row.id !== id);
  if (split.length === 3) return split;
  const free = 3 - split.reduce((n, r) => n + r.amount, 0);
  if (free > 0) return [...split, { id, amount: free }];
  const rows = [...split, { id, amount: 0 }];
  const amounts = adjustDamageAllocation(
    rows.map((row) => row.amount),
    rows.length - 1,
    1,
    3,
    1,
  );
  return rows.map((row, index) => ({ ...row, amount: amounts[index]! }));
}
function Divided({ id, go, data }: SceneProps) {
  const [split, setSplit] = useState<Split>(
      () => readSplit(data) ?? preset(id),
    ),
    [selected, setSelected] = useState<string | null>(null);
  const pending = id === "DC10",
    ready = id === "DC00",
    stack = id === "DC11";
  useEffect(() => {
    if (ready || pending || stack) return;
    const choose = (e: Event) => {
      const key = (e as CustomEvent<unknown>).detail;
      if (typeof key === "string") setSplit((prev) => toggle(prev, key));
    };
    window.addEventListener("spellwake:damage-target", choose);
    return () => window.removeEventListener("spellwake:damage-target", choose);
  }, [ready, pending, stack]);
  useEffect(() => {
    window.dispatchEvent(
      new CustomEvent("spellwake:damage-selection", {
        detail: stack ? [] : split.map((r) => r.id),
      }),
    );
  }, [split, stack]);
  useEffect(() => {
    if (!pending || !readSplit(data)) return;
    const timer = setTimeout(() => go("DC11", data), 1200);
    return () => clearTimeout(timer);
  }, [pending, data, go]);
  const assigned = split.reduce((n, r) => n + r.amount, 0);
  if (ready)
    return (
      <div className="sw-damage-actions">
        <MatchActions
          primary={{ label: "Next", onActivate: () => go("NP-01") }}
        />
      </div>
    );
  if (stack)
    return (
      <>
        <div className="sw-damage-stack">
          <CompactSpellStack
            items={[
              {
                kind: "spell",
                id: "arc",
                image: cardImage("arc"),
                controller: "You",
                portrait: <img src={portrait(0)} alt="" />,
              },
            ]}
            selectedId={selected}
            onSelect={setSelected}
          />
        </div>
        {selected &&
          split.map((row) => {
            const [x, y] = anchors[row.id]!;
            return (
              <TargetConnection
                key={row.id}
                from={{ x: 1264, y: 350 }}
                to={{ x, y: seats.includes(row.id) ? y : y + 53 }}
                bend={-18}
              />
            );
          })}
      </>
    );
  return (
    <>
      <div className="sw-damage-actions">
        <MatchActions
          layout="payment"
          primary={{
            label: "Confirm",
            pending,
            disabled: assigned !== 3,
            onActivate: () => go("DC10", split),
          }}
          secondary={{ label: "Cancel", onActivate: () => go("DC00") }}
        />
      </div>
      <div className="sw-divide-total">
        <DamageAssignmentReadout total={3} assigned={assigned} />
      </div>
      <div className="sw-divide-source">
        <OriginalCardFace image={cardImage("arc")} />
      </div>
      {split.map((row, i) => {
        const [x, y, name] = anchors[row.id]!,
          player = seats.includes(row.id);
        const values = split.map((r) => r.amount);
        const adjust = (delta: 1 | -1) => {
          const amounts = adjustDamageAllocation(values, i, delta, 3, 1);
          setSplit(split.map((r, j) => ({ ...r, amount: amounts[j]! })));
        };
        return (
          <div
            className="sw-damage-point"
            data-player={player}
            key={row.id}
            style={{ left: x, top: y + (player ? 44 : -85) }}
          >
            <QuantityControl
              label={"Damage to " + name}
              treatment="pill"
              value={row.amount}
              canDecrease={!pending && row.amount > 1}
              canIncrease={
                !pending &&
                (assigned < 3 ||
                  split.some((other, j) => j !== i && other.amount > 1))
              }
              onDecrease={() => adjust(-1)}
              onIncrease={() => adjust(1)}
            />
          </div>
        );
      })}
    </>
  );
}
function Combat({ id, go, data }: SceneProps) {
  const source =
    typeof data === "object" &&
    data !== null &&
    "origin" in data &&
    typeof data.origin === "string"
      ? data.origin
      : "DA01";
  const draft =
    typeof data === "object" &&
    data !== null &&
    "assignment" in data &&
    Array.isArray(data.assignment)
      ? (data.assignment as AssignedDamage[])
      : undefined;
  const [trample, setTrample] = useState<[number, number]>([4, 2]);
  if (id === "DA09")
    return (
      <FeedbackBoard
        id={id}
        origin={source}
        go={(target) => go(target, target === source ? data : undefined)}
      />
    );
  if (id === "DA10") return null;
  if (id === "DA08" || id === "DA07") {
    return (
      <>
        <div className="sw-damage-actions">
          <MatchActions
            primary={{
              label: "Confirm",
              disabled: id === "DA07" && trample[0] + trample[1] !== 6,
              onActivate: () => go("DA09", { origin: id }),
            }}
          />
        </div>
        {id === "DA07" &&
          ["Blocker", "Alex"].map((name, i) => (
            <div
              key={name}
              className="sw-damage-point"
              data-player={i === 1}
              style={{ left: i ? 464 : 211, top: i ? 81 : 555 }}
            >
              <QuantityControl
                label={"Damage to " + name}
                treatment="pill"
                value={trample[i]!}
                canDecrease={trample[i]! > 0}
                canIncrease={trample[i]! < 6}
                onDecrease={() =>
                  setTrample(adjustDamageAllocation(trample, i, -1, 6, 0))
                }
                onIncrease={() =>
                  setTrample(adjustDamageAllocation(trample, i, 1, 6, 0))
                }
              />
            </div>
          ))}
      </>
    );
  }
  const multi = ["DA04", "DA05", "DA06"].includes(id);
  const amounts: Record<string, readonly number[]> = {
    DA01: [4, 0],
    DA02: [3, 1],
    DA02C: [2, 2],
    DA03: [3, 0],
  };
  const sources = attackers.slice(0, multi ? 2 : 1).map((attacker) => ({
    ...attacker,
    blockers: attacker.blockers.map((b, i) => ({
      ...b,
      suggested:
        draft
          ?.find((a) => a.sourceId === attacker.id)
          ?.targets.find((t) => t.targetId === b.id)?.amount ??
        (attacker.id === "bears" && id === "DA05"
          ? [1, 0][i]
          : amounts[id]?.[i]) ??
        b.suggested,
    })),
  }));
  return (
    <div className="sw-combat-assignment">
      <CombatDamageAssignment
        decisionId={id}
        attackers={sources}
        initialSourceId={["DA05", "DA06"].includes(id) ? "bears" : "angel"}
        onConfirm={(assignment) => go("DA09", { origin: id, assignment })}
      />
    </div>
  );
}
export default function DamageScene(props: SceneProps) {
  return props.id.startsWith("DC") ? (
    <Divided key={props.id} {...props} />
  ) : (
    <Combat key={props.id} {...props} />
  );
}
