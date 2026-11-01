/** Validated presentation fixture, separate from the future rules-engine match projection. */
import { z } from "zod";
import {
  createSceneSettings,
  SCENE_SETTING_IDS,
  type SceneSettingId,
  type SceneSettings,
} from "./scene-settings";
import { pileEntrySchema } from "./pile-browser";

const publicCardSchema = pileEntrySchema
  .extend({
    canPlay: z.boolean().optional(),
    sourceId: z.string().min(1).optional(),
  })
  .refine(
    (card) => card.faceUp || (card.key === null && card.sourceId === undefined),
    {
      message: "Hidden cards cannot carry a face identity.",
    },
  );
export const playSetupSchema = z
  .object({
    mode: z.enum(["ffa", "one-two", "two-one", "two-two"]),
    viewerSeat: z.number().int().min(1).max(6),
    players: z
      .array(
        z.object({
          name: z.string().min(1),
          handCount: z.number().int().min(0).max(200),
          hand: z.array(publicCardSchema).optional(),
          command: z.array(publicCardSchema).max(8).optional(),
          availableCards: z
            .array(
              publicCardSchema.safeExtend({
                sourceZone: z.enum([
                  "command",
                  "exile",
                  "graveyard",
                  "library",
                  "outside-game",
                ]),
              }),
            )
            .max(200)
            .optional(),
        }),
      )
      .min(2)
      .max(6),
    libraryCount: z.number().int().min(0).max(200),
    cardBack: z.enum(["classic", "spellwake", "unlettered"]),
    sleeve: z.enum(["none", "midnight", "penny"]),
  })
  .superRefine((setup, context) => {
    for (const [index, player] of setup.players.entries())
      if (player.hand && player.hand.length !== player.handCount)
        context.addIssue({
          code: "custom",
          path: ["players", index, "hand"],
          message: "Visible hand entries must match the public count.",
        });
    if (setup.viewerSeat > setup.players.length)
      context.addIssue({
        code: "custom",
        path: ["viewerSeat"],
        message: "Viewer must occupy a seat in the game.",
      });
    const required =
      setup.mode === "two-two" ? 4 : setup.mode === "ffa" ? null : 3;
    if (required !== null && setup.players.length !== required)
      context.addIssue({
        code: "custom",
        path: ["players"],
        message: "Player count must match the selected mode.",
      });
  });
export type PlaySetup = z.infer<typeof playSetupSchema>;

export const SAMPLE_PLAY_SETUP = playSetupSchema.parse({
  mode: "ffa",
  viewerSeat: 1,
  players: ["You", "Alex", "Morgan", "Sam", "Riley", "Jordan"].map(
    (name, seat) => ({
      name,
      handCount: 7,
      hand: Array.from({ length: 7 }, (_, index) => ({
        key:
          seat === 0
            ? index % 3 === 1
              ? "land"
              : "lightning-bolt"
            : index === 2
              ? "serra-angel"
              : null,
        faceUp: seat === 0 || index === 2,
      })),
      command: [{ key: "krenko", faceUp: true }],
      availableCards: [
        {
          key: "krenko",
          faceUp: true,
          sourceZone: "command",
          canPlay: seat === 0,
        },
        {
          key: "lightning-bolt",
          faceUp: true,
          sourceZone: "exile",
          canPlay: seat === 0,
        },
        {
          key: "serra-angel",
          faceUp: true,
          sourceZone: "graveyard",
          canPlay: seat === 0,
        },
      ],
    }),
  ),
  libraryCount: 40,
  cardBack: "spellwake",
  sleeve: "midnight",
});

/** Map game presentation data into the shared renderer's page-independent settings. */
export function settingsForPlay(input: PlaySetup): SceneSettings {
  const setup = playSetupSchema.parse(input);
  const settings = createSceneSettings();
  const values: Partial<Record<SceneSettingId, string>> = {
    view: "layout",
    tilt: "8",
    turn: "0",
    light: "-40",
    "table-zoom": "100",
    "card-back": setup.cardBack,
    "sleeve-design": setup.sleeve === "none" ? "midnight" : setup.sleeve,
    "deck-mode": "optimized",
    "deck-count": String(setup.libraryCount),
    "player-count": String(setup.players.length),
    "viewer-seat": String(setup.viewerSeat),
    "player-focus": String(setup.viewerSeat),
    "navigation-mode": "navigate",
    "table-mode": setup.mode,
    "hand-size": String(setup.players[setup.viewerSeat - 1]?.handCount ?? 0),
    "opponent-hand-size": "7",
  };
  for (const id of SCENE_SETTING_IDS) settings[id].value = values[id] ?? "";
  settings["show-hands"].checked = true;
  settings["auto-tap"].checked = true;
  settings.sleeve.checked = setup.sleeve !== "none";
  return settings;
}
