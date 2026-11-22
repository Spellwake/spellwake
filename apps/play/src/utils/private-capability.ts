/** Encode capability bytes using canonical, unpadded base64url. */
export function base64url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

/** Create a private 256-bit capability using the host's cryptographic source. */
export function newPrivateCapability(): string {
  return base64url(crypto.getRandomValues(new Uint8Array(32)));
}
