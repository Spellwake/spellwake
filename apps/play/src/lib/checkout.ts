import { hostedCheckoutUrlSchema } from "@spellwake/application-contracts";

/** Only verified hosted checkout URLs cross the native opener boundary. */
export async function openNativeCheckout(
  url: string,
  expiresAt: string,
): Promise<boolean> {
  const parsed = hostedCheckoutUrlSchema.safeParse(url);
  const expiry = Date.parse(expiresAt);
  if (!parsed.success || !Number.isFinite(expiry) || expiry <= Date.now())
    return false;
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    if (expiry <= Date.now()) return false;
    await openUrl(parsed.data);
    return true;
  } catch {
    console.warn("checkout_navigation code=open_unavailable");
    return false;
  }
}
