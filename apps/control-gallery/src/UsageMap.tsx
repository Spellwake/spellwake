import { references, type View } from "./references";
export interface ControlUse {
  name: string;
  id: string;
  description: string;
  approvedCount?: number;
  mapSection?: "component" | "shared";
  screens: readonly View[];
}
export function UsageMap({ uses }: { uses: readonly ControlUse[] }) {
  return (
    <section className="usage" aria-labelledby="usage-heading">
      <h2 id="usage-heading">Where these controls appear</h2>
      {uses.map((use) => (
        <div className="usage-row" key={use.id}>
          <div>
            <a
              className="unit-link"
              href={
                "../breakdown.html#" +
                (use.mapSection ?? "shared") +
                "/" +
                use.id
              }
            >
              {use.name}
            </a>
            <p>{use.description}</p>
            {use.approvedCount && (
              <a
                className="coverage-link"
                href={
                  "../breakdown.html#" +
                  (use.mapSection ?? "shared") +
                  "/" +
                  use.id
                }
              >
                See all {use.approvedCount} approved views
              </a>
            )}
          </div>
          <div className="screen-links">
            {use.screens.map((id) => (
              <a href={"?view=" + id} key={id}>
                {references[id].state}
                <small>{id.split(":")[0]}</small>
              </a>
            ))}
          </div>
        </div>
      ))}
    </section>
  );
}
