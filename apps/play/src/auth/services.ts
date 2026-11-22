/** Public client configuration only. SDK persistence is separate from demo state. */
import { createClient, type SupabaseClient } from "@supabase/supabase-js";
import { ApplicationApi } from "../data/application-api";
import { parsePublicConfiguration } from "../lib/public-config";
import { createAuthPersistence, type AuthPersistence } from "./storage";

export type ApplicationServices = {
  auth: SupabaseClient["auth"];
  api: ApplicationApi;
  authOrigin: string;
  persistence: AuthPersistence;
};
export type ServicesState =
  | { status: "ready"; services: ApplicationServices }
  | { status: "unconfigured" };

/** Signing out this device preserves the account's other native sessions. */
export async function endCurrentSession(
  services: ApplicationServices,
): Promise<boolean> {
  try {
    const result = await services.auth.signOut({ scope: "local" });
    return result.error === null;
  } catch {
    console.warn("account_auth code=signout_unavailable");
    return false;
  }
}

let defaultServices: ServicesState | undefined;
/** Strict Mode and repeated app mounts share one SDK instance and refresh lock. */
export function defaultApplicationServices(): ServicesState {
  defaultServices ??= createApplicationServices({
    VITE_SUPABASE_URL: import.meta.env["VITE_SUPABASE_URL"],
    VITE_APPLICATION_API_URL: import.meta.env["VITE_APPLICATION_API_URL"],
    VITE_SUPABASE_PUBLISHABLE_KEY: import.meta.env[
      "VITE_SUPABASE_PUBLISHABLE_KEY"
    ],
  });
  return defaultServices;
}

/** Missing or invalid deployment configuration cannot silently select a demo backend. */
export function createApplicationServices(
  environment: Record<string, unknown>,
): ServicesState {
  const selected = parsePublicConfiguration(environment);
  if (selected.status !== "ready") return { status: "unconfigured" };
  const {
    authOrigin: authUrl,
    apiOrigin: apiUrl,
    publishableKey: key,
  } = selected.config;
  const api = ApplicationApi.create(apiUrl);
  if (!api) return { status: "unconfigured" };
  const persistence = createAuthPersistence(authUrl);
  const client = createClient(authUrl, key, {
    auth: {
      flowType: "pkce",
      detectSessionInUrl: false,
      persistSession: true,
      autoRefreshToken: true,
      experimental: { appendPkceFlowIdToRedirects: true },
      storageKey: persistence.key,
      storage: persistence.storage,
    },
  });
  return {
    status: "ready",
    services: { auth: client.auth, api, authOrigin: authUrl, persistence },
  };
}
