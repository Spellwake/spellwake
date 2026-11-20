/** Catalogue crop references only, paired with the existing PostgreSQL float8 columns. */
import { z } from "zod";
import { identifierSchema, publicAccountIdentitySchema } from "./primitives.ts";
export const cardArtProfileSchema = z.strictObject({
  printingId: identifierSchema,
  cropX: z.number().finite().min(0).max(1),
  cropY: z.number().finite().min(0).max(1),
  scale: z.number().finite().positive(),
});
export const changeProfileArtSchema = z.strictObject({
  requestId: identifierSchema,
  expected: cardArtProfileSchema.nullable(),
  cardArtProfile: cardArtProfileSchema.nullable(),
});
export const ownProfileArtSchema = z.strictObject({
  accountId: identifierSchema,
  cardArtProfile: cardArtProfileSchema.nullable(),
  supporter: z.boolean(),
});
export const publicProfileArtSchema = z
  .strictObject({
    viewerAccountId: identifierSchema,
    person: publicAccountIdentitySchema,
    cardArtProfile: cardArtProfileSchema.nullable(),
    supporter: z.boolean(),
  })
  .refine(
    (p) => !p.person.isDeleted || (p.cardArtProfile === null && !p.supporter),
  );
export const profileArtErrorsSchema = z.union([
  z.strictObject({ account: z.literal("account unavailable") }),
  z.strictObject({
    profile: z.literal("profile art changed; reload before saving"),
  }),
  z.strictObject({ art: z.literal("catalogue art unavailable") }),
]);
export type CardArtProfile = z.infer<typeof cardArtProfileSchema>;
export type OwnProfileArt = z.infer<typeof ownProfileArtSchema>;
export type PublicProfileArt = z.infer<typeof publicProfileArtSchema>;
/** Bind owner responses to the authenticated caller after structural decoding. */
export function validOwnProfileArt(
  profile: OwnProfileArt,
  owner: string,
): boolean {
  return (
    identifierSchema.safeParse(owner).success && profile.accountId === owner
  );
}
/** The public target identity is not a messaging/trading permission. */
export function validPublicProfileArt(
  profile: PublicProfileArt,
  viewer: string,
  target: string,
): boolean {
  return (
    identifierSchema.safeParse(viewer).success &&
    identifierSchema.safeParse(target).success &&
    profile.viewerAccountId === viewer &&
    profile.person.id === target
  );
}

/** A durable profile edit acknowledgment belongs to the original caller. */
export function validProfileArtAck(
  ack: { resourceId: string },
  owner: string,
): boolean {
  return identifierSchema.safeParse(owner).success && ack.resourceId === owner;
}
