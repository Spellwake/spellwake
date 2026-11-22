import type { ReactElement } from "react";
import { EmptyState } from "@spellwake/ui";

/** Honest stand-in for an approved screen that has not been assembled yet. */
export function PendingPage({ title }: { title: string }): ReactElement {
  return (
    <section
      aria-label={title}
      style={{ flex: 1, display: "grid", placeItems: "center" }}
    >
      <EmptyState title={title} description="This screen is not built yet." />
    </section>
  );
}
