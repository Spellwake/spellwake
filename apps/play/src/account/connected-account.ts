import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useQueryState, parseAsString } from "nuqs";
import {
  cardArtProfileSchema,
  identifierSchema,
  type AccountSession,
  type CardArtProfile,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer, useAccountSummary, useOwnProfile } from "../data/account";
import { useCosmeticCatalogue, usePurchaseHistory } from "../data/cosmetics";
import { useDeckFormats } from "../data/deck-commands";
import { useConfirmedDeletion } from "../data/deletion";
import { scryfallImage } from "../data/scryfall-images";
import { useSocialStatus } from "../data/social";
import { failed, succeeded } from "../decks/deck-sources";
import type {
  AccountSettingsSource,
  ArtChoice,
  BadgeChoice,
  OwnIdentity,
  Portrait,
  PublicProfileSource,
  RankRow,
} from "./account-sources";
import { usePatreonLink } from "./connected-patreon";
import { CENTRED } from "./portrait-crop";
import { purchaseRows } from "./purchase-rows";

export type RegisteredSession = AccountSession & { accountId: string };

const SEARCH_PAUSE_MS = 250;

function parseCardArt(raw: unknown): CardArtProfile | null {
  const parsed = cardArtProfileSchema.safeParse(raw);
  return parsed.success ? parsed.data : null;
}

function rankEmblem(tier: string): { name: string; emblemUrl: string } | null {
  const clean = tier.trim();
  if (!clean) return null;
  const known = [
    "unranked",
    "bronze",
    "silver",
    "gold",
    "platinum",
    "diamond",
    "mythic",
  ];
  const lower = clean.toLowerCase();
  const slug = known.includes(lower) ? lower : "unranked";
  return {
    name: clean.charAt(0).toUpperCase() + clean.slice(1).toLowerCase(),
    emblemUrl: `${import.meta.env.BASE_URL}ranks/${slug}.svg`,
  };
}

/** Resolves a stored `CardArtProfile` to a cropped Scryfall portrait URL. */
function useResolvedPortrait(
  owner: RegisteredSession | null,
  services: ApplicationServices | null,
  artProfile: CardArtProfile | null,
): Portrait | null {
  const printingId = artProfile?.printingId ?? null;
  const meta = useQuery({
    queryKey: ["portrait-art", owner?.accountId, printingId],
    enabled: !!owner && !!services && !!printingId,
    staleTime: Infinity,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !printingId) return null;
      const res = await services.api.cardMetadata(
        await ownerBearer(services, owner),
        { printingIds: [printingId] },
        signal,
      );
      if (res.status !== "success") return null;
      const hit = res.data.printings.find((p) => p.printingId === printingId);
      if (!hit) return null;
      return scryfallImage(hit.externalPrintingRef, "art_crop") ?? hit.imageUrl;
    },
  });
  if (!artProfile || !meta.data) return null;
  return {
    artUrl: meta.data,
    crop: {
      x: artProfile.cropX,
      y: artProfile.cropY,
      scale: artProfile.scale,
    },
  };
}

/** The signed-in account's own portrait and badge for the application header. */
export function useConnectedIdentity(): OwnIdentity {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId
      ? (state.session as RegisteredSession)
      : null;
  const profile = useOwnProfile();
  const publicSummary = useQuery({
    queryKey: ["player-public-summary", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return null;
      const res = await services.api.playerPublicSummary(
        await ownerBearer(services, owner),
        owner.accountId,
        signal,
      );
      return res.status === "success" ? res.data : null;
    },
  });
  const artProfile =
    profile.data?.cardArtProfile ??
    parseCardArt(publicSummary.data?.cardArtProfile);
  const portrait = useResolvedPortrait(owner, services, artProfile);
  const badge: BadgeChoice =
    publicSummary.data?.selectedBadge === "supporter" ? "supporter" : "none";
  return { portrait, badge };
}

/**
 * The signed-in account's settings, wired to profile art, badge, Patreon,
 * Stripe Customer Portal, purchase history, and confirmed account deletion.
 */
export function useConnectedAccountSettings(
  owner: RegisteredSession,
  services: ApplicationServices,
): AccountSettingsSource {
  const summary = useAccountSummary();
  const profile = useOwnProfile();
  const queries = useQueryClient();
  const history = usePurchaseHistory();
  const catalogue = useCosmeticCatalogue();
  const deletion = useConfirmedDeletion(owner, services);
  const patreon = usePatreonLink(owner, services);
  const publicSummary = useQuery({
    queryKey: ["player-public-summary", owner.accountId, owner.sessionId],
    queryFn: async ({ signal }) => {
      const res = await services.api.playerPublicSummary(
        await ownerBearer(services, owner),
        owner.accountId,
        signal,
      );
      if (res.status !== "success") throw new Error("Summary unavailable.");
      return res.data;
    },
  });
  const [savedBadge, setSavedBadge] = useState<BadgeChoice | null>(null);
  const [savedPortrait, setSavedPortrait] = useState<Portrait | null>(null);
  const activeBadge: BadgeChoice =
    savedBadge ??
    (publicSummary.data?.selectedBadge === "supporter" ? "supporter" : "none");
  const canUseSupporter =
    patreon.link === "active" || profile.data?.supporter === true;

  const currentArtProfile =
    profile.data?.cardArtProfile ??
    parseCardArt(publicSummary.data?.cardArtProfile);
  const resolvedPortrait = useResolvedPortrait(
    owner,
    services,
    currentArtProfile,
  );
  const activePortrait = savedPortrait ?? resolvedPortrait;

  // Card artwork search for the portrait editor.
  const [artQuery, setArtQuery] = useState("");
  const [sentArtQuery, setSentArtQuery] = useState("");
  useEffect(() => {
    const timer = window.setTimeout(
      () => setSentArtQuery(artQuery.trim()),
      SEARCH_PAUSE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [artQuery]);
  const artSearch = useQuery({
    queryKey: ["portrait-art-search", owner.accountId, sentArtQuery],
    enabled: sentArtQuery.length > 0,
    queryFn: async ({ signal }) => {
      const res = await services.api.searchCardsRich(
        await ownerBearer(services, owner),
        { query: sentArtQuery, limit: 30 },
        signal,
      );
      if (res.status !== "success") throw new Error("Art search unavailable.");
      const seen = new Set<string>();
      const choices: ArtChoice[] = [];
      for (const item of res.data.items) {
        const artUrl =
          scryfallImage(item.externalPrintingRef, "art_crop") ?? item.imageUrl;
        if (!artUrl || seen.has(item.printingId)) continue;
        seen.add(item.printingId);
        choices.push({
          id: item.printingId,
          name: item.name,
          artist: item.setName || null,
          artUrl,
        });
      }
      return choices;
    },
  });

  // Purchases are named from the cosmetic catalogue, which is read in full.
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = catalogue;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const names = useMemo(
    () =>
      new Map(
        (catalogue.data?.pages ?? []).flatMap((page) =>
          page.items.map((item) => [item.id, item.name] as const),
        ),
      ),
    [catalogue.data],
  );
  const rows = useMemo(
    () =>
      purchaseRows(
        (history.data?.pages ?? []).flatMap((page) => page.items),
        names,
      ),
    [history.data, names],
  );

  if (!summary.data)
    return { status: summary.isError ? "unavailable" : "loading" };
  const isRequested =
    owner.state === "deletion_pending" ||
    summary.data.state === "deletion_pending" ||
    (deletion.saved.status === "saved" &&
      deletion.saved.value.state === "accepted");
  return {
    status: "ready",
    username: summary.data.username,
    portrait: activePortrait,
    badge: activeBadge,
    picture: {
      search: {
        query: artQuery,
        setQuery: setArtQuery,
        state:
          sentArtQuery.length === 0
            ? "ready"
            : artSearch.isError
              ? "failed"
              : artSearch.isPending
                ? "loading"
                : "ready",
        results: sentArtQuery.length === 0 ? [] : (artSearch.data ?? []),
      },
      save: async (art, crop) => {
        try {
          const bearer = await ownerBearer(services, owner);
          const nextProfile: CardArtProfile = {
            printingId: art.id,
            cropX: crop.x,
            cropY: crop.y,
            scale: crop.scale,
          };
          const res = await services.api.updateProfileArt(bearer, {
            requestId: crypto.randomUUID(),
            expected: currentArtProfile,
            cardArtProfile: nextProfile,
          });
          if (res.status !== "success") {
            return failed("Couldn’t save picture.");
          }
          setSavedPortrait({ artUrl: art.artUrl, crop });
          await Promise.all([
            queries.invalidateQueries({
              queryKey: ["own-profile", owner.accountId],
            }),
            queries.invalidateQueries({
              queryKey: ["player-public-summary", owner.accountId],
            }),
          ]);
          return succeeded(undefined);
        } catch {
          return failed("Couldn’t reach the server.");
        }
      },
    },
    badges: {
      canUseSupporter,
      save: async (choice) => {
        try {
          const bearer = await ownerBearer(services, owner);
          const res = await services.api.setProfileBadge(
            bearer,
            choice === "none" ? null : choice,
          );
          if (res.status !== "success") {
            return failed("Couldn’t save badge choice.");
          }
          setSavedBadge(choice);
          await queries.invalidateQueries({
            queryKey: ["player-public-summary", owner.accountId],
          });
          return succeeded(undefined);
        } catch {
          return failed("Couldn’t reach the server.");
        }
      },
    },
    patreon,
    billing: {
      manage: async () => {
        try {
          const bearer = await ownerBearer(services, owner);
          const res = await services.api.createBillingPortalSession(
            bearer,
            window.location.href,
          );
          if (res.status !== "success") {
            const noCustomer =
              res.status === "fail" &&
              "billing" in res.data &&
              res.data.billing === "no_customer";
            return failed(
              noCustomer
                ? "No billing history yet."
                : "Couldn’t open billing portal.",
            );
          }
          window.open(res.data.url, "_blank", "noopener");
          return succeeded(undefined);
        } catch {
          return failed("Couldn’t reach the server.");
        }
      },
      purchases: {
        // A failed read keeps the rows it already has and is never shown as empty.
        state: history.data ? "ready" : history.isError ? "failed" : "loading",
        rows,
        loadMore:
          history.hasNextPage && !history.isFetchingNextPage
            ? () => void history.fetchNextPage()
            : null,
      },
    },
    deletion: {
      state: isRequested ? "requested" : "none",
      request: async () => {
        try {
          await deletion.mutation.mutateAsync();
          return succeeded(undefined);
        } catch {
          // A request that got no answer may still have been received.
          return failed("Couldn’t confirm deletion.");
        }
      },
    },
  };
}

/** Public profile dialog data for any player ID in a connected build. */
export function useConnectedPublicProfile(
  playerId: string,
  owner: RegisteredSession,
  services: ApplicationServices,
): PublicProfileSource {
  const isValidId = identifierSchema.safeParse(playerId).success;
  const [, setMessages] = useQueryState("messages", parseAsString);
  const [, setPlayer] = useQueryState("player", parseAsString);
  const formatsQuery = useDeckFormats();
  const socialStatus = useSocialStatus(
    isValidId && playerId !== owner.accountId ? playerId : null,
    isValidId && playerId !== owner.accountId,
  );
  const summary = useQuery({
    queryKey: ["player-public-summary", owner.accountId, playerId],
    enabled: isValidId,
    queryFn: async ({ signal }) => {
      const res = await services.api.playerPublicSummary(
        await ownerBearer(services, owner),
        playerId,
        signal,
      );
      if (res.status === "fail") return null;
      if (res.status !== "success") throw new Error("Profile failed.");
      return res.data;
    },
  });
  const artProfile = parseCardArt(summary.data?.cardArtProfile);
  const portrait = useResolvedPortrait(owner, services, artProfile);
  const formatNames = useMemo(
    () =>
      new Map(
        (formatsQuery.data?.pages ?? []).flatMap((page) =>
          page.items.map((format) => [format.id, format.name] as const),
        ),
      ),
    [formatsQuery.data],
  );

  if (!isValidId) return { status: "unavailable" };
  if (summary.isPending) return { status: "loading" };
  if (summary.isError && !summary.data) return { status: "failed" };
  if (!summary.data || summary.data.person.isDeleted)
    return { status: "unavailable" };

  const ranks: RankRow[] = summary.data.ranks.map((entry) => ({
    format: formatNames.get(entry.formatId) ?? "Format",
    tier: rankEmblem(entry.tier),
    rating: Number(entry.rating) || 1000,
    record: summary.data!.record,
  }));
  const canMessage =
    playerId !== owner.accountId &&
    socialStatus.data?.person?.canContact === true;

  return {
    status: "ready",
    username: summary.data.person.username,
    portrait: portrait ?? (artProfile ? { artUrl: "", crop: CENTRED } : null),
    badge:
      summary.data.selectedBadge === "supporter" && summary.data.supporter
        ? "supporter"
        : "none",
    season: null,
    ranks,
    isStale: summary.isError,
    message: canMessage
      ? async () => {
          await setPlayer(null);
          await setMessages(playerId);
          return succeeded(undefined);
        }
      : null,
  };
}

/** Fallback empty identity when signed out or unconfigured. */
export const CONNECTED_IDENTITY: OwnIdentity = {
  portrait: null,
  badge: "none",
};
