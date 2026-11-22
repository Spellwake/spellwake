/** OAuth benefits association intent only. Persistence/callback handling belongs to the UI owner. */
import {
  patreonLinkRequestSchema,
  type PatreonLinkRequest,
} from "@spellwake/application-contracts";
import { newPrivateCapability } from "../utils/private-capability";
/** Create a new authorization identity; never reuse this state to retry a spent exchange. */
export function newPatreonLinkRequest(): PatreonLinkRequest {
  return patreonLinkRequestSchema.parse({
    requestId: crypto.randomUUID(),
    stateKey: newPrivateCapability(),
  });
}
