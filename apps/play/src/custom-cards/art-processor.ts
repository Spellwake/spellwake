/**
 * Client-side raster art processor for Spellwake Custom Card Studio.
 *
 * - Rejects SVG / non-raster uploads.
 * - Re-encodes uploaded face artwork via Canvas into WebP (or PNG fallback) <= 128 KiB,
 *   stripping all EXIF/GPS metadata automatically.
 * - Downscales external card screenshots <= 512 KiB for Vision OCR fallback.
 * - Applies normalized `artCropBox` cropping returned by Vision OCR cleanly.
 */

export const MAX_FACE_ART_BYTES = 131_072;
export const MAX_VISION_UPLOAD_BYTES = 524_288;

export interface ProcessedFaceArt {
  mimeType: "image/webp" | "image/png";
  dataUrl: string;
  base64: string;
  byteLength: number;
}

export interface NormalizedCropBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

function isAllowedRasterMime(mime: string): boolean {
  return mime === "image/png" || mime === "image/jpeg" || mime === "image/webp";
}

function loadBitmapFromBlob(blob: Blob): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(blob);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      resolve(img);
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("Could not decode image file."));
    };
    img.src = url;
  });
}

function canvasToBlob(
  canvas: HTMLCanvasElement,
  mime: string,
  quality?: number,
): Promise<Blob | null> {
  return new Promise((resolve) => {
    canvas.toBlob((blob) => resolve(blob), mime, quality);
  });
}

async function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error("Failed to encode image bytes."));
    reader.readAsDataURL(blob);
  });
}

function stripDataUrlPrefix(dataUrl: string): string {
  const comma = dataUrl.indexOf(",");
  return comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
}

/**
 * Compress and EXIF-strip an uploaded raster image into `image/webp` (or `image/png`)
 * guaranteed to stay under 128 KiB for custom card face persistence.
 */
export async function processFaceArtFile(
  file: Blob,
  cropBox?: NormalizedCropBox | null,
): Promise<ProcessedFaceArt> {
  if (file.type && !isAllowedRasterMime(file.type)) {
    throw new Error("Only PNG, JPEG, or WebP raster images are supported (SVG is not allowed).");
  }
  const img = await loadBitmapFromBlob(file);
  const srcX = cropBox ? Math.max(0, Math.round(cropBox.x * img.naturalWidth)) : 0;
  const srcY = cropBox ? Math.max(0, Math.round(cropBox.y * img.naturalHeight)) : 0;
  const srcW = cropBox
    ? Math.max(16, Math.min(img.naturalWidth - srcX, Math.round(cropBox.width * img.naturalWidth)))
    : img.naturalWidth;
  const srcH = cropBox
    ? Math.max(16, Math.min(img.naturalHeight - srcY, Math.round(cropBox.height * img.naturalHeight)))
    : img.naturalHeight;

  const scales = [1, 0.85, 0.7, 0.55, 0.4];
  const qualities = [0.86, 0.74, 0.62, 0.5];

  for (const scale of scales) {
    const maxDim = Math.round(894 * scale);
    const ratio = Math.min(1, maxDim / Math.max(srcW, srcH));
    const dstW = Math.max(64, Math.round(srcW * ratio));
    const dstH = Math.max(64, Math.round(srcH * ratio));

    const canvas = document.createElement("canvas");
    canvas.width = dstW;
    canvas.height = dstH;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas context unavailable.");
    ctx.drawImage(img, srcX, srcY, srcW, srcH, 0, 0, dstW, dstH);

    for (const q of qualities) {
      const webp = await canvasToBlob(canvas, "image/webp", q);
      if (webp && webp.type === "image/webp" && webp.size <= MAX_FACE_ART_BYTES) {
        const dataUrl = await blobToDataUrl(webp);
        return {
          mimeType: "image/webp",
          dataUrl,
          base64: stripDataUrlPrefix(dataUrl),
          byteLength: webp.size,
        };
      }
    }

    const png = await canvasToBlob(canvas, "image/png");
    if (png && png.size <= MAX_FACE_ART_BYTES) {
      const dataUrl = await blobToDataUrl(png);
      return {
        mimeType: "image/png",
        dataUrl,
        base64: stripDataUrlPrefix(dataUrl),
        byteLength: png.size,
      };
    }
  }

  throw new Error("Could not compress artwork under the 128 KiB limit.");
}

/**
 * Downscale an external PNG/JPEG/WebP card screenshot under 512 KiB for Vision OCR.
 */
export async function prepareVisionOcrUpload(file: Blob): Promise<{
  mimeType: "image/png" | "image/jpeg" | "image/webp";
  base64: string;
  byteLength: number;
}> {
  if (!isAllowedRasterMime(file.type)) {
    throw new Error("Vision import supports PNG, JPEG, and WebP images only.");
  }
  if (file.size <= MAX_VISION_UPLOAD_BYTES) {
    const dataUrl = await blobToDataUrl(file);
    return {
      mimeType: file.type as "image/png" | "image/jpeg" | "image/webp",
      base64: stripDataUrlPrefix(dataUrl),
      byteLength: file.size,
    };
  }
  const img = await loadBitmapFromBlob(file);
  for (const maxSide of [1200, 960, 768, 600]) {
    const ratio = Math.min(1, maxSide / Math.max(img.naturalWidth, img.naturalHeight));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(64, Math.round(img.naturalWidth * ratio));
    canvas.height = Math.max(64, Math.round(img.naturalHeight * ratio));
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas context unavailable.");
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);

    for (const q of [0.85, 0.72, 0.58]) {
      const jpeg = await canvasToBlob(canvas, "image/jpeg", q);
      if (jpeg && jpeg.size <= MAX_VISION_UPLOAD_BYTES) {
        const dataUrl = await blobToDataUrl(jpeg);
        return {
          mimeType: "image/jpeg",
          base64: stripDataUrlPrefix(dataUrl),
          byteLength: jpeg.size,
        };
      }
    }
  }
  throw new Error("Image is too large for Vision OCR import (512 KiB max).");
}
