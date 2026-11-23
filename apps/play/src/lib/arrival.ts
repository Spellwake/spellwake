import { useEffect, useState } from "react";
import { motionLanguage as m } from "./motion";

/** What a group's arrival is made of. Every value has a default from the motion language. */
export interface ArrivalOptions {
  /** How far each item rises into place, in pixels. */
  distance?: number;
  /** The beat between one item and the next, in seconds. */
  stagger?: number;
  /** How many items take a beat of their own; the rest arrive with the last. */
  most?: number;
  /** A pause before the first item, for a screen that is itself arriving. */
  lead?: number;
}

/** What an arriving item hands to its `motion` element. */
export interface ArrivalProps {
  initial: false | { opacity: number; y: number };
  animate: { opacity: number; y: number };
  transition: {
    duration: number;
    ease: typeof m.ease.arrive;
    delay: number;
  };
}

/**
 * A group arriving once: each item fades in as it rises a short way, one beat
 * after the one before, so the eye is led down the list and the whole group
 * is at rest in well under a second. It happens when the screen first shows
 * its items and never again: items that appear later, from a search, a
 * filter or another page of results, are simply there. Under reduced motion
 * the rise is dropped by the app's motion settings and only the fade remains.
 */
export function useArrival(
  /** The group has items to show. */
  ready: boolean,
  options: ArrivalOptions = {},
): {
  /** The motion props of the item at this place in the group. */
  item: (index: number) => ArrivalProps;
  /** When that item starts, in seconds, or null once the arrival is over. */
  delayOf: (index: number) => number | null;
} {
  const {
    distance = m.travel / 2,
    stagger = m.stagger,
    most = 9,
    lead = 0,
  } = options;
  const [landed, setLanded] = useState(false);
  useEffect(() => {
    if (!ready || landed) return;
    const whole = lead + most * stagger + m.duration.scene;
    const timer = window.setTimeout(() => setLanded(true), whole * 1000);
    return () => window.clearTimeout(timer);
  }, [ready, landed, lead, most, stagger]);

  const delayOf = (index: number): number | null =>
    landed ? null : lead + Math.min(index, most) * stagger;
  return {
    delayOf,
    item: (index) => ({
      initial: landed ? false : { opacity: 0, y: distance },
      animate: { opacity: 1, y: 0 },
      transition: {
        duration: m.duration.scene,
        ease: m.ease.arrive,
        delay: delayOf(index) ?? 0,
      },
    }),
  };
}
