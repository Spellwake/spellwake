import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import {
  Circle,
  CircleUserRound,
  Grip,
  LogOut,
  MessageCircle,
  Settings,
  Sparkles,
  Swords,
} from "lucide-react";
import type {
  PlayerDecision,
  PlayerObject,
  PlayerTarget,
  PlayerTurn,
} from "@spellwake/application-contracts";
import {
  ActionButton,
  ConfirmationDialog,
  designPixels,
  Dialog,
  EmptyState,
  FeedbackMessages,
  LoadingShape,
  LoadingState,
  MatchActions,
  PlayerIdentity,
  TurnPhaseDisplay,
  type MatchPhase,
} from "@spellwake/ui";
import { ImmersiveFrame } from "../../components/shell/ImmersiveFrame";
import * as frame from "../../components/shell/frames.css";
import type { MatchSource, ReadyMatch } from "../../match/match-sources";
import {
  DOCK_MARGIN,
  handShownHeight,
  seatBoard,
  type TableView,
} from "../../match/table-model";
import { CombatLines, useCombat, type CombatUi } from "./combat";
import {
  CastPrompt,
  CastSource,
  ModeChoice,
  pendingObject,
  useCasting,
} from "./casting";
import { DamagePills, DamageSources, useDamage } from "./damage";
import { MatchStack, useStackReading } from "./stack";
import { ConcedeConfirmation, MatchSettings, TableChat } from "./TablePanels";
import { hasUnseen } from "../../match/table-panels";
import { BUILT_ANSWERS, DecisionAnswer } from "./DecisionAnswers";
import { MatchTable } from "./MatchTable";
import { GameEndBand, GameEndPanel } from "./GameEnd";
import * as table from "./match-table.css";
import * as s from "./match.css";

const ICON = designPixels(26);

export const PHASE_MAP: Record<
  PlayerTurn["phase"],
  { phase: MatchPhase; label: string }
> = {
  beginning: { phase: "beginning", label: "Beginning" },
  "precombat-main": { phase: "first-main", label: "First main" },
  combat: { phase: "combat", label: "Combat" },
  "postcombat-main": { phase: "second-main", label: "Second main" },
  ending: { phase: "end", label: "Ending" },
};

const MANA_SHORT: Record<string, string> = {
  white: "W",
  blue: "U",
  black: "B",
  red: "R",
  green: "G",
  colourless: "C",
};

function objectName(obj: PlayerObject): string {
  return obj.identity.kind === "known"
    ? obj.identity.characteristics.name
    : "Face-down card";
}

function objectPowerToughness(obj: PlayerObject): string | null {
  const p =
    obj.publicProperties?.power ??
    (obj.identity.kind === "known"
      ? obj.identity.characteristics.printedPower
      : null);
  const t =
    obj.publicProperties?.toughness ??
    (obj.identity.kind === "known"
      ? obj.identity.characteristics.printedToughness
      : null);
  if (p !== null && t !== null) return `${p}/${t}`;
  if (
    obj.identity.kind === "known" &&
    obj.identity.characteristics.printedLoyalty
  )
    return `L${obj.identity.characteristics.printedLoyalty}`;
  return null;
}

function targetLabel(
  target: PlayerTarget,
  objects: ReadonlyMap<string, PlayerObject>,
  seatNames: ReadonlyMap<string, string>,
): string {
  if (target.kind === "seat") return seatNames.get(target.seatId) ?? "Player";
  if (target.kind === "object") {
    const obj = objects.get(target.handle);
    return obj ? objectName(obj) : target.handle;
  }
  return `Stack ${target.handle}`;
}

function CardTile({
  object,
  artUrl,
  playable,
  selected,
  onClick,
}: {
  object: PlayerObject;
  artUrl?: string | undefined;
  playable?: boolean | undefined;
  selected?: boolean | undefined;
  onClick: () => void;
}): ReactElement {
  const name = objectName(object);
  const pt = objectPowerToughness(object);
  const cost =
    object.identity.kind === "known"
      ? object.identity.characteristics.manaCost
      : null;
  const counters = object.counters
    .map((c) => `${c.count} ${c.name}`)
    .join(", ");
  return (
    <button
      type="button"
      className={s.cardTile}
      data-tapped={object.tapped ? "true" : undefined}
      data-playable={playable ? "true" : undefined}
      data-selected={selected ? "true" : undefined}
      aria-label={name}
      onClick={onClick}
    >
      {artUrl && <img src={artUrl} alt="" className={s.cardTileArt} />}
      <span className={s.cardTileScrim}>
        <span className={s.cardTitle}>{name}</span>
        <span className={s.cardMeta}>
          <span>{cost ?? (object.tapped ? "Tapped" : "")}</span>
          <span>
            {pt ?? ""}
            {counters ? ` · ${counters}` : ""}
          </span>
        </span>
      </span>
    </button>
  );
}

function DecisionPanel({
  decision,
  objects,
  seatNames,
  pool,
  pending,
  onChoose,
}: {
  decision: PlayerDecision;
  objects: ReadonlyMap<string, PlayerObject>;
  seatNames: ReadonlyMap<string, string>;
  /** The answering player's unspent mana, for a cost paid from it. */
  pool: readonly { color: string; amount: number }[];
  pending: boolean;
  onChoose: (
    response: Extract<
      Parameters<ReadyMatch["sendAction"]>[0],
      { kind: "choose" }
    >["response"],
  ) => void;
}): ReactElement {
  const [selectedHandles, setSelectedHandles] = useState<string[]>([]);
  const [selectedAttackers, setSelectedAttackers] = useState<
    Record<string, PlayerTarget>
  >({});
  const [selectedBlockers, setSelectedBlockers] = useState<
    Record<string, string[]>
  >({});
  const [chosenNumber, setChosenNumber] = useState<string>("0");

  const d = decision.domain;

  if (BUILT_ANSWERS.has(d.kind))
    return (
      <DecisionAnswer
        decision={decision}
        pool={pool}
        nameOf={(handle) => {
          const object = objects.get(handle);
          return object ? objectName(object) : "Card";
        }}
        labelOf={(target) => targetLabel(target, objects, seatNames)}
        pending={pending}
        onChoose={onChoose}
      />
    );

  if (d.kind === "options") {
    return (
      <div className={s.decisionCard} role="region" aria-label="Decision">
        <p className={s.decisionTitle}>Choose an option ({decision.kind})</p>
        <div className={s.decisionRow}>
          {d.options.map((opt) => (
            <ActionButton
              key={opt.handle}
              disabled={pending}
              onClick={() => onChoose({ kind: "option", option: opt.handle })}
            >
              {opt.label}
            </ActionButton>
          ))}
        </div>
      </div>
    );
  }

  if (d.kind === "targets") {
    return (
      <div className={s.decisionCard} role="region" aria-label="Choose target">
        <p className={s.decisionTitle}>Choose a target</p>
        <div className={s.decisionRow}>
          {d.targets.map((t, idx) => (
            <ActionButton
              key={idx}
              disabled={pending}
              onClick={() => onChoose({ kind: "target", target: t })}
            >
              {targetLabel(t, objects, seatNames)}
            </ActionButton>
          ))}
        </div>
      </div>
    );
  }

  if (d.kind === "objects") {
    const min = Number(d.minimum);
    const max = Number(d.maximum);
    const toggle = (handle: string) => {
      setSelectedHandles((curr) => {
        if (curr.includes(handle)) return curr.filter((h) => h !== handle);
        if (curr.length >= max) return [...curr.slice(1), handle];
        return [...curr, handle];
      });
    };
    const valid =
      selectedHandles.length >= min && selectedHandles.length <= max;
    return (
      <div className={s.decisionCard} role="region" aria-label="Choose cards">
        <p className={s.decisionTitle}>
          Select {min === max ? min : `${min}–${max}`} card(s) ({decision.kind})
        </p>
        <div className={s.decisionRow}>
          {d.objects.map((handle) => {
            const obj = objects.get(handle);
            const active = selectedHandles.includes(handle);
            return (
              <ActionButton
                key={handle}
                disabled={pending}
                onClick={() => toggle(handle)}
              >
                {active ? "✓ " : ""}
                {obj ? objectName(obj) : handle}
              </ActionButton>
            );
          })}
          <ActionButton
            disabled={pending || !valid}
            onClick={() =>
              onChoose({ kind: "objects", objects: selectedHandles })
            }
          >
            Confirm ({selectedHandles.length})
          </ActionButton>
        </div>
      </div>
    );
  }

  if (d.kind === "attackers") {
    const toggleAttacker = (attacker: string, defender: PlayerTarget) => {
      setSelectedAttackers((curr) => {
        if (curr[attacker]) {
          const next = { ...curr };
          delete next[attacker];
          return next;
        }
        return { ...curr, [attacker]: defender };
      });
    };
    const assignments = Object.entries(selectedAttackers).map(
      ([attacker, defender]) => ({ attacker, defender }),
    );
    return (
      <div
        className={s.decisionCard}
        role="region"
        aria-label="Declare attackers"
      >
        <p className={s.decisionTitle}>Declare attackers</p>
        <div className={s.decisionRow}>
          {d.choices.map((choice) => {
            const obj = objects.get(choice.attacker);
            const def = choice.defenders[0]!;
            const active = Boolean(selectedAttackers[choice.attacker]);
            return (
              <ActionButton
                key={choice.attacker}
                disabled={pending}
                onClick={() => toggleAttacker(choice.attacker, def)}
              >
                <Swords size={16} aria-hidden="true" />
                {active ? "Attacking: " : ""}
                {obj ? objectName(obj) : choice.attacker} →{" "}
                {targetLabel(def, objects, seatNames)}
              </ActionButton>
            );
          })}
          <ActionButton
            disabled={pending}
            onClick={() => onChoose({ kind: "attackers", assignments })}
          >
            {assignments.length > 0
              ? `Attack with ${assignments.length}`
              : "No attacks"}
          </ActionButton>
        </div>
      </div>
    );
  }

  if (d.kind === "blockers") {
    const toggleBlocker = (blocker: string, attacker: string) => {
      setSelectedBlockers((curr) => {
        const list = curr[blocker] ?? [];
        const nextList = list.includes(attacker)
          ? list.filter((a) => a !== attacker)
          : [...list, attacker];
        const next = { ...curr };
        if (nextList.length === 0) delete next[blocker];
        else next[blocker] = nextList;
        return next;
      });
    };
    const assignments = Object.entries(selectedBlockers).map(
      ([blocker, attackers]) => ({ blocker, attackers }),
    );
    return (
      <div
        className={s.decisionCard}
        role="region"
        aria-label="Declare blockers"
      >
        <p className={s.decisionTitle}>Declare blockers</p>
        <div className={s.decisionRow}>
          {d.choices.map((choice) => {
            const blockerObj = objects.get(choice.blocker);
            return choice.attackers.map((attacker) => {
              const attObj = objects.get(attacker);
              const active = (selectedBlockers[choice.blocker] ?? []).includes(
                attacker,
              );
              return (
                <ActionButton
                  key={`${choice.blocker}-${attacker}`}
                  disabled={pending}
                  onClick={() => toggleBlocker(choice.blocker, attacker)}
                >
                  {active ? "✓ " : ""}
                  {blockerObj
                    ? objectName(blockerObj)
                    : choice.blocker} blocks{" "}
                  {attObj ? objectName(attObj) : attacker}
                </ActionButton>
              );
            });
          })}
          <ActionButton
            disabled={pending}
            onClick={() => onChoose({ kind: "blockers", assignments })}
          >
            {assignments.length > 0
              ? `Block (${assignments.length})`
              : "No blocks"}
          </ActionButton>
        </div>
      </div>
    );
  }

  if (d.kind === "payment") {
    return (
      <div className={s.decisionCard} role="region" aria-label="Payment">
        <p className={s.decisionTitle}>Pay cost ({d.total} total)</p>
        <div className={s.decisionRow}>
          <ActionButton
            disabled={pending}
            onClick={() =>
              onChoose({ kind: "pay", payment: { spend: d.minimumSpend } })
            }
          >
            Pay cost
          </ActionButton>
          {d.allowDecline && (
            <ActionButton
              disabled={pending}
              onClick={() => onChoose({ kind: "decline" })}
            >
              Decline
            </ActionButton>
          )}
        </div>
      </div>
    );
  }

  if (d.kind === "library-search") {
    return (
      <div className={s.decisionCard} role="region" aria-label="Library search">
        <p className={s.decisionTitle}>Choose from library</p>
        <div className={s.decisionRow}>
          {d.eligible.map((opt) => (
            <ActionButton
              key={opt.handle}
              disabled={pending}
              onClick={() =>
                onChoose({ kind: "objects", objects: [opt.handle] })
              }
            >
              {opt.label}
            </ActionButton>
          ))}
          {!d.required && (
            <ActionButton
              disabled={pending}
              onClick={() => onChoose({ kind: "objects", objects: [] })}
            >
              Fail to find
            </ActionButton>
          )}
        </div>
      </div>
    );
  }

  if (d.kind === "number") {
    const val = Number(chosenNumber);
    const min = Number(d.minimum);
    const max = Number(d.maximum);
    const valid = Number.isInteger(val) && val >= min && val <= max;
    return (
      <div className={s.decisionCard} role="region" aria-label="Choose number">
        <p className={s.decisionTitle}>
          Choose a number ({d.minimum} to {d.maximum})
        </p>
        <div className={s.decisionRow}>
          <input
            type="number"
            min={min}
            max={max}
            value={chosenNumber}
            onChange={(e) => setChosenNumber(e.target.value)}
            style={{ width: 90, padding: "6px 10px" }}
          />
          <ActionButton
            disabled={pending || !valid}
            onClick={() => onChoose({ kind: "number", value: String(val) })}
          >
            Confirm
          </ActionButton>
        </div>
      </div>
    );
  }

  if (d.kind === "legend") {
    const first = d.conflicts[0]!;
    return (
      <div className={s.decisionCard} role="region" aria-label="Legend rule">
        <p className={s.decisionTitle}>
          Choose which legendary permanent to keep
        </p>
        <div className={s.decisionRow}>
          {first.objects.map((handle) => {
            const obj = objects.get(handle);
            return (
              <ActionButton
                key={handle}
                disabled={pending}
                onClick={() =>
                  onChoose({
                    kind: "legend",
                    conflict: first.conflict,
                    keep: handle,
                  })
                }
              >
                Keep {obj ? objectName(obj) : handle}
              </ActionButton>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className={s.decisionCard} role="region" aria-label="Decision">
      <p className={s.decisionTitle}>Pending decision: {decision.kind}</p>
      <div className={s.decisionRow}>
        <ActionButton
          disabled={pending}
          onClick={() => onChoose({ kind: "decline" })}
        >
          Decline / Continue
        </ActionButton>
      </div>
    </div>
  );
}

/**
 * Where the turn panel docks along the lower edge (an upright phone or
 * tablet), the hand shows beneath it and the board stays above it. Measured
 * from the frame, so the table follows the shell's own layout.
 */
export function useLowerDock(): { trayHeight: number; clearBottom: number } {
  const [dock, setDock] = useState({ trayHeight: 0, clearBottom: 0 });
  const measure = useCallback(() => {
    const panel = document.querySelector('[data-frame="hud-side"]');
    const stage = { width: window.innerWidth, height: window.innerHeight };
    const box = panel?.getBoundingClientRect();
    const top = box?.top ?? 0;
    // Docked along the lower edge, the panel spans the stage; beside the
    // table it is a narrow column. Its width does not change with what it
    // holds or with the tray, so this answer cannot flip as either grows.
    const docked = !!box && box.width > stage.width * 0.6;
    const rootSize = parseFloat(
      getComputedStyle(document.documentElement).fontSize,
    );
    const next = docked
      ? {
          // The frame reserves its tray in design pixels.
          trayHeight: (handShownHeight(stage) * 16) / rootSize,
          clearBottom: stage.height - top + DOCK_MARGIN,
        }
      : { trayHeight: 0, clearBottom: 0 };
    setDock((current) =>
      Math.abs(current.trayHeight - next.trayHeight) < 1 &&
      Math.abs(current.clearBottom - next.clearBottom) < 1
        ? current
        : next,
    );
  }, []);
  // The frame reserves the tray this hook reports, which lifts the panel
  // without resizing it. No observer reports that move, so the panel is read
  // again once the tray it asked for is in place.
  useLayoutEffect(measure, [measure, dock.trayHeight]);
  useEffect(() => {
    const panel = document.querySelector('[data-frame="hud-side"]');
    const observer = new ResizeObserver(measure);
    if (panel) observer.observe(panel);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [measure]);
  return dock;
}

function LiveMatch({ source }: { source: ReadyMatch }): ReactElement {
  const dock = useLowerDock();
  const { snapshot, seatNames, artByPrintingId } = source;
  // Once the game is over nothing more is asked or offered at this table.
  const result = source.result ?? null;
  const series = (result && source.series) ?? null;
  // Between the games of a match there is still a match to walk away from.
  const matchGoesOn = series !== null && series.next.kind !== "over";
  // The band lifts so the finished table can be seen.
  const [resultRaised, setResultRaised] = useState(true);

  const [view, setView] = useState<TableView>({
    kind: "seat",
    seatId: snapshot.viewerSeatId,
  });
  // A seat that has left the game cannot stay the camera's subject.
  const viewedSeat =
    view.kind === "seat" &&
    snapshot.players.some((player) => player.seatId === view.seatId)
      ? view.seatId
      : null;
  const tableView: TableView =
    view.kind === "overview"
      ? view
      : { kind: "seat", seatId: viewedSeat ?? snapshot.viewerSeatId };

  const [inspectedHandle, setInspectedHandle] = useState<string | null>(null);
  const [pileView, setPileView] = useState<{
    title: string;
    objects: PlayerObject[];
  } | null>(null);
  const [chatOpen, setChatOpen] = useState(false);
  const [chatDraft, setChatDraft] = useState("");
  // The newest table message this player has actually had on screen.
  const [chatSeen, setChatSeen] = useState<string | null>(null);
  const seeChat = useCallback((id: string) => setChatSeen(id), []);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [conceding, setConceding] = useState<
    "closed" | "asking" | "sending" | "failed"
  >("closed");
  const [leaveOpen, setLeaveOpen] = useState(false);
  const [leavingMatch, setLeavingMatch] = useState(false);

  const objectByHandle = useMemo(
    () => new Map(snapshot.visibleObjects.map((o) => [o.handle, o] as const)),
    [snapshot.visibleObjects],
  );

  const zoneByType = useMemo(() => {
    const find = (type: string, ownerSeatId: string | null) =>
      snapshot.zones.find(
        (z) => z.zoneType === type && z.ownerSeatId === ownerSeatId,
      );
    return {
      battlefield: find("battlefield", null),
      exile: find("exile", null),
      command: find("command", null),
      viewerHand: find("hand", snapshot.viewerSeatId),
      viewerGraveyard: find("graveyard", snapshot.viewerSeatId),
    };
  }, [snapshot.zones, snapshot.viewerSeatId]);

  const objectsIn = (zone: typeof zoneByType.battlefield): PlayerObject[] => {
    if (!zone || zone.visibility === "hidden") return [];
    return zone.objects
      .map((h) => objectByHandle.get(h))
      .filter((o): o is PlayerObject => Boolean(o));
  };

  const exileObjects = objectsIn(zoneByType.exile);
  const commandObjects = objectsIn(zoneByType.command);

  const viewerPlayer = snapshot.players.find(
    (p) => p.seatId === snapshot.viewerSeatId,
  );

  // Out of the game, a player stays at the table and watches.
  const watching = viewerPlayer?.eliminated ?? false;
  const { commandPending, commandError, dismissCommandError } = source;
  useEffect(() => {
    if (conceding !== "sending" || commandPending) return;
    if (watching) setConceding("closed");
    else if (commandError) {
      // The confirmation says so itself, in place.
      dismissCommandError();
      setConceding("failed");
    }
  }, [conceding, commandPending, commandError, watching, dismissCommandError]);
  const shownNames = useMemo(
    () =>
      new Map(
        snapshot.players.map((player) => {
          const name = seatNames.get(player.seatId) ?? "Player";
          return [
            player.seatId,
            player.eliminated
              ? `${name} · ${player.seatId === snapshot.viewerSeatId ? "Conceded" : "Out"}`
              : name,
          ] as const;
        }),
      ),
    [snapshot.players, snapshot.viewerSeatId, seatNames],
  );

  const hasPriority =
    !watching &&
    !result &&
    (snapshot.turn?.prioritySeatIds.includes(snapshot.viewerSeatId) ?? false);
  const isViewerTurn =
    snapshot.turn?.activeSeatIds.includes(snapshot.viewerSeatId) ?? false;

  const activeDecision = result
    ? undefined
    : snapshot.decisions.find((d) =>
        d.respondingSeatIds.includes(snapshot.viewerSeatId),
      );

  const combat = useCombat(activeDecision ?? null, source);

  const phaseInfo = snapshot.turn
    ? PHASE_MAP[snapshot.turn.phase]
    : { phase: "beginning" as MatchPhase, label: "Opening" };

  const turnLabel = snapshot.turn
    ? isViewerTurn
      ? `Your turn · T${snapshot.turn.number}`
      : `${seatNames.get(snapshot.turn.activeSeatIds[0] ?? "") ?? "Opponent"}’s turn · T${snapshot.turn.number}`
    : "Preparing game";

  const artFor = (obj: PlayerObject): string | undefined =>
    obj.identity.kind !== "known"
      ? undefined
      : ((obj.identity.printingId
          ? artByPrintingId.get(obj.identity.printingId)
          : undefined) ??
        source.faceByName?.get(obj.identity.characteristics.name));

  const damage = useDamage(activeDecision ?? null, source, (handle) => {
    const object = objectByHandle.get(handle);
    return {
      name: object ? objectName(object) : "Card",
      face: object ? artFor(object) : undefined,
    };
  });
  // A spell is cast only while nothing else is asked and the player may act.
  const casting = useCasting(
    source,
    hasPriority && !activeDecision && !source.chooseOpening,
    (handle) => {
      const object = objectByHandle.get(handle);
      const name = object ? objectName(object) : null;
      const src = object ? artFor(object) : undefined;
      const printed = name ? source.printedModesByName?.get(name) : undefined;
      return name && src && printed ? { name, src, printed } : null;
    },
    (target) =>
      target.kind === "seat"
        ? (seatNames.get(target.seatId) ?? "Player")
        : target.kind === "object"
          ? ((object) => (object ? objectName(object) : "Card"))(
              objectByHandle.get(target.handle),
            )
          : "Spell",
  );
  // What is being declared, assigned or cast on the table, if anything.
  const declaring: CombatUi | null = combat.ui ?? damage ?? casting.ui;
  // A second press on an offered hand card casts it; a single press reads it.
  const handPress = useRef<{ handle: string; timer: number } | null>(null);
  useEffect(
    () => () => {
      if (handPress.current) window.clearTimeout(handPress.current.timer);
    },
    [],
  );
  const reading = useStackReading(snapshot);
  const follow = `${tableView.kind === "seat" ? tableView.seatId : "table"}:${snapshot.throughEventSequence}:${dock.clearBottom}`;

  const inspected = inspectedHandle
    ? (objectByHandle.get(inspectedHandle) ?? null)
    : null;

  const handleCardAction = (
    obj: PlayerObject,
    zoneKind: "hand" | "battlefield",
  ) => {
    if (zoneKind === "battlefield" && declaring?.onCard(obj)) return;
    if (zoneKind === "hand" && casting.playable.has(obj.handle)) {
      const earlier = handPress.current;
      if (earlier) window.clearTimeout(earlier.timer);
      if (earlier?.handle === obj.handle) {
        handPress.current = null;
        casting.begin(obj.handle);
        return;
      }
      handPress.current = {
        handle: obj.handle,
        timer: window.setTimeout(() => {
          handPress.current = null;
          setInspectedHandle(obj.handle);
        }, 280),
      };
      return;
    }
    const mine =
      (obj.controllerSeatId ?? obj.ownerSeatId) === snapshot.viewerSeatId;
    if (!mine || !hasPriority || source.commandPending) {
      setInspectedHandle(obj.handle);
      return;
    }
    if (zoneKind === "hand") {
      const isLand = obj.publicProperties?.cardTypes.includes("land");
      if (isLand) {
        void source.sendAction({ kind: "play-land", source: obj.handle });
        return;
      }
      setInspectedHandle(obj.handle);
      return;
    }
    if (zoneKind === "battlefield") {
      const manaAbility = obj.abilities.find((a) => a.kind === "mana");
      if (manaAbility && !obj.tapped) {
        void source.sendAction({
          kind: "activate-mana",
          source: obj.handle,
          ability: manaAbility.handle,
        });
        return;
      }
      setInspectedHandle(obj.handle);
    }
  };

  // The dot clears only once the newest message has been on screen.
  const unreadChat = !chatOpen && hasUnseen(source.chat.messages, chatSeen);
  // The chat and the band want the same part of a small screen. The chat is
  // the one being used, so the result waits in the turn panel beside it.
  const bandShown = result !== null && resultRaised && !chatOpen;

  return (
    <>
      <ImmersiveFrame
        playersLabel="Players"
        trayHeight={dock.trayHeight}
        leading={
          <>
            <button
              type="button"
              className={frame.hudButton}
              aria-label="Settings"
              onClick={() => {
                // Settings take the screen; the chat keeps its draft and place.
                setChatOpen(false);
                setSettingsOpen(true);
              }}
            >
              <Settings size={ICON} aria-hidden="true" />
            </button>
            <span className={s.chatButton}>
              <button
                type="button"
                className={frame.hudButton}
                aria-label={
                  unreadChat ? "Table chat, unread messages" : "Table chat"
                }
                aria-expanded={chatOpen}
                onClick={() => {
                  // Opening the chat lifts the band for good.
                  if (!chatOpen && result) setResultRaised(false);
                  setChatOpen(!chatOpen);
                }}
              >
                <MessageCircle size={ICON} aria-hidden="true" />
              </button>
              {unreadChat && (
                <span className={s.unreadDot} aria-hidden="true" />
              )}
            </span>
            <button
              type="button"
              className={frame.hudButton}
              aria-label={
                tableView.kind === "overview"
                  ? "Return to your board"
                  : "View the whole table"
              }
              aria-pressed={tableView.kind === "overview"}
              onClick={() =>
                setView(
                  tableView.kind === "overview"
                    ? { kind: "seat", seatId: snapshot.viewerSeatId }
                    : { kind: "overview" },
                )
              }
            >
              {tableView.kind === "overview" ? (
                <Circle
                  size={designPixels(11)}
                  fill="currentColor"
                  aria-hidden="true"
                />
              ) : (
                <Grip size={ICON} aria-hidden="true" />
              )}
            </button>
          </>
        }
        players={snapshot.players.map((player) => {
          const name = seatNames.get(player.seatId) ?? "Player";
          const out = player.eliminated;
          const takingTurn =
            !result &&
            (snapshot.turn?.activeSeatIds.includes(player.seatId) ?? false);
          return (
            <span
              key={player.seatId}
              className={table.seatAnchor}
              data-anchor={`seat:${player.seatId}`}
              data-out={out || undefined}
              data-recipient={(declaring?.seatMarks ?? reading.seatMarks).get(
                player.seatId,
              )}
            >
              <PlayerIdentity
                presentation="seat"
                name={shownNames.get(player.seatId) ?? name}
                {...(out ? {} : { life: Number(player.life) || 0 })}
                actionLabel={
                  player.seatId === snapshot.viewerSeatId
                    ? "View your board"
                    : `View ${name}’s board`
                }
                onActivate={() => {
                  if (declaring?.onSeat(player.seatId)) return;
                  setView({ kind: "seat", seatId: player.seatId });
                }}
                takingTurn={takingTurn}
                viewingOpponent={
                  tableView.kind === "seat" &&
                  tableView.seatId === player.seatId &&
                  player.seatId !== snapshot.viewerSeatId
                }
                portrait={<CircleUserRound aria-hidden="true" />}
              />
              {casting.targetSeats.has(player.seatId) && (
                <button
                  type="button"
                  className={table.seatTarget}
                  aria-label={`${
                    casting.pills.some(
                      (pill) => pill.anchor === `seat:${player.seatId}`,
                    )
                      ? "Remove target"
                      : "Target"
                  } ${player.seatId === snapshot.viewerSeatId ? "yourself" : name}`}
                  onClick={() => casting.chooseSeat(player.seatId)}
                />
              )}
            </span>
          );
        })}
        trailing={
          <button
            type="button"
            className={frame.hudButton}
            aria-label="Leave table"
            // A player who is out has nothing left to concede.
            onClick={() =>
              matchGoesOn
                ? setLeavingMatch(true)
                : watching || result
                  ? source.onLeave()
                  : setLeaveOpen(true)
            }
          >
            <LogOut size={ICON} aria-hidden="true" />
          </button>
        }
        side={
          <>
            {/* The turn it ended on is no longer anyone's turn. */}
            {!result && (
              <TurnPhaseDisplay
                turnLabel={turnLabel}
                phase={phaseInfo.phase}
                phaseLabel={phaseInfo.label}
              />
            )}
            {source.connectionState !== "live" && !result && (
              <span className={s.statusBadge} role="status">
                {source.connectionState === "reconnecting"
                  ? "Reconnecting…"
                  : source.connectionState === "ended"
                    ? "Ended"
                    : "Connecting…"}
              </span>
            )}
            {!result && viewerPlayer && viewerPlayer.manaPool.length > 0 && (
              <div className={s.manaBar} aria-label="Mana pool">
                <Sparkles size={14} aria-hidden="true" />
                {viewerPlayer.manaPool.map((m) => (
                  <span key={m.color}>
                    {m.amount}
                    {MANA_SHORT[m.color] ?? m.color}
                  </span>
                ))}
              </div>
            )}
            {damage && <DamageSources sources={damage.sources} />}
            {result ? (
              !bandShown && (
                <GameEndPanel
                  result={result}
                  series={series}
                  onLeave={source.onLeave}
                />
              )
            ) : watching ? (
              <p className={s.watching}>Watching</p>
            ) : declaring ? (
              <MatchActions
                primary={declaring.primary}
                {...(declaring.secondary
                  ? { secondary: declaring.secondary }
                  : {})}
                {...(declaring.status ? { status: declaring.status } : {})}
                {...(declaring.layout ? { layout: declaring.layout } : {})}
              />
            ) : (
              // Next passes with nothing waiting; Resolve passes on what is. While
              // another player acts there is no action to show.
              <MatchActions
                {...(hasPriority
                  ? {
                      primary: {
                        label: snapshot.stack.length > 0 ? "Resolve" : "Next",
                        pending: source.commandPending,
                        onActivate: () =>
                          void source.sendAction({ kind: "pass" }),
                      },
                    }
                  : {})}
                {...(activeDecision
                  ? { status: `Action required: ${activeDecision.kind}` }
                  : hasPriority && combat.note
                    ? { status: combat.note }
                    : {})}
              />
            )}
            <div className={s.sideDecisions}>
              {source.chooseOpening && !result && (
                <div className={s.decisionCard}>
                  <p className={s.decisionTitle}>Choose play or draw</p>
                  <div className={s.decisionRow}>
                    <ActionButton
                      disabled={source.commandPending}
                      onClick={() => void source.chooseOpening?.(true)}
                    >
                      Play first
                    </ActionButton>
                    <ActionButton
                      disabled={source.commandPending}
                      onClick={() => void source.chooseOpening?.(false)}
                    >
                      Draw first
                    </ActionButton>
                  </div>
                </div>
              )}
              {activeDecision && !declaring && !casting.dividing && (
                <DecisionPanel
                  key={activeDecision.decision}
                  decision={activeDecision}
                  objects={objectByHandle}
                  seatNames={seatNames}
                  pool={(viewerPlayer?.manaPool ?? []).map((mana) => ({
                    color: mana.color,
                    amount: Number(mana.amount),
                  }))}
                  pending={source.commandPending}
                  onChoose={(response) =>
                    void source.sendAction({
                      kind: "choose",
                      decision: activeDecision.decision,
                      response,
                    })
                  }
                />
              )}
            </div>

            {(exileObjects.length > 0 || commandObjects.length > 0) && (
              <div className={s.zoneCluster}>
                {exileObjects.length > 0 && (
                  <button
                    type="button"
                    className={s.zonePill}
                    onClick={() =>
                      setPileView({ title: "Exile", objects: exileObjects })
                    }
                  >
                    Exile ({exileObjects.length})
                  </button>
                )}
                {commandObjects.length > 0 && (
                  <button
                    type="button"
                    className={s.zonePill}
                    onClick={() =>
                      setPileView({ title: "Command", objects: commandObjects })
                    }
                  >
                    Command ({commandObjects.length})
                  </button>
                )}
              </div>
            )}
          </>
        }
      >
        <MatchTable
          snapshot={snapshot}
          view={tableView}
          seatNames={shownNames}
          faceOf={artFor}
          marks={declaring?.marks ?? reading.marks}
          isPlayable={(object) => casting.playable.has(object.handle)}
          handTucked={casting.prompt !== null}
          clearBottom={dock.clearBottom}
          onCard={handleCardAction}
          onPile={(pile, seatId) => {
            if (pile !== "graveyard") return;
            const owner =
              seatId === snapshot.viewerSeatId
                ? "Your"
                : `${seatNames.get(seatId) ?? "Player"}’s`;
            setPileView({
              title: `${owner} graveyard`,
              objects: seatBoard(snapshot, seatId).graveyard,
            });
          }}
        />
        <CombatLines lines={declaring?.lines ?? []} follow={follow} />
        <CastPrompt prompt={casting.prompt} />
        <DamagePills
          pills={damage?.pills ?? casting.pills}
          // A spell's shares stand above their cards, as approved (DC-02).
          prefer={damage ? "under" : "over"}
          follow={follow}
          clearBottom={dock.clearBottom}
        />
        <MatchStack
          snapshot={snapshot}
          seatNames={seatNames}
          faceOf={artFor}
          selectedId={reading.selectedId}
          onSelect={reading.select}
          pending={pendingObject(objectByHandle, casting.pending)}
          targets={
            casting.stackTargets.size > 0
              ? { ids: casting.stackTargets, choose: casting.chooseStack }
              : null
          }
          locked={casting.ui !== null}
          aside={casting.sourceFace !== null}
          follow={follow}
          clearBottom={dock.clearBottom}
        />
        <CastSource
          view={casting.sourceFace}
          stackCount={snapshot.stack.length}
          follow={follow}
          clearBottom={dock.clearBottom}
        />
        {result && (
          <GameEndBand
            open={bandShown}
            result={result}
            series={series}
            onView={() => setResultRaised(false)}
            onLeave={source.onLeave}
          />
        )}
        <TableChat
          open={chatOpen}
          messages={source.chat.messages}
          send={source.chat.send}
          watching={watching}
          draft={chatDraft}
          setDraft={setChatDraft}
          onSeen={seeChat}
          onClose={() => setChatOpen(false)}
        />
      </ImmersiveFrame>

      <ModeChoice choice={casting.modeChoice} />
      <MatchSettings
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        onConcede={
          watching || result
            ? undefined
            : () => {
                setSettingsOpen(false);
                setConceding("asking");
              }
        }
      />
      <ConcedeConfirmation
        open={conceding !== "closed"}
        pending={conceding === "sending"}
        failed={conceding === "failed"}
        onCancel={() => {
          // Back to the settings it came from, with every choice kept.
          setConceding("closed");
          setSettingsOpen(true);
        }}
        onConfirm={() => {
          setConceding("sending");
          void source.sendAction({ kind: "concede" });
        }}
      />

      {/* Card Inspector & Cast / Activate Dialog */}
      <Dialog
        open={inspected !== null}
        onClose={() => setInspectedHandle(null)}
        title={inspected ? objectName(inspected) : "Card"}
      >
        {inspected && (
          <div className={s.inspectorBody}>
            {artFor(inspected) && (
              <img
                src={artFor(inspected)}
                alt={objectName(inspected)}
                className={s.inspectorImg}
              />
            )}
            <div className={s.inspectorText}>
              {inspected.identity.kind === "known" && (
                <>
                  <strong>{inspected.identity.characteristics.typeLine}</strong>
                  {inspected.identity.characteristics.manaCost && (
                    <span>
                      Cost: {inspected.identity.characteristics.manaCost}
                    </span>
                  )}
                  <p style={{ margin: 0, whiteSpace: "pre-wrap" }}>
                    {inspected.identity.characteristics.rulesText ||
                      "No rules text."}
                  </p>
                </>
              )}
              {hasPriority && (
                <div className={s.decisionRow}>
                  {zoneByType.viewerHand?.visibility === "visible" &&
                    zoneByType.viewerHand.objects.includes(inspected.handle) &&
                    (inspected.publicProperties?.cardTypes.includes("land") ? (
                      <ActionButton
                        disabled={source.commandPending}
                        onClick={() => {
                          void source.sendAction({
                            kind: "play-land",
                            source: inspected.handle,
                          });
                          setInspectedHandle(null);
                        }}
                      >
                        Play land
                      </ActionButton>
                    ) : (
                      // Where the game offers casts, only an offered card
                      // may be cast, through the same steps as on the table.
                      (source.castOffers === undefined ||
                        casting.playable.has(inspected.handle)) && (
                        <ActionButton
                          disabled={source.commandPending}
                          onClick={() => {
                            if (source.castOffers === undefined)
                              void source.sendAction({
                                kind: "cast-untargeted",
                                source: inspected.handle,
                                payment: null,
                              });
                            else casting.begin(inspected.handle);
                            setInspectedHandle(null);
                          }}
                        >
                          {source.castOffers === undefined
                            ? "Cast spell"
                            : "Cast"}
                        </ActionButton>
                      )
                    ))}
                  {inspected.abilities.map((ab) => (
                    <ActionButton
                      key={ab.handle}
                      disabled={source.commandPending}
                      onClick={() => {
                        if (ab.kind === "mana") {
                          void source.sendAction({
                            kind: "activate-mana",
                            source: inspected.handle,
                            ability: ab.handle,
                          });
                        } else if (ab.kind === "cycling") {
                          void source.sendAction({
                            kind: "cycle",
                            source: inspected.handle,
                            ability: ab.handle,
                            payment: null,
                          });
                        } else {
                          void source.sendAction({
                            kind: "activate-untargeted-ability",
                            source: inspected.handle,
                            ability: ab.handle,
                            payment: null,
                          });
                        }
                        setInspectedHandle(null);
                      }}
                    >
                      {ab.label}
                    </ActionButton>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </Dialog>

      {/* Pile Browser Dialog */}
      <Dialog
        open={pileView !== null}
        onClose={() => setPileView(null)}
        title={pileView?.title ?? "Zone"}
      >
        {pileView &&
          (pileView.objects.length === 0 ? (
            <EmptyState
              title={`No cards in ${pileView.title.toLowerCase()}`}
              description="Cards moved here appear in this pile."
            />
          ) : (
            <div className={s.boardRow}>
              {pileView.objects.map((obj) => (
                <CardTile
                  key={obj.handle}
                  object={obj}
                  artUrl={artFor(obj)}
                  onClick={() => {
                    setPileView(null);
                    setInspectedHandle(obj.handle);
                  }}
                />
              ))}
            </div>
          ))}
      </Dialog>

      {/* Leave / Concede confirmation */}
      <ConfirmationDialog
        open={leaveOpen}
        appearance="match"
        title="Leave match?"
        description="Conceding ends the current game for your seat."
        confirmLabel="Concede & leave"
        cancelLabel="Stay at table"
        onCancel={() => setLeaveOpen(false)}
        onConfirm={() => {
          setLeaveOpen(false);
          void source.sendAction({ kind: "concede" }).finally(source.onLeave);
        }}
      />

      {/* Between games there is no game to concede, only a match to leave. */}
      <ConfirmationDialog
        open={leavingMatch}
        appearance="match"
        title="Leave match?"
        description="The match is not over. The next game starts without you."
        confirmLabel="Leave match"
        cancelLabel="Stay at table"
        onCancel={() => setLeavingMatch(false)}
        onConfirm={() => {
          setLeavingMatch(false);
          source.onLeave();
        }}
      />

      <FeedbackMessages
        label="Match messages"
        presentation="toast"
        items={
          source.commandError
            ? [
                {
                  id: "cmd-error",
                  text: source.commandError,
                  onDismiss: source.dismissCommandError,
                },
              ]
            : []
        }
      />
    </>
  );
}

export function MatchPage({ source }: { source: MatchSource }): ReactElement {
  if (source.status === "loading") {
    return (
      <section className={s.emptyShell} aria-label="Loading match">
        <LoadingState label="Connecting to table…">
          <LoadingShape width={260} height={36} />
        </LoadingState>
      </section>
    );
  }
  if (source.status === "idle") {
    return (
      <section className={s.emptyShell} aria-label="No active match">
        <EmptyState
          title="No active match"
          description="Start a game from Open Play, Collection, or League."
        />
      </section>
    );
  }
  if (source.status !== "ready") {
    return (
      <section className={s.emptyShell} aria-label="Match unavailable">
        <EmptyState
          title="Match unavailable"
          description="Sign in to join or resume a match."
        />
      </section>
    );
  }
  return <LiveMatch source={source} />;
}
