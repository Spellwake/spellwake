import { describe, expect, test } from "bun:test";
import { bezier, openingScript, span } from "./pack-timeline";

describe("bezier", () => {
  test("matches the ends and a linear curve exactly", () => {
    const linear = bezier([0, 0, 1, 1]);
    for (const t of [0, 0.25, 0.5, 0.75, 1])
      expect(linear(t)).toBeCloseTo(t, 5);
  });
  test("decelerates for the app's arrive curve and never overshoots", () => {
    const arrive = bezier([0.22, 0.8, 0.24, 1]);
    let previous = 0;
    for (let step = 1; step <= 100; step += 1) {
      const value = arrive(step / 100);
      expect(value).toBeGreaterThanOrEqual(previous);
      expect(value).toBeLessThanOrEqual(1);
      previous = value;
    }
    expect(arrive(0.5)).toBeGreaterThan(0.75);
  });
});

describe("span", () => {
  test("clamps to the window", () => {
    expect(span(-1, 0, 2)).toBe(0);
    expect(span(1, 0, 2)).toBe(0.5);
    expect(span(3, 0, 2)).toBe(1);
    expect(span(1, 1, 0)).toBe(1);
  });
});

describe("openingScript", () => {
  const beats = { scene: 0.42, hero: 0.84, stagger: 0.06 };
  test("tears, then lifts the cards, then deals them, in that order", () => {
    const script = openingScript(beats, 14);
    const tearEnd = script.tear.start + script.tear.length;
    expect(script.tear.start).toBeGreaterThan(0);
    expect(script.rise.start).toBeGreaterThan(tearEnd);
    expect(script.exit.start).toBeGreaterThan(script.rise.start);
    expect(script.deal.start).toBeGreaterThan(script.rise.start);
    expect(script.end).toBeCloseTo(
      script.deal.start + 13 * beats.stagger + script.deal.each,
      6,
    );
  });
  test("scales with the motion language's beats", () => {
    const slow = openingScript({ ...beats, hero: 1.68, scene: 0.84 }, 14);
    expect(slow.end).toBeGreaterThan(openingScript(beats, 14).end);
  });
  test("handles a pack with no cards", () => {
    const script = openingScript(beats, 0);
    expect(script.end).toBeCloseTo(script.deal.start + script.deal.each, 6);
  });
});
