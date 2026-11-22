import { useState } from "react";
import {
  ActionButton,
  CompactSpellStack,
  ManaCost,
  MatchActions,
  OriginalCardFace,
  PrintedModePicker,
  TargetConnection,
  TargetPrompt,
  XValuePicker,
  type PrintedMode,
  type StackEntry,
} from "@spellwake/ui";
import {
  abradeModes,
  bondModes,
  cardImage,
  crypticModes,
  portrait,
  sourceFor,
  type Card,
} from "./assets";
import type { Piece } from "./battlefield";
import warden from "../../../../../.impeccable/review/open-play/in-game/arena-reference/soul-warden-art.jpg?inline";
import redPayment from "../../../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-15/red-payment.png?inline";
import red from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/R.svg?inline";
import one from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/1.svg?inline";
import two from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/2.svg?inline";
import three from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/3.svg?inline";
import four from "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/4.svg?inline";
const generic = [undefined, one, two, three, four];
export const modeViews = [
  "MC01",
  "MC02",
  "MM01",
  "MM02",
  "MM03",
  "MM05",
  "MX01",
];
const peekViews = ["MC03", "MM04", "MX02"];
export const defaultModes = (id: string): string[] =>
  (
    ({
      MM02: ["draw"],
      MM03: ["tap", "draw"],
      MM05: ["return", "draw"],
      MX01: ["counter", "return"],
    }) as Record<string, string[]>
  )[id] ?? [];
export function modeViewFor(selected: string[], family: string) {
  if (family === "MX") return "MX01";
  return selected.length === 0
    ? "MM01"
    : selected.length === 1
      ? "MM02"
      : selected.includes("return")
        ? "MM05"
        : "MM03";
}
export function CastingChoices({
  id,
  family,
  selected,
  setSelected,
  go,
  peek,
}: {
  id: string;
  family: string;
  selected: string[];
  setSelected: (s: string[]) => void;
  go: (s: string) => void;
  peek: () => void;
}) {
  if (!modeViews.includes(id)) return null;
  const single = family === "MC";
  const modes = single
    ? abradeModes.map((m, i) =>
        id === "MC02" && i === 1
          ? { ...m, unavailableReason: "No legal targets" }
          : m,
      )
    : crypticModes.map((m, i) =>
        family === "MM" && i === 0
          ? { ...m, unavailableReason: "No legal targets" }
          : m,
      );
  return (
    <div className="match-mode-screen">
      <div className="mode-top">
        <button type="button" onClick={peek}>
          View Battlefield
        </button>
      </div>
      <h1>{single ? "Choose one" : "Choose two"}</h1>
      <div className="match-mode-cards">
        <PrintedModePicker
          image={cardImage(single ? "abrade" : "cryptic")}
          modes={modes}
          faceWidth={single ? 316 : 322}
          gap={single ? 30 : 12}
          choice={
            single
              ? {
                  kind: "one",
                  onChoose: (k) => go(k === "damage" ? "MC04" : "MC05"),
                }
              : { kind: "multiple", count: 2, selected, onChange: setSelected }
          }
        />
      </div>
      <div className="mode-footer">
        <ActionButton onClick={() => go(family + "00")}>Cancel</ActionButton>
        {!single && (
          <ActionButton
            emphasis="primary"
            disabled={selected.length !== 2}
            onClick={() =>
              go(
                family === "MX"
                  ? "MX03"
                  : selected.includes("return")
                    ? "MM06"
                    : "MM07",
              )
            }
          >
            Next
          </ActionButton>
        )}
      </div>
    </div>
  );
}
export function legalCastPiece(id: string, p: Piece) {
  const f = id.slice(0, 2),
    source = sourceFor(f),
    creature = ["elf", "bear", "angel", "thopter", "krenko"].includes(p.face);
  if (p.zone === "hand")
    return (
      (id === "BC01" || id.endsWith("00")) &&
      (p.face === source || (id === "BC01" && p.face === "krenko"))
    );
  if (p.back) return false;
  switch (id) {
    case "BC02":
    case "BC03":
      return creature || p.face === "chandra";
    case "BC04":
    case "BC09":
      return p.face === "mountain";
    case "MC04":
      return creature;
    case "MC05":
      return p.face === "ball" || p.face === "thopter";
    case "MM06":
    case "MX04":
    case "MX06":
      return true;
    case "MT01":
    case "MT02":
    case "MT03":
    case "MT04":
    case "MT05":
      return creature;
    default:
      return false;
  }
}
export function pickCast(id: string, p: Piece, go: (s: string) => void) {
  const family = id.slice(0, 2);
  if (p.zone === "hand") {
    go(id === "BC01" ? (p.face === "krenko" ? "BC07" : "BC02") : family + "01");
    return;
  }
  if (id === "BC02" || id === "BC03") go("BC09");
  else if (id === "BC04" || id === "BC09") go("BC06");
  else if (id === "MC04" || id === "MC05") go("MC06");
  else if (id === "MM06") go("MM07");
  else if (id === "MX04" || id === "MX06") go("MX08");
  else if (["MT01", "MT03", "MT05"].includes(id))
    go(p.face === "angel" ? "MT04" : "MT02");
  else if (["MT02", "MT04"].includes(id)) go("MT06");
}
const spellStages = ["MX03", "MX05", "MX07"],
  permanentStages = ["MX04", "MX06"];
export const peekState = (id: string) => peekViews.includes(id);
export function CastingSource({
  id,
  family,
  go,
  draftTarget,
  selection,
  x,
  setX,
  xStage,
  setXStage,
}: {
  id: string;
  family: string;
  go: (s: string) => void;
  draftTarget?: { x: number; y: number } | undefined;
  selection: string[];
  x: number;
  setX: (n: number) => void;
  xStage: "choose" | "target";
  setXStage: (s: "choose" | "target") => void;
}) {
  const xPick = family === "X",
    single = family === "MC",
    multiple = family === "MM",
    sequential = family === "MT",
    mixed = family === "MX",
    basic = family === "BC";
  if (
    !xPick &&
    (id.endsWith("00") ||
      modeViews.includes(id) ||
      peekViews.includes(id) ||
      ["BC01", "BC08", "MC06", "MM07", "MT06", "MX08", "BC06", "BC07"].includes(
        id,
      ))
  )
    return null;
  const pay = ["BC04", "BC05", "BC09"].includes(id);
  const second = ["MT02", "MT04", "MX04", "MX06"].includes(id);
  const edit = ["MT03", "MT05", "MX05", "MX07"].includes(id);
  const chooseSpell = spellStages.includes(id),
    choosePermanent = permanentStages.includes(id);
  const instruction = xPick
    ? xStage === "choose"
      ? "Choose X"
      : "Choose a target"
    : basic
      ? "Choose a target"
      : single
        ? id === "MC04"
          ? "Choose a creature"
          : "Choose an artifact"
        : multiple || choosePermanent
          ? "Choose a permanent"
          : chooseSpell
            ? "Choose a spell"
            : "Choose a creature";
  const large = mixed || multiple || sequential || xPick || single;
  const w = mixed
    ? 270
    : xPick
      ? 259
      : multiple
        ? 324
        : sequential
          ? 230
          : single
            ? 220
            : 178;
  const origin = {
    x: mixed ? 1000 : large ? 1230 : 1270,
    y: mixed
      ? 228
      : xPick
        ? 301
        : multiple
          ? 172
          : single
            ? 225
            : sequential
              ? 264
              : 303,
  };
  let highlights: PrintedMode[] = [];
  if (single)
    highlights = abradeModes.slice(
      id === "MC04" ? 0 : 1,
      id === "MC04" ? 1 : 2,
    );
  if (multiple)
    highlights = crypticModes.filter((m) => selection.includes(m.id));
  if (sequential) highlights = [bondModes[second ? 1 : 0]!];
  if (mixed) highlights = [crypticModes[chooseSpell ? 0 : 1]!];
  const previous = pay
    ? (draftTarget ?? (id === "BC09" ? { x: 255, y: 681 } : { x: 274, y: 49 }))
    : sequential && (second || edit)
      ? (draftTarget ??
        (["MT04", "MT05"].includes(id)
          ? { x: 254, y: 677 }
          : { x: 775, y: 485 }))
      : undefined;
  const source = cardImage(sourceFor(family));
  return (
    <>
      {previous && (
        <TargetConnection
          from={{ x: origin.x, y: origin.y + w * 0.72 }}
          to={previous}
          bend={100}
        />
      )}
      {mixed && (choosePermanent || edit) && (
        <TargetConnection
          from={{ x: origin.x + w - 8, y: origin.y + w * 0.7 }}
          to={{
            x: 1395,
            y: (choosePermanent && id === "MX06") || id === "MX07" ? 242 : 299,
          }}
          bend={-22}
        />
      )}
      <div
        className="match-source"
        style={{ left: origin.x, top: origin.y, width: w }}
      >
        <OriginalCardFace
          image={source}
          highlights={highlights.map((m) => ({
            id: m.id,
            region: m.region,
            tone: multiple ? "selected" : "available",
          }))}
        />
      </div>
      {pay ? (
        <div className="match-instruction">
          <ManaCost
            symbols={[{ src: redPayment, name: "one red mana" }]}
            label="Pay"
            size="payment"
          />
        </div>
      ) : (
        <div
          className={
            "match-instruction" +
            (xPick ? " x-stage" : "") +
            (mixed ? " mixed" : "")
          }
        >
          <TargetPrompt
            instruction={instruction}
            {...(sequential || mixed
              ? { step: { current: second ? 2 : 1, total: 2 } }
              : {})}
          />
        </div>
      )}
      {xPick && xStage === "choose" && (
        <div className="match-x-picker">
          <XValuePicker
            value={x}
            offeredValues={[0, 1, 2, 3, 4]}
            onChange={setX}
            cost={[
              ...(x ? [{ name: x + " generic mana", src: generic[x]! }] : []),
              { name: "one red mana", src: red },
            ]}
          />
        </div>
      )}
      <div
        className={
          "match-cast-actions" +
          (mixed ? " mixed" : "") +
          (pay ? " payment" : "")
        }
      >
        {pay ? (
          <MatchActions
            layout="payment"
            primary={{
              label: "Auto Pay",
              pending: id === "BC05",
              onActivate: () => go("BC05"),
            }}
            secondary={{ label: "Cancel", onActivate: () => go("BC01") }}
          />
        ) : (
          <>
            {second && (
              <ActionButton
                onClick={() =>
                  go(
                    (
                      {
                        MT02: "MT03",
                        MT04: "MT05",
                        MX04: "MX05",
                        MX06: "MX07",
                      } as Record<string, string>
                    )[id]!,
                  )
                }
              >
                Back
              </ActionButton>
            )}
            <ActionButton
              onClick={() =>
                xPick
                  ? xStage === "target"
                    ? setXStage("choose")
                    : setX(3)
                  : go(family + (basic ? "01" : "00"))
              }
            >
              Cancel
            </ActionButton>
            {xPick && xStage === "choose" && (
              <ActionButton onClick={() => setXStage("target")}>
                Next
              </ActionButton>
            )}
          </>
        )}
      </div>
    </>
  );
}
export function MatchStack({
  id,
  family,
  go,
}: {
  id: string;
  family: string;
  go: (s: string) => void;
}) {
  const mixed = family === "MX",
    stack = family === "AS",
    long = id === "AS03" || id === "AS04";
  const response = id === "AS02" ? "counter" : id === "AS04" ? "bolt-1" : null;
  const [inspect, select] = useState<string | null>(response);
  if (
    !stack &&
    !mixed &&
    !["X-stack", "BC06", "BC07", "MC06", "MM07", "MT06", "DC11"].includes(id)
  )
    return null;
  if (id === "AS06" || id === "MX01") return null;
  const where = mixed
    ? { x: 1360, y: long ? 350 : 281 }
    : family === "AS"
      ? { x: 1292, y: long ? 380 : 330 }
      : { x: 1280, y: 175 };
  const build = (face: Card, key: string, controller = "You"): StackEntry => ({
    kind: "spell",
    id: key,
    controller,
    portrait: (
      <img
        src={portrait(controller === "You" ? 0 : controller === "Alex" ? 1 : 2)}
        alt=""
      />
    ),
    image: cardImage(face),
  });
  let items: StackEntry[];
  if (stack) {
    const bolts = Array.from({ length: long ? 5 : 1 }, (_, i) => ({
      ...build("bolt", "bolt-" + (i + 1), "Alex"),
      target: {
        kind: "board" as const,
        label: "You",
        anchor: { x: 277 - where.x, y: 45 - where.y },
      },
    }));
    items =
      id === "AS08"
        ? bolts
        : [
            ...bolts,
            id === "AS07"
              ? {
                  kind: "ability" as const,
                  id: "warden",
                  controller: "Morgan",
                  portrait: <img src={portrait(2)} alt="" />,
                  name: "Soul Warden",
                  art: <img src={warden} alt="" />,
                  typeLabel: "Triggered ability",
                  effect: "Gain 1 life.",
                }
              : {
                  ...build("counter", "counter", "Morgan"),
                  target: { kind: "item", id: bolts[bolts.length - 1]!.id },
                },
          ];
  } else if (mixed) {
    items = [
      build("growth", "growth", "Alex"),
      build("bolt", "bolt", "Morgan"),
    ];
    if (id === "MX08")
      items.push({
        ...build("cryptic", "cryptic"),
        target: { kind: "item", id: "bolt" },
      });
  } else
    items = [build(id === "BC07" ? "krenko" : sourceFor(family), "pending")];
  function onSelect(key: string | null) {
    select(key);
    if (stack) {
      if (id === "AS01" && key === "counter") go("AS02");
      else if (id === "AS02" && !key) go("AS01");
      else if (id === "AS03" && key?.startsWith("bolt-")) go("AS04");
      else if (id === "AS04" && !key) go("AS03");
    }
    if (mixed && spellStages.includes(id) && key)
      go(key === "growth" ? "MX06" : "MX04");
  }
  return (
    <div
      className={
        "match-stack-layer" +
        (mixed && spellStages.includes(id) ? " stack-offered" : "")
      }
      style={{ left: where.x, top: where.y }}
    >
      <CompactSpellStack
        items={items}
        selectedId={inspect}
        onSelect={onSelect}
      />
    </div>
  );
}
