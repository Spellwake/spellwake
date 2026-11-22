import sections from "virtual:approved-screens";
export { sections };
export const screens = sections.flatMap((section) =>
  section.views.map((view) => ({ section, view })),
);
export function findScreen(id: string | null) {
  return screens.find((entry) => entry.view.id === id);
}
