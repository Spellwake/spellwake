import { useState, useEffect, useRef, useId } from "react";
import { motion, useReducedMotion } from "motion/react";
import { ChevronUp, ChevronDown, ChevronLeft } from "lucide-react";
import { OriginalCardFace, type CardImage } from "../cards/OriginalCardFace";
import { adjustDamageAllocation } from "./adjustDamageAllocation";
import * as s from "./combatAssignment.css";
import { designPixels } from "../design-pixels";

export interface CombatDamageSource {
  id: string;
  name: string;
  power: number;
  blockers: readonly { id: string; image: CardImage; suggested: number }[];
}
export interface AssignedDamage {
  sourceId: string;
  targets: { targetId: string; amount: number }[];
}
export interface CombatDamageAssignmentProps {
  /** A new decision ID starts a fresh draft. Source and target ordering is stable for its lifetime. */
  decisionId: string;
  attackers: readonly CombatDamageSource[];
  /** Optional source selected when restoring an existing assignment. */
  initialSourceId?: string;
  pending?: boolean;
  onConfirm: (assignments: AssignedDamage[]) => void;
}
/** The match supplies its legal recipients and suggestions. Only the final action emits assignments. */
export function CombatDamageAssignment(props: CombatDamageAssignmentProps) {
  return props.attackers.length ? (
    <AssignmentSteps key={props.decisionId} {...props} />
  ) : null;
}
function AssignmentSteps({
  attackers,
  initialSourceId,
  pending = false,
  onConfirm,
}: CombatDamageAssignmentProps) {
  const [step, setStep] = useState(() =>
    Math.max(
      0,
      attackers.findIndex((source) => source.id === initialSourceId),
    ),
  );
  const [drafts, setDrafts] = useState(() =>
    attackers.map((source) => source.blockers.map((b) => b.suggested)),
  );
  const [direction, setDirection] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const progressId = useId();
  const reduce = useReducedMotion();
  const attacker = attackers[step]!;
  const split = drafts[step]!;
  const remaining = attacker.power - split.reduce((sum, n) => sum + n, 0);
  const last = step === attackers.length - 1;
  const allAssigned = attackers.every(
    (source, i) => drafts[i]!.reduce((a, b) => a + b, 0) === source.power,
  );
  useEffect(() => {
    heading.current?.focus({ preventScroll: true });
  }, [step]);
  function move(to: number) {
    setDirection(to > step ? 1 : -1);
    setStep(to);
  }
  function adjust(target: number, delta: 1 | -1) {
    setDrafts((previous) =>
      previous.map((values, i) =>
        i === step
          ? adjustDamageAllocation(values, target, delta, attacker.power, 0)
          : values,
      ),
    );
  }
  return (
    <form
      className={s.assignment}
      onSubmit={(event) => {
        event.preventDefault();
        if (pending || remaining !== 0) return;
        if (!last) move(step + 1);
        else if (allAssigned)
          onConfirm(
            attackers.map((source, i) => ({
              sourceId: source.id,
              targets: source.blockers.map((blocker, j) => ({
                targetId: blocker.id,
                amount: drafts[i]![j]!,
              })),
            })),
          );
      }}
    >
      <header>
        <p className="combat-progress" id={progressId}>
          Creature {step + 1} of {attackers.length}
        </p>
        <h1 ref={heading} tabIndex={-1} aria-describedby={progressId}>
          Assign {attacker.name}’
          {attacker.name.toLowerCase().endsWith("s") ? "" : "s"}{" "}
          {attacker.power} damage
        </h1>
      </header>
      <motion.div
        className="combat-blockers"
        key={step}
        initial={
          direction && !reduce ? { opacity: 0.65, x: direction * 14 } : false
        }
        animate={{ opacity: 1, x: 0 }}
        transition={{ duration: reduce ? 0 : 0.16 }}
      >
        {attacker.blockers.map((blocker, index) => {
          const card = blocker.image;
          const amount = split[index];
          return (
            <div className="combat-card" key={blocker.id}>
              <OriginalCardFace image={card}>
                <div
                  className="combat-counter"
                  role="group"
                  aria-label={card.name + " damage"}
                >
                  <button
                    type="button"
                    aria-label={"Increase damage to " + card.name}
                    disabled={pending || amount === attacker.power}
                    onClick={() => adjust(index, 1)}
                  >
                    <ChevronUp />
                  </button>
                  <output aria-label={"Damage to " + card.name}>
                    <motion.span
                      key={amount}
                      initial={reduce ? false : { opacity: 0.65, y: 2 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ duration: reduce ? 0 : 0.12 }}
                    >
                      {amount}
                    </motion.span>
                  </output>
                  <button
                    type="button"
                    aria-label={"Decrease damage to " + card.name}
                    disabled={pending || amount === 0}
                    onClick={() => adjust(index, -1)}
                  >
                    <ChevronDown />
                  </button>
                </div>
              </OriginalCardFace>
            </div>
          );
        })}
      </motion.div>
      <footer>
        <div role="status" className="combat-remaining">
          {remaining > 0 && `${remaining} damage left to assign`}
        </div>
        <div className="combat-buttons">
          {step > 0 && (
            <button
              type="button"
              className="combat-back"
              disabled={pending}
              onClick={() => move(step - 1)}
            >
              <ChevronLeft size={designPixels(18)} /> Back
            </button>
          )}
          <button
            type="submit"
            className="combat-primary"
            disabled={pending || remaining !== 0 || (last && !allAssigned)}
          >
            {last ? "Confirm" : "Next"}
          </button>
        </div>
      </footer>
    </form>
  );
}
