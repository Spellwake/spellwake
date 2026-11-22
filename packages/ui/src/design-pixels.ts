/**
 * A length in the pixel scale of the approved references, expressed in rem.
 * Use it for sizes passed through props or attributes, which no stylesheet
 * transform can reach, so they follow the host's display density.
 */
export function designPixels(pixels: number): string {
  return `${pixels / 16}rem`;
}
