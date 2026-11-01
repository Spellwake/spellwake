/** Reusable play view. Hosts supply presentation data and explicitly choose fixture behavior. */
import { z } from "zod";
import {
  mountCardScene,
  type CardSceneHandle,
  type CardSceneOptions,
} from "./card-scene";
import { settingsForPlay, playSetupSchema, type PlaySetup } from "./play-setup";
export type { CardSceneHandle, CardSceneOptions } from "./card-scene";
export type { PlaySetup } from "./play-setup";
export type { TableDropIntent } from "./table-interaction";

/** No side effects on import. Dispose the returned mount before replacing its DOM. */
export async function mountPlayArea(
  root: ParentNode,
  input: PlaySetup,
  options: CardSceneOptions = {},
): Promise<CardSceneHandle> {
  const setup = playSetupSchema.parse(input);
  if (
    options.playMode !== "fixture-preview" &&
    !setup.players[setup.viewerSeat - 1]?.hand
  ) {
    throw new Error(
      "An integrated table requires explicit viewer hand entries.",
    );
  }
  const element = (id: string) =>
    z.instanceof(HTMLElement).parse(root.querySelector(`#${id}`));
  return mountCardScene(
    {
      canvas: z.instanceof(HTMLCanvasElement).parse(element("play-canvas")),
      status: element("play-status"),
      opponentNavigation: z
        .instanceof(HTMLDivElement)
        .parse(element("opponent-navigation")),
      returnBoard: z
        .instanceof(HTMLButtonElement)
        .parse(element("return-board")),
      stackPanel: z.instanceof(HTMLDivElement).parse(element("stack-preview")),
      stackCount: z.instanceof(HTMLOutputElement).parse(element("stack-count")),
      readyMessage:
        options.playMode === "fixture-preview"
          ? "Table preview ready"
          : "Table ready",
      playerZones: setup.players,
      playerNames: setup.players.map((player) => player.name),
      playerHandCounts: setup.players.map((player) => player.handCount),
    },
    settingsForPlay(setup),
    options,
  );
}
