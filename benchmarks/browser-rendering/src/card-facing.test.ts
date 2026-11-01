import { expect, test } from "bun:test";
import { createCardFacingFade } from "./card-facing";

test("half-turn happens at the faded midpoint and new face reveals gradually", () => {
  const fade = createCardFacingFade();
  const outgoing = fade.sample(Math.PI, 0.045, false);
  expect(outgoing.turn).toBe(0);
  expect(outgoing.opacity).toBeCloseTo(0.5);
  const hidden = fade.sample(Math.PI, 0.045, false);
  expect(hidden.opacity).toBe(0);
  expect(hidden.turn).toBe(Math.PI);
  const incoming = fade.sample(Math.PI, 0.065, false);
  expect(incoming.opacity).toBeCloseTo(0.5);
  expect(incoming.turn).toBe(Math.PI);
  const settled = fade.sample(Math.PI, 0.065, false);
  expect(settled.opacity).toBe(1);
  expect(settled.moving).toBe(false);
});
test("interrupted fade restores the current orientation; reduced motion snaps", () => {
  const fade = createCardFacingFade();
  const outgoing = fade.sample(Math.PI, 0.03, false);
  const canceled = fade.sample(0, 0, false);
  expect(canceled.turn).toBe(0);
  expect(canceled.opacity).toBe(outgoing.opacity);
  const reduced = fade.sample(Math.PI, 0, true);
  expect(reduced.turn).toBe(Math.PI);
  expect(reduced.opacity).toBe(1);
  expect(reduced.moving).toBe(false);
});
