import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  Ellipsis,
  MessageCircle,
  MessagesSquare,
  Settings as Gear,
} from "lucide-react";
import {
  ActionButton,
  MatchActions,
  PlayerAvatar,
  PlayerIdentity,
  TargetConnection,
  TurnPhaseDisplay,
} from "@spellwake/ui";
import type { ScreenProps } from "../../review-types";
import { Battlefield, center, piecesFor, type Piece } from "./battlefield";
import { portrait, seats } from "./assets";
import {
  CastingChoices,
  CastingSource,
  defaultModes,
  legalCastPiece,
  MatchStack,
  modeViewFor,
  modeViews,
  peekState,
  pickCast,
} from "./casting";
import {
  Concede,
  feedbackBoard,
  FeedbackBoard,
  Settings,
  TableChat,
  type Preferences,
} from "./panels";
import DamageScene from "./DamageScene";
import "./match.css";

const startsOverview = (id: string) =>
  [
    "NP-03",
    "CM-09",
    "CM-11",
    "AT-05",
    "AT-06",
    "AT-10",
    "AS09",
    "BC03",
  ].includes(id) ||
  id.startsWith("BL-") ||
  (id.startsWith("DA") && !["DA08", "DA09"].includes(id));
const attackStages = [
  "AT-01",
  "AT-02",
  "AT-03",
  "AT-11",
  "AT-04",
  "AT-05",
  "AT-06",
  "AT-10",
];
const blockStages = ["BL-01", "BL-02", "BL-03", "BL-04", "BL-05"];
const activeCast = (family: string) =>
  ["BC", "MC", "MM", "MT", "MX", "X"].includes(family);
const turnSteps = [
  { phase: "first-main", label: "Main phase" },
  { phase: "combat", label: "Combat" },
  { phase: "second-main", label: "Second main" },
  { phase: "end", label: "End" },
  { phase: "beginning", label: "Beginning" },
] as const;
interface MatchCarry {
  match: true;
  draft: string;
  sent: string[];
  changed: Preferences | null;
  modes: { id: string; modes: string[] } | null;
  target: { id: string; x: number; y: number } | null;
  defenders: { bear: string; angel: string };
  priorChoice: string;
  settingsOrigin: string;
  commitOrigin: string;
}

const point = (x: number, y: number) => ({ x, y });
const seatPoint = (name: string) => point(274 + seats.indexOf(name) * 187, 42);
const dispatchDamage = (id: string) =>
  window.dispatchEvent(
    new CustomEvent("spellwake:damage-target", { detail: id }),
  );

/** Full native-size gameplay review. Every card, HUD action and overlay is a
 * separately rendered surface. Navigation IDs refer to the approved section fixtures. */
export default function MatchScreen({ section, view, go, data }: ScreenProps) {
  const carry =
    data && typeof data === "object" && "match" in data && data.match === true
      ? (data as MatchCarry)
      : null;
  const id = view.id;
  const family = section === "x-value" ? "X" : id.slice(0, 2);
  const damage = family === "DA" || family === "DC",
    watching = ["CM-09", "CM-11"].includes(id);
  const [cameraOverride, changeCamera] = useState<{
    id: string;
    seat: string;
  } | null>(null);
  const camera =
    cameraOverride?.id === id
      ? cameraOverride.seat
      : startsOverview(id)
        ? "Table"
        : id === "NP-02"
          ? "Alex"
          : "You";
  const overview = camera === "Table";
  const [overlay, setOverlay] = useState<{
    id: string;
    type: "chat" | "settings" | "concede";
  } | null>(null);
  const visibleOverlay = overlay?.id === id ? overlay.type : null;
  const [draft, storeDraft] = useState(carry?.draft ?? "");
  const [sent, storeSent] = useState(carry?.sent ?? []);
  const [phaseStep, setPhaseStep] = useState(0);
  const [xStage, setXStage] = useState<"choose" | "target" | "stack">("choose");
  const [x, setX] = useState(3);
  const [damageTargets, setDamageTargets] = useState<string[]>([]);
  useLayoutEffect(() => {
    const listen = (e: Event) => {
      const ids: unknown = (e as CustomEvent).detail;
      if (Array.isArray(ids) && ids.every((i) => typeof i === "string"))
        setDamageTargets(ids);
    };
    window.addEventListener("spellwake:damage-selection", listen);
    return () =>
      window.removeEventListener("spellwake:damage-selection", listen);
  }, []);
  const [changed, setChanged] = useState<Preferences | null>(
    carry?.changed ?? null,
  );
  const prefs =
    changed ??
    (id === "CM-07"
      ? { levels: [58, 0, 40], auto: false }
      : { levels: [80, 50, 80], auto: true });
  const [chosen, setChosen] = useState<{ id: string; modes: string[] } | null>(
    carry?.modes ?? null,
  );
  const modes = chosen?.id === id ? chosen.modes : defaultModes(id);
  const [target, setTarget] = useState<{
    id: string;
    x: number;
    y: number;
  } | null>(carry?.target ?? null);
  const [defenders, assignDefender] = useState(
    carry?.defenders ?? { bear: "Alex", angel: "Morgan" },
  );
  const priorChoice = useRef(carry?.priorChoice ?? "MM01"),
    settingsOrigin = useRef(carry?.settingsOrigin ?? "CM-06"),
    commitOrigin = useRef(carry?.commitOrigin ?? "AT-03");
  const board = piecesFor(id, family, overview, watching, camera);
  const isAttack = family === "AT",
    isBlock = family === "BL",
    combat = (damage && family === "DA") || isAttack || isBlock;
  const needDefender = ["AT-02", "AT-11", "AT-05", "AT-10"].includes(id);
  const showChat =
    ["CM-02", "CM-03", "CM-04", "CM-11"].includes(id) ||
    visibleOverlay === "chat";
  const showSettings =
    ["CM-06", "CM-07"].includes(id) || visibleOverlay === "settings";
  const showConcede = id === "CM-08" || visibleOverlay === "concede";
  const isBoard = feedbackBoard(id) && id !== "DA09";
  const isMode = modeViews.includes(id),
    isPeek = peekState(id);
  function navigate(to: string, changes: Partial<MatchCarry> = {}) {
    setOverlay(null);
    go(to, {
      match: true,
      draft,
      sent,
      changed,
      modes: chosen,
      target,
      defenders,
      priorChoice: priorChoice.current,
      settingsOrigin: settingsOrigin.current,
      commitOrigin: commitOrigin.current,
      ...changes,
    } satisfies MatchCarry);
  }
  function closeOverlay() {
    if (visibleOverlay) {
      setOverlay(null);
      return;
    }
    if (id === "CM-08") {
      navigate(settingsOrigin.current);
      return;
    }
    if (id === "CM-11") navigate("CM-09");
    else if (family === "CM") navigate("CM-01");
  }
  function toggleChat() {
    if (showChat) {
      closeOverlay();
      return;
    }
    if (family === "CM") navigate(watching ? "CM-11" : "CM-02");
    else setOverlay({ id, type: "chat" });
  }
  function toggleSettings() {
    if (showSettings) {
      closeOverlay();
      return;
    }
    if (family === "CM" && !watching) navigate(changed ? "CM-07" : "CM-06");
    else setOverlay({ id, type: "settings" });
  }
  function overviewClick() {
    const next: Record<string, string> = {
      "NP-01": "NP-03",
      "NP-02": "NP-03",
      "NP-03": "NP-01",
      "NP-04": "NP-03",
      "AT-02": "AT-05",
      "AT-05": "AT-02",
      "AT-06": "AT-03",
      "AT-10": "AT-02",
      AS01: "AS09",
      AS09: "AS01",
      BC02: "BC03",
      BC03: "BC02",
      DA01: "DA08",
      DA08: "DA01",
    };
    if (next[id]) navigate(next[id]);
    else changeCamera({ id, seat: overview ? "You" : "Table" });
  }
  function chooseSeat(name: string) {
    if (family === "DC" && !["DC00", "DC10", "DC11"].includes(id)) {
      dispatchDamage(name);
      return;
    }
    if (needDefender && name !== "You") {
      const next = { ...defenders, [id === "AT-11" ? "angel" : "bear"]: name };
      assignDefender(next);
      navigate(id === "AT-11" ? "AT-04" : "AT-03", { defenders: next });
      return;
    }
    if (family === "X" && xStage === "target") {
      setXStage("stack");
      return;
    }
    if (["BC02", "BC03"].includes(id)) {
      navigate("BC04", { target: { id: "BC04", ...seatPoint(name) } });
      return;
    }
    if (
      ["NP-01", "NP-02", "NP-03", "NP-04"].includes(id) &&
      ["You", "Alex"].includes(name)
    ) {
      navigate(name === "You" ? "NP-01" : "NP-02");
      return;
    }
    if (id === "AS09" && name === "You") {
      navigate("AS01");
      return;
    }
    changeCamera({ id, seat: name });
  }
  function legal(p: Piece) {
    if (family === "DC") {
      if (id === "DC00") return p.zone === "hand" && p.face === "arc";
      return (
        (damageTargets.length < 3 || damageTargets.includes(p.id)) &&
        !["DC10", "DC11"].includes(id) &&
        !p.zone &&
        !p.back &&
        ["elf", "bear", "angel", "chandra", "thopter"].includes(p.face)
      );
    }
    if (family === "X")
      return (
        xStage === "target" &&
        !p.zone &&
        !p.back &&
        ["elf", "bear", "angel", "chandra", "thopter"].includes(p.face)
      );
    if (activeCast(family)) return legalCastPiece(id, p);
    if (isAttack && attackStages.includes(id)) {
      if (overview)
        return (
          (p.seat === "Alex" && p.face === "chandra") ||
          (p.seat === "You" && ["bear", "angel"].includes(p.face))
        );
      return !p.zone && ["bear", "other-angel", "elf"].includes(p.id);
    }
    if (isBlock && blockStages.includes(id))
      return (
        ["You-10", "You-12"].includes(p.id) ||
        (["BL-02", "BL-04"].includes(id) && p.id === "Alex-10")
      );
    return false;
  }
  function choosePiece(p: Piece) {
    if (family === "DC") {
      if (id === "DC00") navigate("DC01");
      else dispatchDamage(p.id);
      return;
    }
    if (activeCast(family)) {
      if (family === "X") {
        setXStage("stack");
        return;
      }
      if (["BC02", "BC03"].includes(id)) {
        const next = { id: "BC09", ...center(p) };
        setTarget(next);
        navigate("BC09", { target: next });
        return;
      }
      if (family === "MT" && ["MT01", "MT03", "MT05"].includes(id)) {
        const next = { id: p.face === "angel" ? "MT04" : "MT02", ...center(p) };
        setTarget(next);
        navigate(next.id, { target: next });
        return;
      }
      pickCast(id, p, navigate);
      return;
    }
    if (isAttack) {
      if (p.seat === "Alex" && p.face === "chandra") {
        navigate("AT-06");
        return;
      }
      if (id === "AT-06" && p.seat === "You") {
        navigate("AT-10");
        return;
      }
      if (id === "AT-02" && p.face === "bear") {
        navigate("AT-01");
        return;
      }
      if (id === "AT-03" && p.face === "bear") {
        navigate("AT-01");
        return;
      }
      if (id === "AT-04" || id === "AT-11") {
        navigate("AT-03");
        return;
      }
      navigate(id === "AT-03" ? "AT-11" : "AT-02");
      return;
    }
    if (isBlock) {
      if (p.seat === "Alex") {
        if (id === "BL-02" || id === "BL-04")
          navigate(id === "BL-02" ? "BL-03" : "BL-05");
        return;
      }
      if (id === "BL-02") navigate("BL-01");
      else if (id === "BL-03") navigate(p.face === "elf" ? "BL-04" : "BL-01");
      else if (id === "BL-04" || id === "BL-05") navigate("BL-03");
      else navigate("BL-02");
    }
  }
  function returnChoice() {
    navigate(
      family === "MM" ? priorChoice.current : family === "MC" ? "MC01" : "MX01",
    );
  }
  function escape() {
    if (showChat || showSettings || showConcede) {
      closeOverlay();
      return;
    }
    const esc: Record<string, string> = {
      "AT-02": "AT-01",
      "AT-11": "AT-03",
      "BL-02": "BL-01",
      "BL-04": "BL-03",
      AS02: "AS01",
      AS04: "AS03",
    };
    if (esc[id]) navigate(esc[id]);
    else if (isPeek) returnChoice();
  }
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) escape();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });
  useEffect(() => {
    if (!carry) return;
    const next = (
      { BC05: "BC06", AS10: "AS05", AS05: "AS06" } as Record<string, string>
    )[id];
    if (!next) return;
    const timer = window.setTimeout(() => go(next, carry), 1100);
    return () => window.clearTimeout(timer);
  }, [id, carry, go]);
  const currentTurn =
    isBlock || family === "AS" || watching ? "Alex’s turn" : "Your turn";
  const spellStack = family === "AS" || family === "MX";
  const hudStatus: Record<string, string> = {
    "AT-08": "Attackers declared",
    "AT-09": "No attacks declared",
    "BL-07": "Blocks declared",
    "BL-08": "No blocks declared",
    AS05: "Resolving…",
  };
  const assigned = ["AT-03", "AT-04", "AT-06", "AT-08", "AT-11"].includes(id),
    blocked = ["BL-03", "BL-04", "BL-05", "BL-07"].includes(id);
  function coord(pieceId: string, fallback: { x: number; y: number }) {
    const p = board.find((n) => n.id === pieceId);
    return p ? center(p) : fallback;
  }
  const bearPoint = coord(overview ? "You-11" : "bear", point(775, 548)),
    angelPoint = coord(overview ? "You-10" : "other-angel", point(920, 548));
  const attackPiece = coord(isBlock ? "Alex-10" : "Alex-9", point(130, 466));
  const boardLines: {
    from: { x: number; y: number };
    to: { x: number; y: number };
    tone: string;
  }[] = [];
  if (isAttack && assigned)
    boardLines.push({
      from: bearPoint,
      to: id === "AT-06" ? attackPiece : seatPoint(defenders.bear),
      tone: "attack",
    });
  if (id === "AT-04")
    boardLines.push({
      from: angelPoint,
      to: seatPoint(defenders.angel),
      tone: "attack",
    });
  if (isBlock && !isBoard)
    boardLines.push({
      from: attackPiece,
      to: seatPoint("You"),
      tone: "attack",
    });
  if (isBlock && blocked)
    boardLines.push({
      from: coord("You-10", angelPoint),
      to: attackPiece,
      tone: "block",
    });
  if (id === "BL-05")
    boardLines.push({
      from: coord("You-12", point(325, 653)),
      to: attackPiece,
      tone: "block",
    });
  const selected: string[] = family === "DC" ? [...damageTargets] : [];
  if (isAttack && assigned) selected.push("bear");
  if (["AT-11", "AT-04"].includes(id)) selected.push("other-angel");
  if (id === "AT-02") selected.push("bear");
  if (["BC04", "BC05", "BC09"].includes(id)) selected.push("land4");
  const castingControls =
    activeCast(family) &&
    !["BC01", "BC08", "MX00"].includes(id) &&
    !id.endsWith("00");
  const mayNext =
    family === "NP" ||
    (family === "CM" && !watching) ||
    id.endsWith("00") ||
    ["BC01", "BC08"].includes(id);

  const status = hudStatus[id];
  function onModeChange(ids: string[]) {
    const dst = modeViewFor(ids, family);
    const choice = { id: dst, modes: ids };
    setChosen(choice);
    if (dst !== id) navigate(dst, { modes: choice });
  }
  const scaleX = view.width / 1584,
    scaleY = view.height / 993;
  return (
    <div
      className="match-screen"
      style={{ width: view.width, height: view.height }}
    >
      <div
        className={
          "match-scene" +
          (family === "NP" || family === "CM" || isAttack || isBlock
            ? " dark-table"
            : "")
        }
        style={{ transform: `scale(${scaleX},${scaleY})` }}
        data-match-view={id}
      >
        {id === "DA09" ? (
          <DamageScene id={id} go={go} data={data} />
        ) : isBoard ? (
          <FeedbackBoard id={id} go={navigate} origin={commitOrigin.current} />
        ) : (
          <>
            <Battlefield
              id={id}
              family={family}
              camera={camera}
              watching={watching}
              overview={overview}
              legal={legal}
              onChoose={choosePiece}
              selected={selected}
            />
            {boardLines.map((l, i) => (
              <div key={i} className={"match-line " + l.tone}>
                <TargetConnection
                  from={l.from}
                  to={l.to}
                  bend={isBlock ? 30 : 60}
                />
              </div>
            ))}
            <div className="match-corners left">
              <button
                type="button"
                aria-label="Settings"
                onClick={toggleSettings}
              >
                <Gear />
              </button>
              <button
                type="button"
                aria-label="Table chat"
                data-unread={id === "CM-01" || undefined}
                onClick={toggleChat}
              >
                <MessageCircle />
              </button>
            </div>
            <div className="match-seats">
              <button
                type="button"
                className="match-overview"
                aria-label="Table overview"
                onClick={overviewClick}
              >
                <Ellipsis />
              </button>
              {seats.map((name, i) => (
                <div
                  className={
                    "match-seat" +
                    ((needDefender && i > 0) ||
                    ["BC02", "BC03"].includes(id) ||
                    (family === "X" && xStage === "target") ||
                    (family === "DC" && id === "DC01")
                      ? " offered-seat"
                      : "")
                  }
                  key={name}
                >
                  <PlayerIdentity
                    presentation="seat"
                    name={name}
                    life={40}
                    actionLabel={"View " + name}
                    onActivate={() => {
                      if (
                        needDefender ||
                        family === "DC" ||
                        (family === "X" && xStage === "target") ||
                        ["BC02", "BC03"].includes(id)
                      )
                        changeCamera({ id, seat: name });
                      else chooseSeat(name);
                    }}
                    pressed={!overview && camera === name}
                    takingTurn={
                      name === (currentTurn === "Your turn" ? "You" : "Alex")
                    }
                    viewingOpponent={
                      !overview && camera === name && name !== "You"
                    }
                    portrait={
                      <PlayerAvatar diameter={39}>
                        <img src={portrait(i)} alt="" />
                      </PlayerAvatar>
                    }
                  />
                  {((needDefender && i > 0) ||
                    ["BC02", "BC03"].includes(id) ||
                    (family === "X" && xStage === "target") ||
                    (family === "DC" &&
                      !["DC00", "DC10", "DC11"].includes(id) &&
                      (damageTargets.length < 3 ||
                        damageTargets.includes(name)))) && (
                    <button
                      type="button"
                      className="match-target-seat"
                      aria-label={(needDefender ? "Attack " : "Target ") + name}
                      onClick={() => chooseSeat(name)}
                    />
                  )}
                </div>
              ))}
            </div>
            <div className="match-corners right">
              <button
                type="button"
                aria-label="Messages"
                onClick={() => navigate("MSG-S01")}
              >
                <MessagesSquare />
              </button>
              <button
                type="button"
                aria-label="Your profile"
                onClick={() => navigate("PP-01")}
              >
                <img src={portrait(0)} alt="" />
              </button>
            </div>
            <aside className="match-phase-controls">
              <TurnPhaseDisplay
                turnLabel={currentTurn}
                phase={combat ? "combat" : turnSteps[phaseStep]!.phase}
                phaseLabel={
                  combat
                    ? "Combat"
                    : phaseStep === 0 && (family === "AS" || activeCast(family))
                      ? "Main"
                      : turnSteps[phaseStep]!.label
                }
              />
              {status ? (
                <i role="status">{status}</i>
              ) : isAttack && attackStages.includes(id) ? (
                <MatchActions
                  primary={{
                    label: "Attack",
                    disabled: !assigned || needDefender,
                    onActivate: () => {
                      commitOrigin.current = id;
                      navigate("AT-08");
                    },
                  }}
                  secondary={{
                    label: "No Attacks",
                    onActivate: () => navigate("AT-09"),
                  }}
                />
              ) : isBlock && blockStages.includes(id) ? (
                <MatchActions
                  primary={{
                    label: "Block",
                    disabled: !blocked || id === "BL-04",
                    onActivate: () => navigate("BL-07"),
                  }}
                  secondary={{
                    label: "No Blocks",
                    onActivate: () => navigate("BL-08"),
                  }}
                />
              ) : spellStack &&
                [
                  "AS01",
                  "AS02",
                  "AS03",
                  "AS04",
                  "AS09",
                  "AS10",
                  "MX00",
                ].includes(id) ? (
                <MatchActions
                  primary={{
                    label: "Resolve",
                    pending: id === "AS10",
                    onActivate: () => navigate("AS10"),
                  }}
                />
              ) : mayNext && !damage && !castingControls ? (
                <MatchActions
                  primary={{
                    label: "Next",
                    onActivate: () =>
                      setPhaseStep((phaseStep + 1) % turnSteps.length),
                  }}
                />
              ) : null}
            </aside>
            {family === "NP" && id === "NP-04" && (
              <span className="match-phase-tip">First main</span>
            )}
            {damage ? (
              <DamageScene id={id} go={go} data={data} />
            ) : (
              <>
                {activeCast(family) &&
                  !isMode &&
                  !isPeek &&
                  !(family === "X" && xStage === "stack") && (
                    <CastingSource
                      key={id}
                      id={id}
                      family={family}
                      go={navigate}
                      selection={
                        chosen?.modes ??
                        (id === "MM06" ? ["return", "draw"] : modes)
                      }
                      draftTarget={target ?? undefined}
                      x={x}
                      setX={setX}
                      xStage={xStage === "target" ? "target" : "choose"}
                      setXStage={setXStage}
                    />
                  )}
                <MatchStack
                  key={id + "stack"}
                  id={family === "X" && xStage === "stack" ? "X-stack" : id}
                  family={family}
                  go={navigate}
                />
                {isMode && (
                  <CastingChoices
                    id={id}
                    family={family}
                    selected={modes}
                    setSelected={onModeChange}
                    go={navigate}
                    peek={() => {
                      priorChoice.current = id;
                      navigate(
                        family === "MM"
                          ? "MM04"
                          : family === "MX"
                            ? "MX02"
                            : "MC03",
                      );
                    }}
                  />
                )}
                {isPeek && (
                  <div className="match-peek">
                    <ActionButton onClick={returnChoice}>
                      Return to choice
                    </ActionButton>
                    <ActionButton onClick={() => navigate(family + "00")}>
                      Cancel
                    </ActionButton>
                  </div>
                )}
              </>
            )}
            {showChat && (
              <div className="match-chat-location">
                <TableChat
                  key={id}
                  kind={
                    watching
                      ? "watch"
                      : id === "CM-04"
                        ? "sent"
                        : id === "CM-03"
                          ? "draft"
                          : "read"
                  }
                  close={closeOverlay}
                  go={navigate}
                  stored={draft}
                  onStore={storeDraft}
                  savedMessages={sent}
                  onAccept={(s) => {
                    const next = [...sent, s];
                    storeSent(next);
                    storeDraft("");
                    if (!watching && family === "CM" && id !== "CM-04")
                      navigate("CM-04", { sent: next, draft: "" });
                  }}
                />
              </div>
            )}
            {(showSettings || showConcede) && (
              <div
                className="match-modal-shade"
                onMouseDown={(e) => {
                  if (e.target === e.currentTarget) closeOverlay();
                }}
              >
                {showConcede ? (
                  <Concede
                    close={closeOverlay}
                    confirm={() => navigate("CM-09")}
                  />
                ) : (
                  <Settings
                    prefs={prefs}
                    change={(p) => {
                      setChanged(p);
                      if (id === "CM-06") navigate("CM-07", { changed: p });
                    }}
                    close={closeOverlay}
                    {...(watching
                      ? {}
                      : {
                          concede: () => {
                            settingsOrigin.current = id;
                            if (family === "CM") navigate("CM-08");
                            else setOverlay({ id, type: "concede" });
                          },
                        })}
                  />
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
