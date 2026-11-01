import { expect, test } from "bun:test";
import {
  playSetupSchema,
  SAMPLE_PLAY_SETUP,
  settingsForPlay,
} from "./play-setup";

test("game setup rejects missing viewers and incompatible team seating", () => {
  expect(
    playSetupSchema.safeParse({
      ...SAMPLE_PLAY_SETUP,
      viewerSeat: 6,
      players: SAMPLE_PLAY_SETUP.players.slice(0, 2),
    }).success,
  ).toBe(false);
  expect(
    playSetupSchema.safeParse({ ...SAMPLE_PLAY_SETUP, mode: "two-two" })
      .success,
  ).toBe(false);
  expect(
    playSetupSchema.safeParse({
      ...SAMPLE_PLAY_SETUP,
      mode: "two-two",
      players: SAMPLE_PLAY_SETUP.players.slice(0, 4),
    }).success,
  ).toBe(true);
});
test("play sessions initialize independent settings without browser controls", () => {
  const first = settingsForPlay({
    ...SAMPLE_PLAY_SETUP,
    viewerSeat: 2,
    players: [
      { name: "A", handCount: 3 },
      { name: "B", handCount: 9 },
    ],
  });
  const second = settingsForPlay(SAMPLE_PLAY_SETUP);
  expect(first["viewer-seat"].value).toBe("2");
  expect(first["player-focus"].value).toBe("2");
  expect(first["hand-size"].value).toBe("9");
  first["player-focus"].value = "1";
  expect(second["player-focus"].value).toBe("1");
  expect(second["player-count"].value).toBe("6");
  expect(first["player-focus"]).not.toBe(second["player-focus"]);
});

test("hidden opponent entries cannot include a face identity", () => {
  expect(
    playSetupSchema.safeParse({
      ...SAMPLE_PLAY_SETUP,
      players: [
        {
          name: "A",
          handCount: 1,
          hand: [{ key: "serra-angel", faceUp: false }],
        },
        { name: "B", handCount: 0 },
      ],
    }).success,
  ).toBe(false);
});

test("other-zone offers retain their zone without leaking concealed identities", () => {
  const player = {
    name: "A",
    handCount: 0,
    availableCards: [
      { key: "serra-angel", faceUp: false, sourceZone: "exile", canPlay: true },
    ],
  };
  expect(
    playSetupSchema.safeParse({
      ...SAMPLE_PLAY_SETUP,
      players: [player, { name: "B", handCount: 0 }],
    }).success,
  ).toBe(false);
  const safe = playSetupSchema.parse({
    ...SAMPLE_PLAY_SETUP,
    players: [
      {
        ...player,
        availableCards: [
          { key: null, faceUp: false, sourceZone: "exile", canPlay: false },
        ],
      },
      { name: "B", handCount: 0 },
    ],
  });
  expect(safe.players[0]?.availableCards?.[0]?.sourceZone).toBe("exile");
  expect(safe.players[0]?.availableCards?.[0]?.key).toBeNull();
});

test("host presentation identities survive parsing only for known faces", () => {
  const setup = structuredClone(SAMPLE_PLAY_SETUP);
  setup.players[0]!.hand![0] = {
    key: "lightning-bolt",
    faceUp: true,
    sourceId: "opaque-view-object",
    canPlay: true,
  };
  expect(playSetupSchema.parse(setup).players[0]?.hand?.[0]?.sourceId).toBe(
    "opaque-view-object",
  );
  setup.players[1]!.hand![0] = {
    key: null,
    faceUp: false,
    sourceId: "tracking-secret",
  };
  expect(playSetupSchema.safeParse(setup).success).toBe(false);
});
