import { usernameSchema } from "@spellwake/application-contracts";

/**
 * The rules of account entry that need no network: what counts as an email or
 * a username, and how each failure is worded. The wording is the approved copy.
 */
export const RESEND_WAIT_SECONDS = 15;
export const CODE_LENGTH = 6;

/** Why an email cannot be sent a code, or null when it can. */
export function emailProblem(raw: string): string | null {
  const email = raw.trim();
  return email.length > 0 &&
    email.length <= 254 &&
    /^[^\s@]+@[^\s@.]+(\.[^\s@.]+)+$/.test(email)
    ? null
    : "Enter a valid email address.";
}

/** Why a username cannot be registered, or null when its shape is allowed. */
export function usernameProblem(raw: string): string | null {
  return usernameSchema.safeParse(raw).success
    ? null
    : "Use 3–24 letters, numbers or underscores.";
}

/**
 * How the server's answer to a registration reads on the username field.
 * Anything it does not explain is a passive notice, not a field error.
 */
export function registrationProblem(
  errors: Readonly<Record<string, string>>,
): { field: string } | { notice: string } {
  if (errors["username"] === "unavailable")
    return { field: "That username is taken." };
  if (errors["username"] === "invalid")
    return { field: "Use 3–24 letters, numbers or underscores." };
  return { notice: "Couldn’t create account." };
}
