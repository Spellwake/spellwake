import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { atom, useAtom } from "jotai";
import type { UpdatesSource } from "./updates-source";
import { targetOf, type Update } from "./updates";

const PREVIEW: readonly Update[] = [
  {
    id: "preview-update-1",
    kind: "trade-offer",
    text: "Trade offer received",
    sourceRef: "preview-trade-1",
    isNew: true,
  },
  {
    id: "preview-update-2",
    kind: "sold",
    text: "Marketplace listing sold",
    sourceRef: "preview-listing-1",
    isNew: true,
  },
  {
    id: "preview-update-3",
    kind: "trade-accepted",
    text: "Trade accepted",
    sourceRef: "preview-trade-2",
    isNew: false,
  },
];

/** Which preview updates have been seen, shared by the header and Messages. */
const seenAtom = atom<readonly string[]>([]);

/** The preview's updates: two unread and one read. `none` has none. */
export function useDemoUpdates(state: "some" | "none" = "some"): UpdatesSource {
  const navigate = useNavigate();
  const [seen, setSeen] = useAtom(seenAtom);
  // Marked seen this visit, but still shown as new until Messages closes.
  const [shownNew] = useState(
    () => new Set(PREVIEW.filter((u) => u.isNew).map((u) => u.id)),
  );
  return useMemo(() => {
    const items = state === "none" ? [] : PREVIEW;
    return {
      status: "ready",
      items: items.map((update) => ({
        ...update,
        isNew:
          update.isNew &&
          (shownNew.has(update.id) || !seen.includes(update.id)),
      })),
      unread: items.filter(
        (update) => update.isNew && !seen.includes(update.id),
      ).length,
      markSeen: () =>
        setSeen((current) =>
          current.length === items.filter((u) => u.isNew).length
            ? current
            : items.filter((u) => u.isNew).map((u) => u.id),
        ),
      open: (update) => {
        const target = targetOf(update);
        if (target?.to === "listings")
          void navigate("/market/listings?mine=true");
        // The preview's trade is with Mira, its first friend.
        else if (target?.to === "trade")
          void navigate("/collection/trade/mira");
      },
      notice: null,
    };
  }, [state, seen, setSeen, shownNew, navigate]);
}
