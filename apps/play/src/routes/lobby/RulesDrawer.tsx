import { useEffect, useRef, useState, type ReactElement } from "react";
import { ChevronDown, ChevronUp, X } from "lucide-react";
import { ActionButton, Dialog, SelectMenu } from "@spellwake/ui";
import type { Outcome } from "../../decks/deck-sources";
import type { LobbyRules, RulesChoices } from "../../lobby/lobby-sources";
import { useDeviceClass } from "../../lib/display-scale";
import { useCommand } from "../account/operation";
import * as s from "./lobby.css";

/**
 * Game rules: a draft of the room's rules. Apply sends it once; a change
 * resets every human to Not ready. An invalid draft stays open with its
 * problem said. Cancel, X and Escape discard the draft.
 */
export function RulesDrawer({
  rules,
  choices,
  onApply,
  onClose,
}: {
  rules: LobbyRules;
  choices: RulesChoices;
  onApply: (next: LobbyRules) => Promise<Outcome>;
  onClose: () => void;
}): ReactElement {
  const [draft, setDraft] = useState(rules);
  const [isAdvancedOpen, setIsAdvancedOpen] = useState(true);
  const [problem, setProblem] = useState<string | null>(null);
  const command = useCommand();
  const isPhone = useDeviceClass() === "phone";
  const change = (next: Partial<LobbyRules>) => {
    setDraft((current) => ({ ...current, ...next }));
    setProblem(null);
  };
  const isChanged = JSON.stringify(draft) !== JSON.stringify(rules);
  const apply = (): void => {
    if (!isChanged) return onClose();
    void command
      .run(() => onApply(draft))
      .then((outcome) => {
        if (outcome?.status === "success") onClose();
        else if (outcome) setProblem(outcome.message);
      });
  };
  const select = (
    label: string,
    value: string,
    options: readonly string[],
    onPick: (value: string) => void,
  ): ReactElement => (
    <div className={s.field}>
      <span>{label}</span>
      {options.length > 1 ? (
        <SelectMenu
          label={label}
          value={value}
          options={options.map((option) => ({ value: option, label: option }))}
          onValueChange={onPick}
          disabled={command.isBusy}
        />
      ) : (
        <span>{value}</span>
      )}
    </div>
  );
  const body = (
    <>
      {select("Format", draft.format, choices.formats, (format) =>
        change({ format }),
      )}
      {select(
        "Table rules",
        draft.tableRules,
        choices.tableRules,
        (tableRules) => change({ tableRules }),
      )}
      {select(
        "Players",
        String(draft.players),
        choices.players.map(String),
        (players) => change({ players: Number(players) }),
      )}
      {select("Match", draft.match, choices.matches, (match) =>
        change({
          match: match === "Best of three" ? "Best of three" : "Single game",
        }),
      )}
      {(choices.planechase || choices.influences.length > 0) && (
        <div className={s.advanced}>
          <button
            type="button"
            className={s.advancedToggle}
            aria-expanded={isAdvancedOpen}
            onClick={() => setIsAdvancedOpen((open) => !open)}
          >
            {isAdvancedOpen ? (
              <ChevronUp aria-hidden="true" />
            ) : (
              <ChevronDown aria-hidden="true" />
            )}
            Advanced
          </button>
          {/* Collapsing hides the fields only; the draft keeps their values. */}
          <div className={s.advancedBody} hidden={!isAdvancedOpen}>
            {choices.planechase && (
              <div className={s.field}>
                <span>Planechase</span>
                <span
                  className={s.toggle}
                  role="radiogroup"
                  aria-label="Planechase"
                >
                  {([false, true] as const).map((on) => (
                    <button
                      key={String(on)}
                      type="button"
                      role="radio"
                      aria-checked={draft.planechase === on}
                      className={s.toggleOption}
                      onClick={() => change({ planechase: on })}
                    >
                      {on ? "On" : "Off"}
                    </button>
                  ))}
                </span>
              </div>
            )}
            {choices.influences.length > 0 &&
              select(
                "Range of influence",
                draft.influence ?? choices.influences[0] ?? "",
                choices.influences,
                (influence) => change({ influence }),
              )}
          </div>
        </div>
      )}
      {problem && (
        <p className={s.fieldError} role="alert">
          {problem}
        </p>
      )}
    </>
  );
  const actions = (
    <>
      <ActionButton disabled={command.isBusy} onClick={onClose}>
        Cancel
      </ActionButton>
      <ActionButton
        emphasis="primary"
        disabled={!isChanged}
        pending={command.isBusy}
        onClick={apply}
      >
        Apply changes
      </ActionButton>
    </>
  );
  // A phone gets the shared drawer; wider screens a panel from the right.
  if (isPhone)
    return (
      <Dialog
        open
        title="Game rules"
        busy={command.isBusy}
        onClose={onClose}
        actions={actions}
      >
        {body}
      </Dialog>
    );
  return (
    <SideDrawer busy={command.isBusy} onClose={onClose} actions={actions}>
      {body}
    </SideDrawer>
  );
}

function SideDrawer({
  busy,
  onClose,
  actions,
  children,
}: {
  busy: boolean;
  onClose: () => void;
  actions: ReactElement;
  children: ReactElement;
}): ReactElement {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal();
    element?.querySelector<HTMLElement>("h2")?.focus();
  }, []);
  return (
    <dialog
      ref={dialog}
      className={s.drawer}
      aria-labelledby="rules-title"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
      onClick={(event) => {
        // A press on the backdrop, outside the panel, closes it.
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className={s.drawerHead}>
        <h2 className={s.drawerTitle} id="rules-title" tabIndex={-1}>
          Game rules
        </h2>
        <button
          type="button"
          className={s.close}
          aria-label="Close"
          disabled={busy}
          onClick={onClose}
        >
          <X aria-hidden="true" />
        </button>
      </div>
      <div className={s.drawerBody}>{children}</div>
      <div className={s.drawerFoot}>{actions}</div>
    </dialog>
  );
}
