import type { TooltipProps } from "@spellwake/ui";
import type { ControlUse } from "./UsageMap";

export const popupUses: ControlUse[] = [
  {
    id: "action-menu",
    name: "Action menu",
    approvedCount: 6,
    description:
      "Deck actions, relationship-specific conversation commands, and the account menu.",
    screens: [
      "DB-01:menu",
      "DB-09:menu",
      "MSG-D04:menu",
      "MSG-D06:menu",
      "MSG-D10:menu",
      "DS-03:menu",
    ],
  },
  {
    id: "tooltip",
    name: "Tooltip",
    approvedCount: 5,
    description:
      "Legality and addition guards in the deck builder, phase names and table overview in a match.",
    screens: [
      "DB-05:tip",
      "DB-11:tip",
      "DB-10:add",
      "DB-10:commander",
      "NP-04:tip",
      "DA08:tip",
    ],
  },
];
export interface MenuSample {
  kind: "deck" | "social" | "account";
  skin: string;
  trigger: [number, number, number, number];
}
export const menus = {
  "DB-01:menu": {
    kind: "deck",
    skin: "workbench",
    trigger: [191, 6, 44, 45],
  },
  "DB-09:menu": {
    kind: "deck",
    skin: "management",
    trigger: [199, 15, 68, 60],
  },
  "MSG-D04:menu": {
    kind: "social",
    skin: "stranger",
    trigger: [147, 2, 31, 32],
  },
  "MSG-D06:menu": {
    kind: "social",
    skin: "friend",
    trigger: [146, 2, 31, 32],
  },
  "MSG-D10:menu": {
    kind: "social",
    skin: "blocked",
    trigger: [142, 2, 31, 35],
  },
  "DS-03:menu": {
    kind: "account",
    skin: "account",
    trigger: [186, 4, 91, 47],
  },
} satisfies Record<string, MenuSample>;
type HintKind =
  "warning" | "tile" | "plus" | "commander" | "phase" | "overview";
export interface HintSample {
  kind: HintKind;
  text: string;
  trigger: [number, number, number, number];
  placement: NonNullable<TooltipProps["placement"]>;
}
export const hints = {
  "DB-05:tip": {
    kind: "warning",
    text: "Not legal in Standard.",
    trigger: [191, 60, 21, 21],
    placement: "top-end",
  },
  "DB-11:tip": {
    kind: "tile",
    text: "Not legal in Standard.",
    trigger: [8, 14, 27, 27],
    placement: "right",
  },
  "DB-10:add": {
    kind: "plus",
    text: "Can’t verify format eligibility.",
    trigger: [232, 12, 54, 39],
    placement: "bottom-end",
  },
  "DB-10:commander": {
    kind: "commander",
    text: "Remove the current commander first.",
    trigger: [138, 12, 202, 46],
    placement: "bottom-end",
  },
  "NP-04:tip": {
    kind: "phase",
    text: "Combat",
    trigger: [32, 93, 42, 42],
    placement: "top",
  },
  "DA08:tip": {
    kind: "overview",
    text: "Overview",
    trigger: [41, 10, 46, 46],
    placement: "bottom",
  },
} satisfies Record<string, HintSample>;
