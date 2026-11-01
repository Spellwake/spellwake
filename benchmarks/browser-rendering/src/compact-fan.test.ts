import { expect, test } from "bun:test";
import { fanWindow, groupedHandFan } from "./compact-fan";

test("hand and source-zone groups follow the same circle with gaps between groups", () => {
  const fan = groupedHandFan(
    7,
    ["command", "exile", "exile", "graveyard"],
    7.08,
  );
  const radius = (fan.width - 3) / (2 * Math.sin(0.65 / 2));
  for (const pose of [...fan.hand, ...fan.external]) {
    expect(Math.hypot(pose.x, pose.y + radius)).toBeCloseTo(radius);
    expect(Math.atan2(pose.x, pose.y + radius)).toBeCloseTo(-pose.angle);
  }
  const [commander, exile, secondExile, graveyard] = fan.external;
  if (!commander || !exile || !secondExile || !graveyard)
    throw new Error("missing grouped offers");
  expect(exile.x - commander.x).toBeGreaterThan((secondExile.x - exile.x) * 2);
  expect(graveyard.x - secondExile.x).toBeGreaterThan(
    (secondExile.x - exile.x) * 2,
  );
  expect(commander.x).toBeGreaterThan(fan.hand.at(-1)!.x);
  expect(
    groupedHandFan(
      7,
      Array.from({ length: 200 }, () => "exile"),
      7.08,
    ).width,
  ).toBeLessThan(16);
});

test("fan windows clamp at both ends and preserve small groups", () => {
  expect(fanWindow(0, 0)).toEqual({ start: 0, count: 0 });
  expect(fanWindow(4, 2)).toEqual({ start: 0, count: 4 });
  expect(fanWindow(200, -100)).toEqual({ start: 0, count: 9 });
  expect(fanWindow(200, 100)).toEqual({ start: 96, count: 9 });
  expect(fanWindow(200, 1000)).toEqual({ start: 191, count: 9 });
  expect(fanWindow(40, 5.49)).toEqual({ start: 1, count: 9 });
  expect(fanWindow(40, 5.5)).toEqual({ start: 2, count: 9 });
  expect(fanWindow(40, 10, 5)).toEqual({ start: 8, count: 5 });
});

test("every entry can occupy the bounded source-group window", () => {
  for (const count of [1, 9, 10, 40, 200]) {
    const reached = new Set<number>();
    for (let focus = 0; focus < count; focus++) {
      const window = fanWindow(count, focus);
      expect(window.count).toBeLessThanOrEqual(9);
      expect(focus).toBeGreaterThanOrEqual(window.start);
      expect(focus).toBeLessThan(window.start + window.count);
      for (let slot = 0; slot < window.count; slot++)
        reached.add(window.start + slot);
    }
    expect([...reached]).toEqual(
      Array.from({ length: count }, (_, index) => index),
    );
  }
});
