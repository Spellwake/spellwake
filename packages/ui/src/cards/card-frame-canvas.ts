/**
 * Browser Canvas rasterizer for the unified 1062x1482 300-DPI SVG card frame renderer.
 *
 * Used for:
 * - 300-DPI PNG card export (1062x1482) with lossless embedded iTXt metadata
 * - 3D table Three.js CanvasTexture generation for in-match custom cards
 * - 3D DeckBox face card canvas textures
 */

import {
  CARD_HEIGHT,
  CARD_WIDTH,
  renderCardFrameSvg,
  type CardFrameRenderInput,
} from "./card-frame-svg";

export interface CardCanvasRenderOptions {
  width?: number;
  height?: number;
}

async function inlineArtUrl(url: string | null | undefined): Promise<string | null> {
  if (!url) return null;
  if (!url.startsWith("blob:")) return url;
  const response = await fetch(url);
  if (!response.ok) return null;
  const blob = await response.blob();
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") resolve(reader.result);
      else reject(new Error("Failed to inline blob art URL"));
    };
    reader.onerror = () => reject(new Error("Failed to read blob art URL"));
    reader.readAsDataURL(blob);
  });
}

async function resolveInlineArtInput(
  input: CardFrameRenderInput,
): Promise<CardFrameRenderInput> {
  if (input.kind === "stack_trigger") {
    const artUrl = await inlineArtUrl(input.artUrl);
    return { ...input, artUrl };
  }
  const [faceArt, secondaryArt] = await Promise.all([
    inlineArtUrl(input.face.artUrl),
    input.secondaryFace ? inlineArtUrl(input.secondaryFace.artUrl) : Promise.resolve(null),
  ]);
  return {
    ...input,
    face: { ...input.face, artUrl: faceArt },
    ...(input.secondaryFace
      ? { secondaryFace: { ...input.secondaryFace, artUrl: secondaryArt } }
      : {}),
  };
}

/** Render a card frame SVG onto an HTMLCanvasElement at the requested pixel resolution. */
export async function renderCardFrameToCanvas(
  input: CardFrameRenderInput,
  options: CardCanvasRenderOptions = {},
): Promise<HTMLCanvasElement> {
  const width = options.width ?? CARD_WIDTH;
  const height = options.height ?? CARD_HEIGHT;
  // Browsers block external <image href="blob:..."> fetches inside SVG-in-<img>; inline data: URIs first.
  const resolvedInput = await resolveInlineArtInput(input);
  const svgMarkup = renderCardFrameSvg(resolvedInput);

  const blob = new Blob([svgMarkup], { type: "image/svg+xml;charset=utf-8" });
  const objectUrl = URL.createObjectURL(blob);

  try {
    const img = await loadImage(objectUrl);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      throw new Error("2D canvas context unavailable for card frame export");
    }
    ctx.clearRect(0, 0, width, height);
    ctx.drawImage(img, 0, 0, width, height);
    return canvas;
  } finally {
    URL.revokeObjectURL(objectUrl);
  }
}

/** Render a card frame to raw PNG bytes (`Uint8Array`) at 300-DPI (`1062x1482` by default). */
export async function renderCardFrameToPngBytes(
  input: CardFrameRenderInput,
  options: CardCanvasRenderOptions = {},
): Promise<Uint8Array> {
  const canvas = await renderCardFrameToCanvas(input, options);
  const blob = await new Promise<Blob>((resolve, reject) => {
    canvas.toBlob(
      (result) => {
        if (result) resolve(result);
        else reject(new Error("Canvas PNG encoding failed"));
      },
      "image/png",
      1,
    );
  });
  const buf = await blob.arrayBuffer();
  return new Uint8Array(buf);
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.decoding = "async";
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Failed to decode card frame SVG image"));
    img.src = src;
  });
}
