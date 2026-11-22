import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { motion, useReducedMotion } from "motion/react";
import type {
  PlayerDecision,
  PlayerObject,
} from "@spellwake/application-contracts";
import type { MatchAction } from "@spellwake/ui";
import {
  attackTarget,
  attackTargets,
  attackWithCreature,
  blockAttacker,
  blockTargets,
  blockWithCreature,
  canAttack,
  canBlock,
  canSkipAttack,
  canSkipBlock,
  cancelPending,
  EMPTY_ATTACK,
  EMPTY_BLOCK,
  publishedAttackers,
  reconcileAttack,
  reconcileBlock,
  type AttackDraft,
  type BlockDraft,
} from "../../match/combat-draft";
import type { ReadyMatch } from "../../match/match-sources";
import * as s from "./match-table.css";

/** How a card is rimmed while a declaration is being made. */
export type CardMark = "eligible" | "copper" | "ivory";
export interface CombatLine {
  /** Anchors named `card:<handle>` or `seat:<seatId>`. */
  from: string;
  to: string;
  tone: "copper" | "ivory";
}
export interface CombatUi {
  marks: ReadonlyMap<string, CardMark>;
  /** Players offered as, or already chosen as, a defender. */
  seatMarks: ReadonlyMap<string, "offered" | "assigned">;
  lines: readonly CombatLine[];
  /** True when the click was this declaration's to take. */
  onCard: (object: PlayerObject) => boolean;
  onSeat: (seatId: string) => boolean;
  primary: MatchAction;
  secondary: MatchAction | null;
  status: string | null;
}

/**
 * The attack or block being declared, drawn on the table itself: rims on the
 * creatures and players offered, a line for each choice, and the two actions
 * that submit it. Nothing is sent until Attack, Block or the empty choice.
 * After the game accepts it, `note` says so until the next decision.
 */
export function useCombat(
  decision: PlayerDecision | null,
  source: ReadyMatch,
): { ui: CombatUi | null; note: string | null } {
  const domain = decision?.domain ?? null;
  const attack = domain?.kind === "attackers" ? domain : null;
  const block = domain?.kind === "blockers" ? domain : null;
  const key = decision?.decision ?? null;
  const [attackDraft, setAttackDraft] = useState<AttackDraft>(EMPTY_ATTACK);
  const [blockDraft, setBlockDraft] = useState<BlockDraft>(EMPTY_BLOCK);
  const [changed, setChanged] = useState(false);
  const [sent, setSent] = useState<{
    key: string;
    words: string;
    failed: string;
  } | null>(null);
  const [note, setNote] = useState<string | null>(null);

  // A new decision starts a new draft; an updated one keeps what is still offered.
  const lastKey = useRef<string | null>(null);
  useEffect(() => {
    if (key !== lastKey.current) {
      lastKey.current = key;
      setAttackDraft(EMPTY_ATTACK);
      setBlockDraft(EMPTY_BLOCK);
      setChanged(false);
      if (key) setNote(null);
      return;
    }
    if (attack) {
      const next = reconcileAttack(attackDraft, attack);
      if (next.changed) {
        setAttackDraft(next.draft);
        setChanged(true);
      }
    }
    if (block) {
      const next = reconcileBlock(blockDraft, block);
      if (next.changed) {
        setBlockDraft(next.draft);
        setChanged(true);
      }
    }
  }, [key, attack, block, attackDraft, blockDraft]);

  // What was sent is acknowledged only once the game no longer asks for it.
  const { commandPending, commandError } = source;
  useEffect(() => {
    if (!sent || commandPending) return;
    if (key === sent.key) return;
    if (!commandError) setNote(sent.words);
    setSent(null);
  }, [sent, commandPending, commandError, key]);

  const hasPending =
    (attack && attackDraft.pending) || (block && blockDraft.pending);
  useEffect(() => {
    if (!hasPending) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setAttackDraft(cancelPending);
      setBlockDraft(cancelPending);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [hasPending]);

  const { sendAction } = source;
  const submit = useCallback(
    (
      response:
        | {
            kind: "attackers";
            assignments: { attacker: string; defender: AttackTargetValue }[];
          }
        | {
            kind: "blockers";
            assignments: { blocker: string; attackers: string[] }[];
          },
      words: string,
      failed: string,
    ) => {
      if (!key) return;
      setSent({ key, words, failed });
      setChanged(false);
      void sendAction({ kind: "choose", decision: key, response });
    },
    [key, sendAction],
  );

  const ui = useMemo((): CombatUi | null => {
    const failed =
      sent && sent.key === key && commandError && !commandPending
        ? sent.failed
        : null;
    if (attack) {
      const targets = attackTargets(attackDraft, attack);
      const marks = new Map<string, CardMark>();
      for (const choice of attack.choices)
        marks.set(
          choice.attacker,
          attackDraft.assigned.has(choice.attacker) ||
            attackDraft.pending === choice.attacker
            ? "copper"
            : "eligible",
        );
      const seatMarks = new Map<string, "offered" | "assigned">();
      for (const target of targets) {
        if (target.kind === "seat") seatMarks.set(target.seatId, "offered");
        if (target.kind === "object") marks.set(target.handle, "copper");
      }
      const lines: CombatLine[] = [];
      for (const [attacker, target] of attackDraft.assigned) {
        if (target.kind === "seat") seatMarks.set(target.seatId, "assigned");
        if (target.kind === "object") marks.set(target.handle, "copper");
        if (target.kind !== "stack")
          lines.push({
            from: `card:${attacker}`,
            to:
              target.kind === "seat"
                ? `seat:${target.seatId}`
                : `card:${target.handle}`,
            tone: "copper",
          });
      }
      return {
        marks,
        seatMarks,
        lines,
        onCard: (object) => {
          if (commandPending) return true;
          const asTarget = targets.find(
            (target) =>
              target.kind === "object" && target.handle === object.handle,
          );
          if (asTarget) {
            setAttackDraft((draft) => attackTarget(draft, attack, asTarget));
            return true;
          }
          if (
            !attack.choices.some((choice) => choice.attacker === object.handle)
          )
            return false;
          setAttackDraft((draft) =>
            attackWithCreature(draft, attack, object.handle),
          );
          return true;
        },
        onSeat: (seatId) => {
          // A portrait is a defender only while a creature waits for one.
          if (!attackDraft.pending) return false;
          if (commandPending) return true;
          setAttackDraft((draft) =>
            attackTarget(draft, attack, { kind: "seat", seatId }),
          );
          return true;
        },
        primary: {
          label: "Attack",
          disabled: !canAttack(attackDraft, attack),
          pending: commandPending && sent?.words === "Attackers declared",
          onActivate: () =>
            submit(
              {
                kind: "attackers",
                assignments: [...attackDraft.assigned].map(
                  ([attacker, defender]) => ({ attacker, defender }),
                ),
              },
              "Attackers declared",
              "Couldn’t declare attackers.",
            ),
        },
        secondary: canSkipAttack(attack)
          ? {
              label: "No Attacks",
              pending:
                commandPending && sent?.words === "No attackers declared",
              onActivate: () =>
                submit(
                  { kind: "attackers", assignments: [] },
                  "No attackers declared",
                  "Couldn’t declare attackers.",
                ),
            }
          : null,
        status: failed ?? (changed ? "Attack selection changed." : null),
      };
    }
    if (block) {
      const targets = blockTargets(blockDraft, block);
      const marks = new Map<string, CardMark>();
      const attackers = publishedAttackers(block);
      for (const attacker of attackers) marks.set(attacker, "copper");
      for (const choice of block.choices)
        marks.set(
          choice.blocker,
          blockDraft.assigned.has(choice.blocker) ||
            blockDraft.pending === choice.blocker
            ? "ivory"
            : "eligible",
        );
      for (const target of targets) marks.set(target, "ivory");
      const defender = block.defendingSeatId ?? source.snapshot.viewerSeatId;
      const lines: CombatLine[] = [
        // The attacks already declared stay drawn while blocks are chosen.
        ...attackers.map((attacker): CombatLine => ({
          from: `card:${attacker}`,
          to: `seat:${defender}`,
          tone: "copper",
        })),
        ...[...blockDraft.assigned].flatMap(([blocker, chosen]) =>
          chosen.map((attacker): CombatLine => ({
            from: `card:${blocker}`,
            to: `card:${attacker}`,
            tone: "ivory",
          })),
        ),
      ];
      return {
        marks,
        seatMarks: new Map([[defender, "assigned"]]),
        lines,
        onCard: (object) => {
          if (commandPending) return true;
          if (targets.includes(object.handle)) {
            setBlockDraft((draft) =>
              blockAttacker(draft, block, object.handle),
            );
            return true;
          }
          if (!block.choices.some((choice) => choice.blocker === object.handle))
            return false;
          setBlockDraft((draft) =>
            blockWithCreature(draft, block, object.handle),
          );
          return true;
        },
        // Players are never block targets, so portraits keep moving the camera.
        onSeat: () => false,
        primary: {
          label: "Block",
          disabled: !canBlock(blockDraft, block),
          pending: commandPending && sent?.words === "Blocks declared",
          onActivate: () =>
            submit(
              {
                kind: "blockers",
                assignments: [...blockDraft.assigned].map(
                  ([blocker, chosen]) => ({ blocker, attackers: [...chosen] }),
                ),
              },
              "Blocks declared",
              "Couldn’t declare blockers.",
            ),
        },
        secondary: canSkipBlock(block)
          ? {
              label: "No Blocks",
              pending: commandPending && sent?.words === "No blocks declared",
              onActivate: () =>
                submit(
                  { kind: "blockers", assignments: [] },
                  "No blocks declared",
                  "Couldn’t declare blockers.",
                ),
            }
          : null,
        status: failed ?? (changed ? "Block selection changed." : null),
      };
    }
    return null;
  }, [
    attack,
    block,
    attackDraft,
    blockDraft,
    changed,
    commandPending,
    commandError,
    sent,
    key,
    submit,
    source.snapshot.viewerSeatId,
  ]);
  return { ui, note: ui ? null : note };
}
type AttackTargetValue = NonNullable<
  ReturnType<AttackDraft["assigned"]["get"]>
>;

interface Drawn {
  id: string;
  d: string;
  tone: CombatLine["tone"];
}

/**
 * The lines of a declaration, drawn over the table from each card to its
 * target. Endpoints are measured from the cards and portraits themselves, so
 * they follow the camera; a new line draws itself in.
 */
export function CombatLines({
  lines,
  follow,
}: {
  lines: readonly CombatLine[];
  /** Changes whenever the cards may have moved. */
  follow: unknown;
}): ReactElement {
  const root = useRef<SVGSVGElement>(null);
  const [drawn, setDrawn] = useState<Drawn[]>([]);
  const reduced = useReducedMotion();
  const wanted = lines
    .map((line) => `${line.from}>${line.to}>${line.tone}`)
    .join("|");
  useLayoutEffect(() => {
    const svg = root.current;
    if (!svg) return;
    let frame = 0;
    const until = performance.now() + 900;
    const measure = () => {
      const origin = svg.getBoundingClientRect();
      const point = (name: string, end: "from" | "to") => {
        const node = document.querySelector<HTMLElement>(
          `[data-anchor="${CSS.escape(name)}"]`,
        );
        if (!node) return null;
        const rect = node.getBoundingClientRect();
        if (rect.width === 0) return null;
        // A portrait is met at the foot of its picture; a card at its near edge.
        if (name.startsWith("seat:"))
          return {
            x: rect.left + rect.height / 2 - origin.left,
            y: rect.bottom + 2 - origin.top,
          };
        return {
          x: rect.left + rect.width / 2 - origin.left,
          y: (end === "from" ? rect.top + 6 : rect.bottom - 6) - origin.top,
        };
      };
      const next: Drawn[] = [];
      for (const line of lines) {
        const from = point(line.from, "from");
        const to = point(line.to, "to");
        if (!from || !to) continue;
        const reach = Math.max(40, Math.abs(from.y - to.y) * 0.5);
        next.push({
          id: `${line.from}>${line.to}`,
          tone: line.tone,
          d: `M ${from.x.toFixed(1)} ${from.y.toFixed(1)} C ${from.x.toFixed(1)} ${(from.y - reach).toFixed(1)}, ${to.x.toFixed(1)} ${(to.y + reach).toFixed(1)}, ${to.x.toFixed(1)} ${to.y.toFixed(1)}`,
        });
      }
      setDrawn((current) =>
        current.length === next.length &&
        current.every(
          (entry, index) =>
            entry.id === next[index]!.id &&
            entry.d === next[index]!.d &&
            entry.tone === next[index]!.tone,
        )
          ? current
          : next,
      );
      // Keep following while the cards travel to a new view.
      if (performance.now() < until) frame = requestAnimationFrame(measure);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", measure);
    };
    // `wanted` stands for the lines; `follow` for anything that moves a card.
  }, [wanted, follow]);
  return (
    <svg ref={root} className={s.lines} aria-hidden="true" focusable="false">
      <defs>
        {(["copper", "ivory"] as const).map((tone) => (
          <marker
            key={tone}
            id={`combat-head-${tone}`}
            markerUnits="userSpaceOnUse"
            markerWidth="16"
            markerHeight="16"
            refX="11"
            refY="8"
            viewBox="0 0 16 16"
            orient="auto"
          >
            <path
              d="M2 2 L12 8 L2 14 Z"
              className={s.lineHead}
              data-tone={tone}
            />
          </marker>
        ))}
      </defs>
      {drawn.map((line) => (
        <motion.path
          key={line.id}
          d={line.d}
          className={s.line}
          data-tone={line.tone}
          markerEnd={`url(#combat-head-${line.tone})`}
          initial={reduced ? false : { pathLength: 0, opacity: 0 }}
          animate={{ pathLength: 1, opacity: 1 }}
          transition={
            reduced
              ? { duration: 0 }
              : { duration: 0.42, ease: [0.22, 1, 0.36, 1] }
          }
        />
      ))}
    </svg>
  );
}
