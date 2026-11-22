import { useMemo, useState, type ReactElement } from "react";
import {
  CircleUserRound,
  LogOut,
  MessageCircle,
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
  ChatComposer,
  ChatMessage,
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
import { useOrientation, useShellLayout } from "../../lib/display-scale";
import type { MatchSource, ReadyMatch } from "../../match/match-sources";
import * as s from "./match.css";

const ICON = designPixels(26);

const PHASE_MAP: Record<
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
  pending,
  onChoose,
}: {
  decision: PlayerDecision;
  objects: ReadonlyMap<string, PlayerObject>;
  seatNames: ReadonlyMap<string, string>;
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

  if (d.kind === "combat-damage") {
    const autoAllocations = d.sources.flatMap((src) =>
      src.targets.length > 0
        ? [{ source: src.source, target: src.targets[0]!, amount: src.total }]
        : [],
    );
    return (
      <div className={s.decisionCard} role="region" aria-label="Combat damage">
        <p className={s.decisionTitle}>Assign combat damage</p>
        <div className={s.decisionRow}>
          <ActionButton
            disabled={pending}
            onClick={() =>
              onChoose({ kind: "combat-damage", assignments: autoAllocations })
            }
          >
            Assign lethal / default damage
          </ActionButton>
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

function LiveMatch({ source }: { source: ReadyMatch }): ReactElement {
  const { snapshot, seatNames, artByPrintingId } = source;
  const isShort =
    useShellLayout() === "compact" && useOrientation() === "landscape";

  const opponents = useMemo(
    () => snapshot.players.filter((p) => p.seatId !== snapshot.viewerSeatId),
    [snapshot.players, snapshot.viewerSeatId],
  );
  const [viewedOpponentId, setViewedOpponentId] = useState<string | null>(
    opponents[0]?.seatId ?? null,
  );
  const activeOpponent =
    opponents.find((p) => p.seatId === viewedOpponentId) ??
    opponents[0] ??
    null;

  const [inspectedHandle, setInspectedHandle] = useState<string | null>(null);
  const [pileView, setPileView] = useState<{
    title: string;
    objects: PlayerObject[];
  } | null>(null);
  const [chatOpen, setChatOpen] = useState(false);
  const [chatDraft, setChatDraft] = useState("");
  const [leaveOpen, setLeaveOpen] = useState(false);

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
      oppGraveyard: activeOpponent
        ? find("graveyard", activeOpponent.seatId)
        : undefined,
    };
  }, [snapshot.zones, snapshot.viewerSeatId, activeOpponent]);

  const objectsIn = (zone: typeof zoneByType.battlefield): PlayerObject[] => {
    if (!zone || zone.visibility === "hidden") return [];
    return zone.objects
      .map((h) => objectByHandle.get(h))
      .filter((o): o is PlayerObject => Boolean(o));
  };

  const battlefieldObjects = objectsIn(zoneByType.battlefield);
  const viewerBattlefield = battlefieldObjects.filter(
    (o) => (o.controllerSeatId ?? o.ownerSeatId) === snapshot.viewerSeatId,
  );
  const oppBattlefield = activeOpponent
    ? battlefieldObjects.filter(
        (o) => (o.controllerSeatId ?? o.ownerSeatId) === activeOpponent.seatId,
      )
    : [];

  const viewerLands = viewerBattlefield.filter((o) =>
    o.publicProperties?.cardTypes.includes("land"),
  );
  const viewerNonLands = viewerBattlefield.filter(
    (o) => !o.publicProperties?.cardTypes.includes("land"),
  );
  const oppLands = oppBattlefield.filter((o) =>
    o.publicProperties?.cardTypes.includes("land"),
  );
  const oppNonLands = oppBattlefield.filter(
    (o) => !o.publicProperties?.cardTypes.includes("land"),
  );

  const viewerHand = objectsIn(zoneByType.viewerHand);
  const viewerGraveyard = objectsIn(zoneByType.viewerGraveyard);
  const exileObjects = objectsIn(zoneByType.exile);
  const commandObjects = objectsIn(zoneByType.command);

  const viewerPlayer = snapshot.players.find(
    (p) => p.seatId === snapshot.viewerSeatId,
  );

  const hasPriority =
    snapshot.turn?.prioritySeatIds.includes(snapshot.viewerSeatId) ?? false;
  const isViewerTurn =
    snapshot.turn?.activeSeatIds.includes(snapshot.viewerSeatId) ?? false;

  const activeDecision = snapshot.decisions.find((d) =>
    d.respondingSeatIds.includes(snapshot.viewerSeatId),
  );

  const phaseInfo = snapshot.turn
    ? PHASE_MAP[snapshot.turn.phase]
    : { phase: "beginning" as MatchPhase, label: "Opening" };

  const turnLabel = snapshot.turn
    ? isViewerTurn
      ? `Your turn · T${snapshot.turn.number}`
      : `${seatNames.get(snapshot.turn.activeSeatIds[0] ?? "") ?? "Opponent"}’s turn · T${snapshot.turn.number}`
    : "Preparing game";

  const artFor = (obj: PlayerObject): string | undefined =>
    obj.identity.kind === "known" && obj.identity.printingId
      ? artByPrintingId.get(obj.identity.printingId)
      : undefined;

  const inspected = inspectedHandle
    ? (objectByHandle.get(inspectedHandle) ?? null)
    : null;

  const handleCardAction = (
    obj: PlayerObject,
    zoneKind: "hand" | "battlefield",
  ) => {
    if (!hasPriority || source.commandPending) {
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

  return (
    <>
      <ImmersiveFrame
        playersLabel="Players"
        trayHeight={isShort ? 136 : 186}
        leading={
          <>
            <button
              type="button"
              className={frame.hudButton}
              aria-label="Table chat"
              onClick={() => setChatOpen(true)}
            >
              <MessageCircle size={ICON} aria-hidden="true" />
            </button>
            <span className={s.statusBadge}>
              {source.connectionState === "live"
                ? "Live"
                : source.connectionState === "reconnecting"
                  ? "Reconnecting…"
                  : source.connectionState === "ended"
                    ? "Ended"
                    : "Connecting…"}
            </span>
          </>
        }
        players={snapshot.players.map((player) => {
          const name = seatNames.get(player.seatId) ?? "Player";
          const takingTurn =
            snapshot.turn?.activeSeatIds.includes(player.seatId) ?? false;
          return (
            <PlayerIdentity
              key={player.seatId}
              presentation="seat"
              name={name}
              life={Number(player.life) || 0}
              actionLabel={`View ${name}`}
              onActivate={() => {
                if (player.seatId !== snapshot.viewerSeatId) {
                  setViewedOpponentId(player.seatId);
                }
              }}
              takingTurn={takingTurn}
              portrait={<CircleUserRound aria-hidden="true" />}
            />
          );
        })}
        trailing={
          <button
            type="button"
            className={frame.hudButton}
            aria-label="Leave table"
            onClick={() => setLeaveOpen(true)}
          >
            <LogOut size={ICON} aria-hidden="true" />
          </button>
        }
        side={
          <>
            <TurnPhaseDisplay
              turnLabel={turnLabel}
              phase={phaseInfo.phase}
              phaseLabel={phaseInfo.label}
            />
            {viewerPlayer && viewerPlayer.manaPool.length > 0 && (
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
            <MatchActions
              primary={
                hasPriority
                  ? {
                      label:
                        snapshot.stack.length > 0 ? "Resolve" : "Pass priority",
                      pending: source.commandPending,
                      onActivate: () =>
                        void source.sendAction({ kind: "pass" }),
                    }
                  : {
                      label: "Waiting…",
                      disabled: true,
                      onActivate: () => undefined,
                    }
              }
              {...(activeDecision
                ? { status: `Action required: ${activeDecision.kind}` }
                : hasPriority
                  ? { status: "You have priority" }
                  : {})}
            />
          </>
        }
        tray={
          <div className={s.handTray} aria-label="Your hand and zones">
            <div className={s.zoneCluster}>
              <button
                type="button"
                className={s.zonePill}
                onClick={() =>
                  setPileView({ title: "Graveyard", objects: viewerGraveyard })
                }
              >
                GY ({viewerGraveyard.length})
              </button>
              <button
                type="button"
                className={s.zonePill}
                onClick={() =>
                  setPileView({ title: "Exile", objects: exileObjects })
                }
              >
                Exile ({exileObjects.length})
              </button>
              {commandObjects.length > 0 && (
                <button
                  type="button"
                  className={s.zonePill}
                  onClick={() =>
                    setPileView({ title: "Command", objects: commandObjects })
                  }
                >
                  Cmd ({commandObjects.length})
                </button>
              )}
              <span className={s.zonePill} aria-label="Library count">
                Lib ({viewerPlayer?.libraryCount ?? "0"})
              </span>
            </div>
            {viewerHand.map((card) => (
              <CardTile
                key={card.handle}
                object={card}
                artUrl={artFor(card)}
                playable={hasPriority}
                selected={inspectedHandle === card.handle}
                onClick={() => handleCardAction(card, "hand")}
              />
            ))}
          </div>
        }
      >
        <div className={s.tableSurface}>
          {/* Opponent battlefield */}
          <section className={s.boardHalf} aria-label="Opponent battlefield">
            <div className={s.boardHeader}>
              <span>
                {activeOpponent
                  ? (seatNames.get(activeOpponent.seatId) ?? "Opponent")
                  : "Opponent"}
              </span>
              {activeOpponent && (
                <span>
                  Hand {activeOpponent.handCount} · Library{" "}
                  {activeOpponent.libraryCount}
                </span>
              )}
            </div>
            <div className={s.boardRow}>
              {oppLands.map((obj) => (
                <CardTile
                  key={obj.handle}
                  object={obj}
                  artUrl={artFor(obj)}
                  selected={inspectedHandle === obj.handle}
                  onClick={() => setInspectedHandle(obj.handle)}
                />
              ))}
            </div>
            <div className={s.boardRow}>
              {oppNonLands.map((obj) => (
                <CardTile
                  key={obj.handle}
                  object={obj}
                  artUrl={artFor(obj)}
                  selected={inspectedHandle === obj.handle}
                  onClick={() => setInspectedHandle(obj.handle)}
                />
              ))}
            </div>
          </section>

          {/* Center strip: Stack, opening choice, or pending engine decision */}
          <div className={s.centerStrip}>
            {snapshot.stack.length > 0 && (
              <div className={s.decisionRow} aria-label="Spell stack">
                {snapshot.stack.map((entry) => (
                  <span key={entry.handle} className={s.stackChip}>
                    {entry.label} (
                    {seatNames.get(entry.controllerSeatId) ?? "Player"})
                  </span>
                ))}
              </div>
            )}
            {source.chooseOpening && (
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
            {activeDecision && (
              <DecisionPanel
                key={activeDecision.decision}
                decision={activeDecision}
                objects={objectByHandle}
                seatNames={seatNames}
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

          {/* Viewer battlefield */}
          <section className={s.boardHalf} aria-label="Your battlefield">
            <div className={s.boardRow}>
              {viewerNonLands.map((obj) => (
                <CardTile
                  key={obj.handle}
                  object={obj}
                  artUrl={artFor(obj)}
                  playable={
                    hasPriority && obj.abilities.length > 0 && !obj.tapped
                  }
                  selected={inspectedHandle === obj.handle}
                  onClick={() => handleCardAction(obj, "battlefield")}
                />
              ))}
            </div>
            <div className={s.boardRow}>
              {viewerLands.map((obj) => (
                <CardTile
                  key={obj.handle}
                  object={obj}
                  artUrl={artFor(obj)}
                  playable={hasPriority && !obj.tapped}
                  selected={inspectedHandle === obj.handle}
                  onClick={() => handleCardAction(obj, "battlefield")}
                />
              ))}
            </div>
          </section>
        </div>
      </ImmersiveFrame>

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
                      <ActionButton
                        disabled={source.commandPending}
                        onClick={() => {
                          void source.sendAction({
                            kind: "cast-untargeted",
                            source: inspected.handle,
                            payment: null,
                          });
                          setInspectedHandle(null);
                        }}
                      >
                        Cast spell
                      </ActionButton>
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

      {/* Match Chat Dialog */}
      <Dialog
        open={chatOpen}
        onClose={() => setChatOpen(false)}
        title="Table chat"
      >
        <div className={s.chatDrawer}>
          <div className={s.chatList}>
            {source.chat.messages.length === 0 ? (
              <EmptyState
                title="No messages yet"
                description="Chat with players at this table."
              />
            ) : (
              source.chat.messages.map((msg) => (
                <ChatMessage
                  key={msg.id}
                  surface="table"
                  author={msg.senderName}
                  body={msg.body}
                />
              ))
            )}
          </div>
          {source.chat.send && (
            <ChatComposer
              surface="table"
              mode="editable"
              value={chatDraft}
              maxCharacters={500}
              onChange={setChatDraft}
              onSend={(draft) => {
                setChatDraft("");
                void source.chat.send?.(draft);
              }}
              placeholder="Send a table message…"
            />
          )}
        </div>
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
