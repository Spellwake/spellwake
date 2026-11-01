/** A continuous orbit between seat views, independent of board or hidden-card state. */
import * as THREE from "three";
export type TableCameraPose = {
  center: THREE.Vector3;
  yaw: number;
  elevation: number;
  distance: number;
};
const TRAVEL_SECONDS = 0.85;
const copyPose = (pose: TableCameraPose): TableCameraPose => ({
  ...pose,
  center: pose.center.clone(),
});
const samePose = (a: TableCameraPose, b: TableCameraPose): boolean =>
  a.center.distanceToSquared(b.center) < 1e-8 &&
  Math.abs(a.yaw - b.yaw) +
    Math.abs(a.elevation - b.elevation) +
    Math.abs(a.distance - b.distance) <
    1e-6;

export function createTableCameraMotion() {
  let current: TableCameraPose | null = null;
  let origin: TableCameraPose | null = null;
  let destination: TableCameraPose | null = null;
  let progress = 1;
  return {
    get moving(): boolean {
      return progress < 1;
    },
    reset(): void {
      current = origin = destination = null;
      progress = 1;
    },
    sample(
      target: TableCameraPose,
      seconds: number,
      reducedMotion: boolean,
    ): TableCameraPose {
      if (!current || reducedMotion) {
        current = copyPose(target);
        destination = copyPose(target);
        progress = 1;
        return current;
      }
      if (!destination || !samePose(destination, target)) {
        origin = copyPose(current);
        destination = copyPose(target);
        progress = 0;
      }
      if (!origin || !destination || progress >= 1) return current;
      progress = Math.min(1, progress + Math.max(0, seconds) / TRAVEL_SECONDS);
      const eased = progress * progress * (3 - 2 * progress);
      const turn = Math.atan2(
        Math.sin(destination.yaw - origin.yaw),
        Math.cos(destination.yaw - origin.yaw),
      );
      current = {
        center: origin.center.clone().lerp(destination.center, eased),
        yaw: origin.yaw + turn * eased,
        elevation: THREE.MathUtils.lerp(
          origin.elevation,
          destination.elevation,
          eased,
        ),
        distance:
          THREE.MathUtils.lerp(origin.distance, destination.distance, eased) *
          (1 + Math.sin(Math.PI * eased) * 0.1),
      };
      if (progress === 1) current = copyPose(destination);
      return current;
    },
  };
}
