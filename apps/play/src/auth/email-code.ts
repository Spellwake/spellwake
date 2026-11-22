import type { ApplicationServices } from "./services";

/**
 * Email one-time codes through the Auth SDK. The email and code stay in the
 * caller's memory; nothing here logs or stores either.
 */
type Auth = Pick<ApplicationServices["auth"], "signInWithOtp" | "verifyOtp">;

/** Asks the Auth service to email a six-digit code. New and returning people use the same call. */
export async function sendEmailCode(
  services: { auth: Auth },
  email: string,
): Promise<"sent" | "failed"> {
  try {
    const result = await services.auth.signInWithOtp({
      email,
      options: { shouldCreateUser: true },
    });
    if (!result.error) return "sent";
    console.warn("account_auth code=email_send_rejected");
    return "failed";
  } catch {
    console.warn("account_auth code=email_send_unavailable");
    return "failed";
  }
}

/**
 * Checks a code. "rejected" is the Auth service saying the code is wrong or
 * expired. "unavailable" is anything else, where the same code may be tried again.
 */
export async function verifyEmailCode(
  services: { auth: Auth },
  email: string,
  code: string,
): Promise<"verified" | "rejected" | "unavailable"> {
  try {
    const result = await services.auth.verifyOtp({
      email,
      token: code,
      type: "email",
    });
    if (!result.error) return result.data.session ? "verified" : "unavailable";
    const status = result.error.status;
    // Too many attempts is a wait, not a wrong code.
    return status !== undefined &&
      status >= 400 &&
      status < 500 &&
      status !== 429
      ? "rejected"
      : "unavailable";
  } catch {
    console.warn("account_auth code=email_verify_unavailable");
    return "unavailable";
  }
}
