import { z } from "zod";
import { DECK_TEXT_SECTIONS } from "../../data/deck-text";

import { DEMO_FORMATS } from "./demo-deck-presets";
export { DEMO_FORMATS };
export const DEMO_SECTIONS = DECK_TEXT_SECTIONS;
/**
 * A deck box's look: its colour identity, and its face card, a card from
 * the deck chosen by name whose official art is printed on the box.
 */
export const demoAppearanceSchema = z
  .object({
    colors: z.array(z.enum(["W", "U", "B", "R", "G"])).max(5),
    faceCard: z.string().trim().min(1).max(240).nullable().default(null),
    /**
     * Retired generated cover art. Saves from before face cards still carry
     * it; it is read and dropped, and the next save writes it away.
     */
    artwork: z.string().optional(),
  })
  .strict()
  .transform(({ colors, faceCard }) => ({ colors, faceCard }));

/** Local demo text is not a canonical card identity, legality result, or owned card. */
export const demoDeckEntrySchema = z
  .object({
    id: z.string().uuid(),
    name: z.string().trim().min(1).max(240),
    quantity: z
      .string()
      .regex(/^[1-9][0-9]{0,77}$/, "Enter a positive whole quantity."),
    section: z.enum(DEMO_SECTIONS),
    sourceText: z.string().max(2000).optional(),
    catalogue: z
      .object({
        oracleId: z.string().uuid(),
        printingId: z.string().uuid().optional(),
        type: z.string(),
        manaValue: z.number().nonnegative().nullable(),
      })
      .strict()
      .optional(),
  })
  .strict();

export const demoDeckInputSchema = z
  .object({
    name: z.string().trim().min(1, "Enter a deck name.").max(120),
    format: z.enum(DEMO_FORMATS),
    mode: z.enum(["open", "collection"]).default("open"),
    entries: z.array(demoDeckEntrySchema).max(1000).default([]),
    appearance: demoAppearanceSchema.default({ colors: [], faceCard: null }),
    /** Sleeves, card back, dice, playmat and pet, by preview cosmetic ID. Older decks have none. */
    cosmetics: z
      .object({
        sleeve: z.string().min(1).max(120).nullable(),
        back: z.string().min(1).max(120).nullable(),
        dice: z.string().min(1).max(120).nullable(),
        playmat: z.string().min(1).max(120).nullable(),
        pet: z.string().min(1).max(120).nullable(),
      })
      .strict()
      .optional(),
    sourceText: z.string().max(1_000_000).optional(),
  })
  .strict();

export const demoDeckSchema = demoDeckInputSchema
  .extend({
    mode: z.enum(["open", "collection"]),
    entries: z.array(demoDeckEntrySchema).max(1000),
    appearance: demoAppearanceSchema,
    id: z.string().uuid(),
    revision: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .superRefine((deck, context) => {
    if (
      new Set(deck.entries.map((entry) => entry.id)).size !==
      deck.entries.length
    ) {
      context.addIssue({
        code: "custom",
        path: ["entries"],
        message: "Entry IDs must be unique.",
      });
    }
  });

export const demoLibrarySchema = z
  .object({
    version: z.literal(1),
    decks: z.array(demoDeckSchema).max(2000),
  })
  .strict()
  .superRefine((library, context) => {
    if (
      new Set(library.decks.map((deck) => deck.id)).size !==
      library.decks.length
    ) {
      context.addIssue({
        code: "custom",
        path: ["decks"],
        message: "Deck IDs must be unique.",
      });
    }
  });

export type DemoDeck = z.infer<typeof demoDeckSchema>;
export type DemoDeckInput = z.input<typeof demoDeckInputSchema>;
export type DemoDeckEntry = z.infer<typeof demoDeckEntrySchema>;
export type DemoLibrary = z.infer<typeof demoLibrarySchema>;
export type DemoFormat = (typeof DEMO_FORMATS)[number];
export type DemoSection = (typeof DEMO_SECTIONS)[number];
