import { z } from "zod";

const boxViewSchema = z.object({
  azimuth: z.coerce.number().finite().min(-180).max(180),
  elevation: z.coerce.number().finite().min(0.1).max(89.9),
});

export type BoxView = z.infer<typeof boxViewSchema>;

/** User-selected collection direction, captured from the orbit preview. */
export const DEFAULT_BOX_VIEW: BoxView = { azimuth: 21.63, elevation: 11.07 };

/** Shareable viewing direction; zoom stays responsive to each collection tile. */
export function readBoxView(search: string): BoxView | undefined {
  const query = new URLSearchParams(search);
  if (!query.has("azimuth") || !query.has("elevation")) return undefined;
  const result = boxViewSchema.safeParse({
    azimuth: query.get("azimuth"),
    elevation: query.get("elevation"),
  });
  return result.success ? result.data : undefined;
}

export function collectionViewUrl(view: BoxView): string {
  const query = new URLSearchParams({
    screen: "collection",
    azimuth: view.azimuth.toFixed(2),
    elevation: view.elevation.toFixed(2),
  });
  return `?${query.toString()}`;
}
