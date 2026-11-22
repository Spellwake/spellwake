import type { PublicConfiguration } from "../src/lib/public-config";

/** Restrict development transport to the selected Vite host, including physical devices. */
export function nativeDevOrigin(host: string | undefined): string {
  const selected = host || "127.0.0.1";
  if (!/^(?:[A-Za-z0-9.-]+|\[[A-Fa-f0-9:]+\])$/.test(selected))
    throw new Error(
      "TAURI_DEV_HOST must be a hostname or address without a port.",
    );
  return new URL(`http://${selected}:6204`).origin;
}
function withConnections(csp: string, origins: string[]): string {
  const directives = csp
    .split(";")
    .map((value) => value.trim())
    .filter(Boolean);
  if (
    directives.filter((value) => value.split(/\s/)[0] === "connect-src")
      .length !== 1
  )
    throw new Error("Native CSP requires one explicit connect-src directive.");
  return directives
    .map((value) =>
      value.split(/\s+/)[0] === "connect-src"
        ? `connect-src 'self' blob: ipc: http://ipc.localhost${origins.length ? " " + [...new Set(origins)].join(" ") : ""}`
        : value,
    )
    .join("; ");
}
/** Only selected public origins enter native security configuration; the key stays out. */
export function nativeSecurityOverlay(
  csp: string,
  config: PublicConfiguration | null,
  devOrigin: string,
  hmrOrigin = devOrigin.replace(/^http:/, "ws:"),
) {
  const origins = config ? [config.authOrigin, config.apiOrigin] : [];
  const authPattern = config
    ? [...(config.authOrigin + "/auth/v1/authorize?")]
        .map((character) =>
          ["*", "?", "[", "]"].includes(character)
            ? "[" + character + "]"
            : character,
        )
        .join("") + "*"
    : "";
  return {
    build: { devUrl: devOrigin },
    app: {
      security: {
        csp: withConnections(csp, origins),
        devCsp: withConnections(csp, [...origins, devOrigin, hmrOrigin]),
        capabilities: config
          ? [
              "main",
              {
                identifier: "auth-provider-opener",
                description:
                  "Open only the selected Supabase authorization endpoint in the default browser.",
                windows: ["main"],
                permissions: [
                  {
                    identifier: "opener:allow-open-url",
                    allow: [{ url: authPattern }],
                  },
                ],
              },
            ]
          : ["main"],
      },
    },
  };
}
/** Tauri overlays precede Cargo/app arguments and override earlier build-number overlays. */
export function withNativeOverlay(args: string[], overlay: unknown): string[] {
  const separator = args.indexOf("--");
  const insert = separator < 0 ? args.length : separator;
  return [
    ...args.slice(0, insert),
    "--config",
    JSON.stringify(overlay),
    ...args.slice(insert),
  ];
}
