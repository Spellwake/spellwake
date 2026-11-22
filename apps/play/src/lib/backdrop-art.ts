/** Artwork shown behind every screen. Any image works: nothing in the frame assumes its subject. */
export type BackdropArt = {
  url: string;
  /**
   * The part of the artwork that must stay in view on every screen shape,
   * as fractions from the top left. Wide windows and tall phones crop around it.
   */
  focus: { x: number; y: number };
};

/** No artwork has been chosen yet, so the frame shows its plain ground. */
export const backdropArt: BackdropArt | null = null;
