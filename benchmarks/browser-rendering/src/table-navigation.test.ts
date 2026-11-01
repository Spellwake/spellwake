import { expect, test } from "bun:test";
import { navigationDirections } from "./table-navigation";

test("every other player retains a distinct navigation control from every seat", () => {
  const seats = [
    { x: -17.5, z: 5 },
    { x: -17.5, z: -5 },
    { x: 0, z: -5 },
    { x: 17.5, z: -5 },
    { x: 17.5, z: 5 },
    { x: 0, z: 5 },
  ];
  for (let focus = 1; focus <= 6; focus++) {
    const targets = navigationDirections(
      seats,
      focus,
      focus >= 2 && focus <= 4 ? Math.PI : 0,
    );
    expect(targets.map((target) => target.seat).sort()).toEqual(
      [1, 2, 3, 4, 5, 6].filter((seat) => seat !== focus),
    );
    const positions = targets.map((target) => [
      target.horizontal * (target.vertical === 0 ? 8 : 6.2) +
        (target.vertical === 0 ? 0 : target.offset),
      (target.vertical < 0 ? -4.8 : -3.4) +
        (target.vertical === 0 ? target.offset : 0),
    ]);
    for (let i = 0; i < positions.length; i++)
      for (let j = i + 1; j < positions.length; j++) {
        const a = positions[i]!,
          b = positions[j]!;
        expect(Math.hypot(a[0]! - b[0]!, a[1]! - b[1]!)).toBeGreaterThan(0.82);
      }
  }
});
