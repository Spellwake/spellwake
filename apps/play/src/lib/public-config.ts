/** Only HTTPS or explicit loopback HTTP origins can receive credentials. */
export function publicOrigin(value: string): string | null {
  try {
    const url = new URL(value);
    if (
      url.origin !== value ||
      url.username ||
      url.password ||
      url.hostname.includes("*") ||
      url.search ||
      url.hash ||
      /[\s;'"<>]/.test(value) ||
      !(
        url.protocol === "https:" ||
        (url.protocol === "http:" &&
          ["127.0.0.1", "[::1]"].includes(url.hostname))
      )
    )
      return null;
    return url.origin;
  } catch {
    return null;
  }
}
export type PublicConfiguration = {
  authOrigin: string;
  apiOrigin: string;
  publishableKey: string;
};
export type PublicConfigurationResult =
  | { status: "ready"; config: PublicConfiguration }
  | { status: "unconfigured" }
  | { status: "invalid" };
/** Builds and the SDK use the same three settings, without reading any private key. */
export function parsePublicConfiguration(
  environment: Record<string, unknown>,
): PublicConfigurationResult {
  const auth = environment["VITE_SUPABASE_URL"];
  const api = environment["VITE_APPLICATION_API_URL"];
  const key = environment["VITE_SUPABASE_PUBLISHABLE_KEY"];
  if ([auth, api, key].every((value) => value === undefined || value === ""))
    return { status: "unconfigured" };
  if (
    typeof auth !== "string" ||
    typeof api !== "string" ||
    typeof key !== "string" ||
    !publicOrigin(auth) ||
    !publicOrigin(api) ||
    !/^sb_publishable_[A-Za-z0-9_-]+$/.test(key)
  )
    return { status: "invalid" };
  return {
    status: "ready",
    config: { authOrigin: auth, apiOrigin: api, publishableKey: key },
  };
}
