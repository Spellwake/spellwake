import { lazy, Suspense } from "react";
import { createRoot } from "react-dom/client";

const GalleryApp = lazy(() => import("./GalleryApp"));
const FullReview = lazy(() =>
  import("./full-review/Review").then((m) => ({ default: m.FullReview })),
);
const ReviewScene = lazy(() =>
  import("./full-review/Review").then((m) => ({ default: m.ReviewScene })),
);
const CombatDamageWalkthrough = lazy(() => import("./CombatDamageWalkthrough"));

const query = new URLSearchParams(location.search);
const combatWalkthrough =
  query.get("walkthrough") === "combat-damage" ||
  /^(?:DA01|DA0[13456]:allocation)$/.test(query.get("view") ?? "");

createRoot(document.getElementById("root")!).render(
  <Suspense fallback={null}>
    {query.has("review") ? (
      <FullReview />
    ) : query.has("scene") ? (
      <ReviewScene />
    ) : combatWalkthrough ? (
      <CombatDamageWalkthrough />
    ) : (
      <GalleryApp />
    )}
  </Suspense>,
);
