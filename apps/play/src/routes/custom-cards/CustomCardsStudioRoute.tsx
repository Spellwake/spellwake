import { useCallback, useEffect, useMemo, useState, type ReactElement } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useSearchParams } from "react-router-dom";
import type {
  CustomCardCompileInput,
  CustomCardFaceRules,
  VisionImportResult,
} from "@spellwake/application-contracts";
import { useAccount } from "../../auth/AccountProvider";
import { ownerBearer, useOwnProfile } from "../../data/account";
import {
  INITIAL_DEMO_CUSTOM_CARDS,
  demoCompileCard,
  demoStandardizeFaces,
  type DemoVaultCard,
} from "../../data/demo-custom-cards";
import {
  CustomCardsStudioPage,
  type CustomCardsStudioSource,
} from "./CustomCardsStudioPage";

const useIsPreview = (): boolean => useAccount().state.status === "unconfigured";

function PreviewStudio(): ReactElement {
  const [searchParams] = useSearchParams();
  const [cards, setCards] = useState<DemoVaultCard[]>(() => INITIAL_DEMO_CUSTOM_CARDS);
  const isPatron = searchParams.get("patron") === "1";

  const source = useMemo<CustomCardsStudioSource>(
    () => ({
      mode: "preview",
      apiOrigin: "",
      bearer: null,
      isPatron,
      cards,
      activeCount: cards.filter((c) => c.archivedAt === null).length,
      activeLimit: 250,
      isLoading: false,
      compile: async (input: CustomCardCompileInput) => demoCompileCard(input),
      standardize: async (faces: CustomCardFaceRules[]) => demoStandardizeFaces(faces),
      saveCard: async (input) => {
        const compiled = demoCompileCard({
          layout: input.layout,
          rarity: input.rarity,
          faces: input.faces,
        });
        const cardId = input.cardId ?? crypto.randomUUID();
        const revisionId = crypto.randomUUID();
        const now = new Date().toISOString();
        const frontName = input.faces[0]?.name ?? "Untitled Card";
        const backName = input.faces[1]?.name ?? "";
        const nextName =
          input.faces.length === 2
            ? `${frontName} // ${backName}`
            : frontName;

        setCards((prev) => {
          const existing = prev.find((c) => c.id === cardId);
          const nextRevNum = existing
            ? String(Number(existing.currentRevision.revisionNumber) + 1)
            : "1";
          const demoArtUrls: Partial<Record<0 | 1, string>> = {
            ...(existing?.demoArtUrls ?? {}),
          };
          for (const a of input.art) {
            if (a.artBase64 && a.mimeType) {
              demoArtUrls[a.faceIndex as 0 | 1] =
                `data:${a.mimeType};base64,${a.artBase64}`;
            }
          }
          const updated: DemoVaultCard = {
            id: cardId,
            archivedAt: existing?.archivedAt ?? null,
            createdAt: existing?.createdAt ?? now,
            updatedAt: now,
            demoArtUrls,
            currentRevision: {
              revisionId,
              customCardId: cardId,
              revisionNumber: nextRevNum,
              rulesSha256: compiled.rulesSha256,
              name: nextName,
              layout: input.layout,
              rarity: input.rarity,
              faces: input.faces,
              visualFrameAccents: input.visualFrameAccents,
              flavorTexts: input.flavorTexts,
              collectorNote: input.collectorNote,
              stage1Parsed: compiled.stage1Parsed,
              stage2Playable: compiled.stage2Playable,
              compilerRevision: compiled.compilerRevision,
              createdAt: now,
              art: input.art.map((a) => ({
                faceIndex: a.faceIndex,
                mimeType: a.mimeType ?? "image/webp",
                artSha256:
                  "0000000000000000000000000000000000000000000000000000000000000001",
                hasArt: true,
                cropX: a.cropX,
                cropY: a.cropY,
                zoom: a.zoom,
                artist: a.artist ?? null,
              })),
            },
          };
          return existing
            ? prev.map((c) => (c.id === cardId ? updated : c))
            : [updated, ...prev];
        });

        return { ok: true, cardId, revisionId, compile: compiled };
      },
      archiveCard: async (cardId, archived) => {
        setCards((prev) =>
          prev.map((c) =>
            c.id === cardId
              ? { ...c, archivedAt: archived ? new Date().toISOString() : null }
              : c,
          ),
        );
        return true;
      },
      deleteCard: async (cardId) => {
        setCards((prev) => prev.filter((c) => c.id !== cardId));
        return "deleted";
      },
      importVision: async ({ fileName }): Promise<VisionImportResult> => {
        await new Promise((r) => window.setTimeout(r, 700));
        const faces: CustomCardFaceRules[] = [
          {
            name: "Stormwing Griffin",
            manaCost: "{2}{W}{U}",
            typeLine: "Creature \u2014 Griffin",
            oracleText:
              "Flying, vigilance\nWhen Stormwing Griffin enters, scry 2.",
            power: "3",
            toughness: "3",
            loyalty: null,
            defense: null,
            colorIndicator: null,
          },
        ];
        const standardized = demoStandardizeFaces(faces);
        const compiled = demoCompileCard({
          layout: "normal",
          rarity: "rare",
          faces: standardized.faces,
        });
        return {
          cards: [
            {
              layout: "normal",
              rarity: "rare",
              faces: standardized.faces,
              flavorTexts: ["Born where the gale meets the spire."],
              collectorNote: null,
              artist: "Imported via Preview OCR",
              artCropBox: { x: 0.08, y: 0.11, width: 0.84, height: 0.44 },
              standardized,
              compiled,
            },
          ],
          warnings: [`Preview Vision OCR parsed ${fileName} locally.`],
        };
      },
      quickTest: async () => ({
        ok: true,
        roomId: "preview-quick-test",
      }),
    }),
    [cards, isPatron],
  );

  return <CustomCardsStudioPage source={source} />;
}

function AccountStudio(): ReactElement {
  const { state, services } = useAccount();
  const queries = useQueryClient();
  const profile = useOwnProfile();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;

  const [bearer, setBearer] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    if (!owner || !services) {
      setBearer(null);
      return;
    }
    void ownerBearer(services, owner)
      .then((tok) => {
        if (alive) setBearer(tok);
      })
      .catch(() => {
        if (alive) setBearer(null);
      });
    return () => {
      alive = false;
    };
  }, [owner, services]);

  const vaultQuery = useQuery({
    queryKey: ["custom-cards-vault", owner?.accountId, owner?.sessionId],
    enabled: owner !== null && services !== null,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to load custom cards.");
      const token = await ownerBearer(services, owner);
      const res = await services.api.listCustomCards(
        token,
        { limit: 100, includeArchived: true },
        signal,
      );
      if (res.status !== "success") {
        throw new Error("Could not load custom cards.");
      }
      return res.data;
    },
  });

  const getToken = useCallback(async () => {
    if (!owner || !services) throw new Error("Sign in required.");
    const tok = await ownerBearer(services, owner);
    setBearer(tok);
    return tok;
  }, [owner, services]);

  const source = useMemo<CustomCardsStudioSource>(
    () => ({
      mode: "connected",
      apiOrigin: services?.api.origin ?? "",
      bearer,
      isPatron: profile.data?.supporter ?? false,
      cards: vaultQuery.data?.items ?? [],
      activeCount: vaultQuery.data?.activeCount ?? 0,
      activeLimit: vaultQuery.data?.activeLimit ?? 250,
      isLoading: vaultQuery.isLoading,
      compile: async (input) => {
        if (!services) return null;
        try {
          const res = await services.api.compileCustomCard(await getToken(), input);
          return res.status === "success" ? res.data : null;
        } catch {
          return null;
        }
      },
      standardize: async (faces) => {
        if (!services) return null;
        try {
          const res = await services.api.standardizeCustomCard(await getToken(), faces);
          return res.status === "success" ? res.data : null;
        } catch {
          return null;
        }
      },
      saveCard: async (input) => {
        if (!services) return { ok: false, message: "Application API unavailable." };
        try {
          const token = await getToken();
          const req = {
            requestId: crypto.randomUUID(),
            expectedRevisionId: input.expectedRevisionId,
            layout: input.layout,
            rarity: input.rarity,
            faces: input.faces,
            visualFrameAccents: input.visualFrameAccents,
            flavorTexts: input.flavorTexts,
            collectorNote: input.collectorNote,
            art: input.art,
          };
          const res = input.cardId
            ? await services.api.saveCustomCardRevision(token, input.cardId, req)
            : await services.api.createCustomCard(token, req);
          if (res.status === "success") {
            void queries.invalidateQueries({ queryKey: ["custom-cards-vault"] });
            return {
              ok: true,
              cardId: res.data.customCardId,
              revisionId: res.data.revisionId,
              compile: res.data.compile,
            };
          }
          const msg =
            res.status === "fail"
              ? Object.values(res.data)[0] ?? "Could not save custom card."
              : res.message;
          return { ok: false, message: msg };
        } catch {
          return { ok: false, message: "Could not reach Spellwake to save this card." };
        }
      },
      archiveCard: async (cardId, archived) => {
        if (!services) return false;
        try {
          const res = await services.api.archiveCustomCard(await getToken(), cardId, {
            requestId: crypto.randomUUID(),
            archived,
          });
          if (res.status === "success") {
            void queries.invalidateQueries({ queryKey: ["custom-cards-vault"] });
            return true;
          }
          return false;
        } catch {
          return false;
        }
      },
      deleteCard: async (cardId) => {
        if (!services) return null;
        try {
          const res = await services.api.deleteCustomCard(
            await getToken(),
            cardId,
            crypto.randomUUID(),
          );
          if (res.status === "success") {
            void queries.invalidateQueries({ queryKey: ["custom-cards-vault"] });
            return res.data.disposition ?? "deleted";
          }
          return null;
        } catch {
          return null;
        }
      },
      importVision: async ({ sourceKind, imageDataUrl }) => {
        if (!services) return null;
        try {
          const res = await services.api.importCustomCardVision(await getToken(), {
            sourceKind,
            imageDataUrl,
          });
          return res.status === "success" ? res.data : null;
        } catch {
          return null;
        }
      },
      quickTest: async (revisionId, aiPlaysCustomCard) => {
        if (!services) return { ok: false, message: "Application API unavailable." };
        try {
          const res = await services.api.quickTestCustomCard(await getToken(), {
            requestId: crypto.randomUUID(),
            customCardRevisionId: revisionId,
            aiPlaysCustomCard,
          });
          if (res.status === "success") {
            return { ok: true, roomId: res.data.roomId };
          }
          const msg =
            res.status === "fail"
              ? Object.values(res.data)[0] ?? "Could not start Quick Test."
              : res.message;
          return { ok: false, message: msg };
        } catch {
          return { ok: false, message: "Could not start Quick Test room." };
        }
      },
    }),
    [
      services,
      bearer,
      profile.data?.supporter,
      vaultQuery.data,
      vaultQuery.isLoading,
      getToken,
      queries,
    ],
  );

  return <CustomCardsStudioPage source={source} />;
}

export function CustomCardsStudioRoute(): ReactElement {
  return useIsPreview() ? <PreviewStudio /> : <AccountStudio />;
}
