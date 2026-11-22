import drake from "../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-13/two-attackers/wind-drake.jpg?inline";
import phantom from "../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-13/two-attackers/phantom-monster.jpg?inline";
import elves from "../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-13/two-attackers/llanowar-elves.jpg?inline";
import visionary from "../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-13/two-attackers/elvish-visionary.jpg?inline";

function blocker(id: string, name: string, src: string, suggested: number) {
  return { id, image: { name, src, width: 488, height: 680 }, suggested };
}
export const attackers = [
  {
    id: "angel",
    name: "Serra Angel",
    power: 4,
    blockers: [
      blocker("drake", "Wind Drake", drake, 1),
      blocker("phantom", "Phantom Monster", phantom, 3),
    ],
  },
  {
    id: "bears",
    name: "Grizzly Bears",
    power: 2,
    blockers: [
      blocker("elves", "Llanowar Elves", elves, 1),
      blocker("visionary", "Elvish Visionary", visionary, 1),
    ],
  },
] as const;
