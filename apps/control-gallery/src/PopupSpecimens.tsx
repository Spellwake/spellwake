import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  Copy,
  Ellipsis,
  EllipsisVertical,
  LogOut,
  Palette,
  Settings,
  Swords,
  Trash2,
  Upload,
} from "lucide-react";
import { ActionMenu, Tooltip, type MenuCommand } from "@spellwake/ui";
import { originalUrl, references, type View } from "./references";
import {
  menus,
  hints,
  type MenuSample,
  type HintSample,
} from "./popupExamples";
const deckCommands: MenuCommand[] = [
  { id: "duplicate", label: "Duplicate", leading: <Copy /> },
  { id: "export", label: "Export", leading: <Upload /> },
  {
    id: "delete",
    label: "Delete",
    dividerBefore: true,
    danger: true,
    leading: <Trash2 />,
  },
];
/** Only the avatar art comes from the approved composition. All menu text and controls are live. */
function AccountPortrait() {
  return (
    <svg aria-hidden viewBox="457 114 50 50" className="account-art">
      <image href={originalUrl("DS-03:menu")} width="1536" height="1024" />
    </svg>
  );
}
const accountCommands: MenuCommand[] = [
  {
    id: "profile",
    label: "Planeswalker",
    detail: "View profile",
    leading: <AccountPortrait />,
  },
  {
    id: "cosmetics",
    label: "Cosmetics",
    leading: <Palette />,
    dividerBefore: true,
  },
  {
    id: "settings",
    label: "Default settings",
    leading: <Settings />,
    current: true,
  },
  { id: "logout", label: "Sign out", leading: <LogOut />, dividerBefore: true },
];
function conversationCommands(
  friend: boolean,
  blocked: boolean,
): MenuCommand[] {
  return [
    { id: "friend", label: friend ? "Remove friend" : "Add friend" },
    {
      id: "block",
      label: blocked ? "Unblock" : "Block",
      dividerBefore: !friend,
      danger: friend && !blocked,
    },
  ];
}
function position([left, top, width, height]: readonly number[]) {
  return { position: "absolute" as const, left, top, width, height };
}
function MenuExample({
  sample,
  onCommand,
}: {
  sample: MenuSample;
  onCommand: (text: string) => void;
}) {
  const { kind, skin } = sample;
  const commands =
    kind === "deck"
      ? deckCommands
      : kind === "account"
        ? accountCommands
        : conversationCommands(skin !== "stranger", skin === "blocked");
  const name =
    kind === "deck"
      ? "Deck actions"
      : kind === "account"
        ? "Account actions"
        : "Conversation actions";
  return (
    <ActionMenu
      label={name}
      commands={commands}
      onCommand={(id) => {
        const selected = commands.find((command) => command.id === id);
        if (selected) onCommand(selected.label);
      }}
      defaultOpen
      trigger={
        <button
          type="button"
          aria-label={name}
          className={"overlay-trigger " + kind + " " + skin}
          style={position(sample.trigger)}
        >
          {kind === "account" ? (
            <>
              <AccountPortrait />
              <ChevronDown />
            </>
          ) : (
            <EllipsisVertical />
          )}
        </button>
      }
    />
  );
}
function HintExample({ sample }: { sample: HintSample }) {
  const ref = useRef<HTMLDivElement>(null);
  // Seed the pictured state on entry to the gallery. The shared tooltip opens through the user's interest.
  useEffect(() => {
    const tip = ref.current?.querySelector<HTMLElement>("[role=tooltip]");
    if (!tip?.showPopover) return;
    tip.showPopover();
    const release = () => tip.hidePopover();
    tip.addEventListener("loseinterest", release, { once: true });
    return () => tip.removeEventListener("loseinterest", release);
  }, []);
  const { kind } = sample;
  const passive = kind === "plus" || kind === "commander";
  const label = {
    warning: "Card legality",
    tile: "Card legality",
    plus: "Add card",
    commander: "Set as Commander",
    phase: "Combat phase",
    overview: "Table overview",
  }[kind];
  return (
    <div ref={ref}>
      <Tooltip
        text={sample.text}
        placement={sample.placement}
        aria-label={label}
        aria-disabled={passive || undefined}
        className={"overlay-trigger " + kind}
        style={position(sample.trigger)}
      >
        {kind === "phase" ? (
          <Swords strokeWidth={1.25} />
        ) : kind === "overview" ? (
          <Ellipsis />
        ) : kind === "plus" ? (
          "+"
        ) : kind === "commander" ? (
          label
        ) : (
          "!"
        )}
      </Tooltip>
    </div>
  );
}
export default function PopupSpecimen({ view }: { view: View }) {
  const [command, setCommand] = useState("");
  const [, , width, height] = references[view].crop;
  const menu = Object.entries(menus).find(([key]) => key === view)?.[1];
  const hint = Object.entries(hints).find(([key]) => key === view)?.[1];
  return (
    <>
      <div className="popup-stage">
        <div
          data-specimen
          className={"popup-scene " + (menu?.kind ?? hint?.kind)}
          style={{ width, height }}
        >
          {menu && <MenuExample sample={menu} onCommand={setCommand} />}
          {hint && <HintExample sample={hint} />}
        </div>
      </div>
      <div role="status" className="sample-result">
        {command ? "Selected · " + command : ""}
      </div>
    </>
  );
}
