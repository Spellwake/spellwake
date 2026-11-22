import type { ApplicationServices } from "./services";

export type SignInProvider = "google" | "apple" | "discord";
export const NATIVE_AUTH_CALLBACK = "com.spellwake.client://auth/callback";
const destinations = ["/collection", "/cosmetics"];
export function authDestination(value: string | null): string {
  return value && destinations.includes(value) ? value : "/collection";
}
export function browserAuthCallback(origin: string, base: string): string {
  return new URL(`${base.replace(/\/$/, "")}/auth/callback`, origin).href;
}
type AuthReturn = { code: string; flowId: string; next: string };
/** Deep links and browser parameters select a PKCE exchange, never an account or session. */
export function parseAuthReturn(
  raw: string,
  expected: string,
): AuthReturn | null {
  if (raw.length > 8192) return null;
  try {
    const url = new URL(raw);
    const target = new URL(expected);
    if (
      url.protocol !== target.protocol ||
      url.host !== target.host ||
      url.pathname !== target.pathname ||
      url.username ||
      url.password ||
      url.hash
    )
      return null;
    for (const key of url.searchParams.keys())
      if (
        !["code", "sb_flow_id", "next"].includes(key) ||
        url.searchParams.getAll(key).length !== 1
      )
        return null;
    const code = url.searchParams.get("code");
    const flowId = url.searchParams.get("sb_flow_id");
    if (
      !code ||
      !/^[A-Za-z0-9._~-]{1,2048}$/.test(code) ||
      !flowId ||
      !/^[a-fA-F0-9]{32}$/.test(flowId)
    )
      return null;
    return {
      code,
      flowId,
      next: authDestination(url.searchParams.get("next")),
    };
  } catch {
    return null;
  }
}
const exchanges = new WeakMap<
  ApplicationServices["auth"],
  Map<string, Promise<boolean>>
>();
/** Strict Mode and native getCurrent/event redelivery share one code exchange. */
export function exchangeAuthReturn(
  services: ApplicationServices,
  returned: AuthReturn,
): Promise<boolean> {
  let pending = exchanges.get(services.auth);
  if (!pending) {
    pending = new Map();
    exchanges.set(services.auth, pending);
  }
  const key = returned.flowId + ":" + returned.code;
  const prior = pending.get(key);
  if (prior) return prior;
  const work = (async () => {
    try {
      const result = await services.auth.exchangeCodeForSession(returned.code, {
        flowId: returned.flowId,
      });
      if (result.error || !result.data.session) {
        console.warn("account_auth code=exchange_rejected");
        return false;
      }
      return true;
    } catch {
      console.warn("account_auth code=exchange_unavailable");
      return false;
    }
  })();
  pending.set(key, work);
  if (pending.size > 32) {
    const oldest = pending.keys().next().value;
    if (oldest) pending.delete(oldest);
  }
  return work;
}
/** The SDK builds the PKCE authorization URL; only its configured Auth endpoint may open. */
export async function beginProviderSignIn(
  services: ApplicationServices,
  provider: SignInProvider,
  callback: string,
  next: string,
  native: boolean,
): Promise<boolean> {
  try {
    const redirect = new URL(callback);
    redirect.searchParams.set("next", authDestination(next));
    const result = await services.auth.signInWithOAuth({
      provider,
      options: { redirectTo: redirect.href, skipBrowserRedirect: true },
    });
    if (result.error || !result.data.url) {
      console.warn("account_auth code=provider_rejected");
      return false;
    }
    const authorization = new URL(result.data.url);
    if (
      authorization.origin !== services.authOrigin ||
      authorization.pathname !== "/auth/v1/authorize" ||
      authorization.username ||
      authorization.password ||
      authorization.hash ||
      authorization.searchParams.get("code_challenge_method") !== "s256" ||
      authorization.searchParams.getAll("provider").length !== 1 ||
      authorization.searchParams.get("provider") !== provider
    ) {
      console.warn("account_auth code=provider_contract");
      return false;
    }
    if (native) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(authorization.href);
    } else window.location.assign(authorization.href);
    return true;
  } catch {
    console.warn("account_auth code=provider_unavailable");
    return false;
  }
}
