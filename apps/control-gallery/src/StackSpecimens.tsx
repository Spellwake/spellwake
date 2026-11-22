import { useLayoutEffect, useRef, useState } from "react";
import { CompactSpellStack, type StackEntry } from "@spellwake/ui";
import boltSrc from "../../../.impeccable/review/open-play/in-game/arena-reference/lightning-bolt-existing-card.jpg?inline";
import wardenArt from "../../../.impeccable/review/open-play/in-game/arena-reference/soul-warden-art.jpg?inline";
import primordialArt from "../../../.impeccable/review/open-play/in-game/arena-reference/diluvian-primordial-art.jpg?inline";
import counterSrc from "../../../.impeccable/review/open-play/approved-screens/implementation/full-match-assets/counter.webp?inline";
import { originalUrl, references, type View } from "./references";

const empty = "AS06:stack",
  pair = "AS01:stack",
  ability = "AS07:stack";
const bolt = { src: boltSrc, name: "Lightning Bolt", width: 488, height: 680 },
  counter = { src: counterSrc, name: "Counterspell", width: 488, height: 680 };
function StudyArt({ view, region }: { view: View; region: string }) {
  return (
    <svg
      aria-hidden
      viewBox={region}
      preserveAspectRatio="xMidYMid slice"
      width="100%"
      height="100%"
    >
      <image href={originalUrl(view)} width={1584} height={993} />
    </svg>
  );
}
const portraits = {
  Alex: <StudyArt view="AS04:stack" region="1436 560 28 28" />,
  Morgan: <StudyArt view="AS02:stack" region="1438 513 26 26" />,
  Sam: <StudyArt view={pair} region="815 18 22 22" />,
};
export default function StackSpecimen({ view }: { view: View }) {
  const long = view === "AS03:stack" || view === "AS04:stack";
  const startId =
    view === "AS02:stack" ? "counter" : view === "AS04:stack" ? "bolt-1" : null;
  const [selected, setSelected] = useState<string | null>(startId);
  const stage = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const pane = stage.current;
    if (!pane) return;
    pane.scrollLeft = long ? pane.scrollWidth : 0;
    const ref = pane
      .closest(".stack-comparison")
      ?.querySelector(".reference-inner");
    if (ref) ref.scrollLeft = pane.scrollLeft;
  }, [long]);
  const [left, top, width, height] = references[view].crop;
  const front = long
    ? { x: 1290, y: 374 }
    : view === "AS08:stack"
      ? { x: 1278, y: 324 }
      : { x: 1294, y: 332 };
  const you = { x: 288 - front.x, y: 50 - front.y };
  const bolts: StackEntry[] = Array.from({ length: long ? 5 : 1 }, (_, i) => ({
    kind: "spell",
    id: "bolt-" + (i + 1),
    image: bolt,
    controller: "Alex",
    portrait: portraits.Alex,
    target: { kind: "board", label: "You", anchor: you },
  }));
  const response: StackEntry = {
    kind: "spell",
    id: "counter",
    image: counter,
    controller: "Morgan",
    portrait: portraits.Morgan,
    target: { kind: "item", id: bolts[bolts.length - 1]!.id },
  };
  const trigger: StackEntry = {
    kind: "ability",
    id: "warden",
    name: "Soul Warden",
    art: <img src={wardenArt} alt="" draggable={false} />,
    effect: "Gain 1 life.",
    typeLabel: "Triggered ability",
    controller: "Sam",
    portrait: portraits.Sam,
  };
  const longTrigger: StackEntry = {
    ...trigger,
    id: "primordial",
    name: "Diluvian Primordial",
    art: <img src={primordialArt} alt="" draggable={false} />,
    effect:
      "When this creature enters, for each opponent, you may cast up to one target instant or sorcery card from that player’s graveyard without paying its mana cost. If a spell cast this way would be put into a graveyard, exile it instead.",
  };
  const items: StackEntry[] =
    view === empty
      ? []
      : view === "AS08:stack"
        ? bolts
        : [
            ...bolts,
            view === ability
              ? trigger
              : view === "AS07:long-trigger"
                ? longTrigger
                : response,
          ];
  const playerTarget =
    items.find((item) => item.id === selected)?.target?.kind === "board";
  return (
    <div ref={stage} className="popup-stage">
      <div
        className="popup-scene stack-scene"
        data-specimen
        style={{ width, height }}
      >
        <div
          className="stack-context"
          aria-hidden
          style={{
            width: 1584,
            height: 993,
            left: -left,
            top: -top,
            backgroundImage: `url("${originalUrl(empty)}")`,
          }}
        >
          {long && (
            <div
              className="stack-hud-context"
              style={{ backgroundImage: `url("${originalUrl(pair)}")` }}
            />
          )}
          {playerTarget && <span className="stack-target-seat" />}
        </div>
        <div
          className="stack-front"
          style={{ left: front.x - left, top: front.y - top }}
        >
          <CompactSpellStack
            items={items}
            selectedId={selected}
            onSelect={setSelected}
          />
        </div>
      </div>
    </div>
  );
}
