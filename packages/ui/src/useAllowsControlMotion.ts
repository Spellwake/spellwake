import { useSyncExternalStore } from "react";
import { standardMotion } from "./control-motion";
function subscribeToMotionPreference(update: () => void) {
  const media = window.matchMedia(standardMotion);
  media.addEventListener("change", update);
  return () => media.removeEventListener("change", update);
}
const allowsMotion = () => window.matchMedia(standardMotion).matches;

export function useAllowsControlMotion() {
  return useSyncExternalStore(
    subscribeToMotionPreference,
    allowsMotion,
    () => false,
  );
}
