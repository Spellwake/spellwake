import * as THREE from "three";
import { z } from "zod";

const rigSchema = z.object({
  segments: z.tuple([
    z.number().positive(),
    z.number().positive(),
    z.number().positive(),
    z.number().positive(),
    z.number().positive(),
  ]),
  start: z.tuple([z.number().finite(), z.number().finite()]),
  jointCount: z.number().int().min(3).max(512),
});
type Frame = { y: number; z: number; angle: number };
export type LeatherBend = {
  pose: (
    rearDegrees: number,
    frontDegrees: number,
  ) => { rear: Frame; front: Frame; highest: number };
};

/** Bend a flat rest surface along fixed material distances, preserving fold arc length. */
export function createLeatherBend(assembly: THREE.Object3D): LeatherBend {
  const definition = rigSchema.parse(assembly.userData.leatherRig);
  const length = definition.segments.reduce((sum, segment) => sum + segment, 0);
  const joints = Array.from({ length: definition.jointCount }, (_, index) => {
    const joint = assembly.getObjectByName(
      `Leather_${String(index).padStart(3, "0")}`,
    );
    if (!joint) throw new Error(`Missing leather frame ${index}`);
    return joint;
  });
  const frameAt = (
    distance: number,
    rearDegrees: number,
    frontDegrees: number,
  ): Frame => {
    let [y, zPosition] = definition.start;
    let angle = 0;
    const turns = [
      0,
      Math.PI / 2 - THREE.MathUtils.degToRad(rearDegrees),
      0,
      Math.PI / 2 - THREE.MathUtils.degToRad(frontDegrees),
      0,
    ];
    for (const [index, segment] of definition.segments.entries()) {
      const travel = Math.min(Math.max(distance, 0), segment);
      const curvature = (turns[index] ?? 0) / segment;
      if (Math.abs(curvature) < 1e-9) {
        y += travel * Math.cos(angle);
        zPosition += travel * Math.sin(angle);
      } else {
        y +=
          (Math.sin(angle + curvature * travel) - Math.sin(angle)) / curvature;
        zPosition +=
          (Math.cos(angle) - Math.cos(angle + curvature * travel)) / curvature;
      }
      angle += curvature * travel;
      distance -= travel;
      if (distance < 1e-9) break;
    }
    return { y, z: zPosition, angle };
  };
  return {
    pose(rearDegrees, frontDegrees) {
      let highest = 0.95;
      for (const [index, joint] of joints.entries()) {
        const frame = frameAt(
          (index / (joints.length - 1)) * length,
          rearDegrees,
          frontDegrees,
        );
        joint.position.set(0, frame.y, frame.z);
        joint.rotation.set(frame.angle, 0, 0);
        highest = Math.max(highest, frame.y + 0.06);
      }
      const [back, rearArc, crown, frontArc] = definition.segments;
      return {
        rear: frameAt(back + rearArc / 2, rearDegrees, frontDegrees),
        front: frameAt(
          back + rearArc + crown + frontArc / 2,
          rearDegrees,
          frontDegrees,
        ),
        highest,
      };
    },
  };
}
