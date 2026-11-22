import { references, originalUrl, type View } from "./references";

export type ArtworkCrop = readonly [number, number, number, number];
/** Samples item artwork only. The surrounding controls are rendered by the shared UI. */
export function ReferenceArtwork({
  source,
  region,
}: {
  source: View;
  region: ArtworkCrop;
}) {
  const [w, h] = references[source].size;
  return (
    <svg
      aria-hidden="true"
      viewBox={region.join(" ")}
      width={region[2]}
      height={region[3]}
    >
      <image href={originalUrl(source)} width={w} height={h} />
    </svg>
  );
}
