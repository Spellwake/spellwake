/**
 * Ref-counted authenticated binary art cache (`blob:` object URLs) for custom cards.
 *
 * Every custom card art endpoint (`/v1/custom-cards/revisions/{rev}/art/{face}`,
 * `/v1/rooms/{room}/lent-decks/{deck}/face-art/{rev}`, and
 * `/v1/games/{game}/custom-cards/{rev}/art/{face}?contextId=...`) requires a Bearer token
 * and returns immutable private binary PNG/WebP bytes. This cache:
 * - Deduplicates in-flight fetches per path.
 * - Ref-counts active consumers so `URL.revokeObjectURL` fires cleanly when unmounted.
 * - Supports explicit `clearCustomArtCache()` on sign-out or session switch.
 */

import { useEffect, useState } from "react";

interface CacheEntry {
  blobUrl: string;
  refCount: number;
}

const entries = new Map<string, CacheEntry>();
const inflight = new Map<string, Promise<string | null>>();

export function acquireCustomArtUrl(
  apiOrigin: string,
  artPath: string,
  bearer: string | null,
): Promise<string | null> {
  if (!artPath || !bearer) return Promise.resolve(null);
  if (artPath.startsWith("data:") || artPath.startsWith("blob:")) {
    return Promise.resolve(artPath);
  }
  const key = `${apiOrigin}${artPath}`;
  const existing = entries.get(key);
  if (existing) {
    existing.refCount += 1;
    return Promise.resolve(existing.blobUrl);
  }
  const pending = inflight.get(key);
  if (pending) {
    return pending.then((url) => {
      if (url) {
        const entry = entries.get(key);
        if (entry) entry.refCount += 1;
      }
      return url;
    });
  }

  const task = fetch(key, {
    method: "GET",
    headers: { Authorization: `Bearer ${bearer}` },
  })
    .then(async (resp) => {
      if (!resp.ok) return null;
      const blob = await resp.blob();
      const blobUrl = URL.createObjectURL(blob);
      entries.set(key, { blobUrl, refCount: 1 });
      return blobUrl;
    })
    .catch(() => null)
    .finally(() => {
      inflight.delete(key);
    });

  inflight.set(key, task);
  return task;
}

export function releaseCustomArtUrl(apiOrigin: string, artPath: string): void {
  if (!artPath || artPath.startsWith("data:") || artPath.startsWith("blob:")) return;
  const key = `${apiOrigin}${artPath}`;
  const entry = entries.get(key);
  if (!entry) return;
  entry.refCount -= 1;
  if (entry.refCount <= 0) {
    URL.revokeObjectURL(entry.blobUrl);
    entries.delete(key);
  }
}

export function clearCustomArtCache(): void {
  for (const entry of entries.values()) {
    URL.revokeObjectURL(entry.blobUrl);
  }
  entries.clear();
  inflight.clear();
}

export function customArtCacheSize(): number {
  return entries.size;
}

/** React hook resolving an authenticated custom card art path to a managed `blob:` URL. */
export function useAuthenticatedCustomArt(
  apiOrigin: string,
  artPath: string | null | undefined,
  bearer: string | null,
): string | null {
  const [url, setUrl] = useState<string | null>(() => {
    if (!artPath) return null;
    if (artPath.startsWith("data:") || artPath.startsWith("blob:")) return artPath;
    return null;
  });

  useEffect(() => {
    if (!artPath) {
      setUrl(null);
      return;
    }
    if (artPath.startsWith("data:") || artPath.startsWith("blob:")) {
      setUrl(artPath);
      return;
    }
    if (!bearer) {
      setUrl(null);
      return;
    }
    let active = true;
    let acquired = false;
    void acquireCustomArtUrl(apiOrigin, artPath, bearer).then((resolved) => {
      if (!active) {
        if (resolved) releaseCustomArtUrl(apiOrigin, artPath);
        return;
      }
      if (resolved) {
        acquired = true;
        setUrl(resolved);
      } else {
        setUrl(null);
      }
    });
    return () => {
      active = false;
      if (acquired) {
        releaseCustomArtUrl(apiOrigin, artPath);
      }
    };
  }, [apiOrigin, artPath, bearer]);

  return url;
}
