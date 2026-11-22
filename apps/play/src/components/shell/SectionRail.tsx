import type { ReactElement } from "react";
import { useHref, useLocation, useNavigate } from "react-router-dom";
import { SectionNavigation } from "@spellwake/ui";
import type { Destination } from "../../routes/navigation";

/** Desktop side rail for the current destination's sections. */
export function SectionRail({
  destination,
}: {
  destination: Destination;
}): ReactElement {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const base = useHref("/").replace(/\/$/, "");
  const links = destination.sections.map((section) => ({
    href: base + section.path,
    label: section.label,
    icon: section.icon,
    path: section.path,
  }));
  return (
    <SectionNavigation
      label={destination.label}
      links={links}
      currentHref={base + pathname}
      surface="clear"
      onNavigate={(event, link) => {
        const target = links.find((candidate) => candidate.href === link.href);
        if (!target) return;
        event.preventDefault();
        void navigate(target.path);
      }}
    />
  );
}
