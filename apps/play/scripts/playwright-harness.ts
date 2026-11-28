/**
 * Shared Playwright E2E harness for Spellwake domain browser qualification scripts.
 *
 * Manages an isolated loopback Vite dev server for `apps/play`, cross-platform
 * headless Chromium/Edge launch, stand-in Auth session seeding, CORS routing,
 * structured pass/fail reporting, and automatic temp-directory cleanup.
 */
import {
  chromium,
  type Browser,
  type BrowserContext,
  type Page,
  type Route,
} from "@playwright/test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer, type ViteDevServer } from "vite";

export const CORS_HEADERS: Record<string, string> = {
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "authorization, content-type",
  "access-control-allow-methods": "GET, POST, OPTIONS",
};

export type ApiReply = {
  code: number;
  body: unknown;
};

export type CheckResult = {
  name: string;
  pass: boolean;
  detail?: string;
};

export const makeUuid = (n: number): string =>
  `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;

export const apiSuccess = (data: unknown): ApiReply => ({
  code: 200,
  body: { status: "success", data },
});

export const apiAccepted = (data: unknown): ApiReply => ({
  code: 202,
  body: { status: "success", data },
});

export const apiFail = (
  data: Record<string, string>,
  code = 409,
): ApiReply => ({
  code,
  body: { status: "fail", data },
});

export const apiError = (message: string, code = 500): ApiReply => ({
  code,
  body: { status: "error", message },
});

export function standInSession(token: string, subject: string) {
  return {
    access_token: token,
    refresh_token: `${token}-refresh`,
    token_type: "bearer",
    expires_in: 86400,
    expires_at: Math.floor(Date.now() / 1000) + 86400,
    user: {
      id: subject,
      aud: "authenticated",
      role: "authenticated",
      app_metadata: {},
      user_metadata: {},
      created_at: "2026-01-01T00:00:00Z",
    },
  };
}

export function assertCondition(
  condition: unknown,
  message: string,
): asserts condition {
  if (!condition) throw new Error(message);
}

export async function launchBrowser(): Promise<Browser> {
  const linuxWebGlArgs =
    process.platform === "linux"
      ? ["--enable-webgl", "--ignore-gpu-blocklist"]
      : [];
  const explicitChannel = process.env["PLAYWRIGHT_CHANNEL"];
  if (explicitChannel) {
    return chromium.launch({
      channel: explicitChannel,
      headless: true,
      args: linuxWebGlArgs,
    });
  }
  const attempts: Array<{ channel?: string; headless: boolean; args: string[] }> = [
    { channel: "chromium", headless: true, args: linuxWebGlArgs },
    { headless: true, args: linuxWebGlArgs },
    ...(process.platform === "win32"
      ? [
          { channel: "chrome", headless: true, args: linuxWebGlArgs },
          { channel: "msedge", headless: true, args: linuxWebGlArgs },
        ]
      : []),
  ];
  let lastError: unknown;
  for (const options of attempts) {
    try {
      return await chromium.launch(options);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}

export type PlayE2eRuntime = {
  origin: string;
  authOrigin: string;
  apiOrigin: string;
  browser: Browser;
  checks: CheckResult[];
  runCheck: (name: string, fn: () => Promise<void>) => Promise<void>;
  withAuthedPage: (
    options: {
      token: string;
      subject: string;
      viewport?: { width: number; height: number } | undefined;
      answer: (
        method: string,
        url: URL,
        body: unknown,
        bearer: string,
      ) => ApiReply;
      extraRoutes?: ((context: BrowserContext) => Promise<void>) | undefined;
    },
    fn: (harness: {
      page: Page;
      context: BrowserContext;
      sent: (path: string) => Record<string, unknown>[];
    }) => Promise<void>,
  ) => Promise<void>;
  withGuestPage: (
    options: {
      viewport?: { width: number; height: number } | undefined;
      answer: (
        method: string,
        url: URL,
        body: unknown,
        bearer: string,
      ) => ApiReply;
      serveAuth: (route: Route) => Promise<void>;
      seedSession?: { token: string; subject: string } | undefined;
      extraRoutes?: ((context: BrowserContext) => Promise<void>) | undefined;
    },
    fn: (harness: {
      page: Page;
      context: BrowserContext;
      sent: (path: string) => Record<string, unknown>[];
    }) => Promise<void>,
  ) => Promise<void>;
  finish: () => void;
};

export async function withPlayE2eRuntime(
  suiteName: string,
  ports: { authPort: number; apiPort: number },
  run: (runtime: PlayE2eRuntime) => Promise<void>,
): Promise<void> {
  const authOrigin = `http://127.0.0.1:${ports.authPort}`;
  const apiOrigin = `http://127.0.0.1:${ports.apiPort}`;
  const tempDir = await mkdtemp(join(tmpdir(), `spellwake-qualify-${suiteName}-`));
  const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

  process.env["VITE_SUPABASE_URL"] = authOrigin;
  process.env["VITE_APPLICATION_API_URL"] = apiOrigin;
  process.env["VITE_SUPABASE_PUBLISHABLE_KEY"] = "sb_publishable_local_stand_in";

  const server: ViteDevServer = await createServer({
    root: appRoot,
    cacheDir: join(tempDir, "vite-cache"),
    server: { port: 0, strictPort: false, host: "127.0.0.1" },
    logLevel: "error",
  });

  let browser: Browser | undefined;
  try {
    await server.listen();
    const address = server.httpServer?.address();
    if (!address || typeof address === "string") {
      throw new Error("Could not resolve Vite server port.");
    }
    const origin = `http://127.0.0.1:${address.port}/play`;
    browser = await launchBrowser();

    const checks: CheckResult[] = [];
    const runCheck = async (
      name: string,
      fn: () => Promise<void>,
    ): Promise<void> => {
      try {
        await fn();
        checks.push({ name, pass: true });
        console.log(`(pass) ${name}`);
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        checks.push({ name, pass: false, detail });
        console.error(`(fail) ${name}: ${detail}`);
      }
    };

    const withAuthedPage: PlayE2eRuntime["withAuthedPage"] = async (
      options,
      fn,
    ) => {
      const received: { path: string; body: unknown }[] = [];
      const serveApi = async (route: Route): Promise<void> => {
        const req = route.request();
        if (req.method() === "OPTIONS") {
          await route.fulfill({ status: 204, headers: CORS_HEADERS });
          return;
        }
        const url = new URL(req.url());
        const body: unknown = req.postDataJSON();
        if (req.method() === "POST") {
          received.push({ path: url.pathname, body });
        }
        const bearer = (req.headers()["authorization"] ?? "").replace(
          /^Bearer /,
          "",
        );
        const reply = options.answer(req.method(), url, body, bearer);
        await route.fulfill({
          status: reply.code,
          headers: CORS_HEADERS,
          contentType: "application/json",
          body: JSON.stringify(reply.body),
        });
      };

      const serveAuth = async (route: Route): Promise<void> => {
        const req = route.request();
        if (req.method() === "OPTIONS") {
          await route.fulfill({ status: 204, headers: CORS_HEADERS });
          return;
        }
        const url = new URL(req.url());
        if (url.pathname === "/auth/v1/token") {
          await route.fulfill({
            status: 200,
            headers: CORS_HEADERS,
            contentType: "application/json",
            body: JSON.stringify(standInSession(options.token, options.subject)),
          });
          return;
        }
        await route.fulfill({
          status: 404,
          headers: CORS_HEADERS,
          contentType: "application/json",
          body: JSON.stringify({ msg: "No such route." }),
        });
      };

      const context = await browser!.newContext({
        viewport: options.viewport ?? { width: 1440, height: 900 },
        reducedMotion: "reduce",
      });
      try {
        await context.route(`${apiOrigin}/**`, serveApi);
        await context.route(`${authOrigin}/**`, serveAuth);
        await context.route("**/*.glb*", (route) => route.abort());
        if (options.extraRoutes) {
          await options.extraRoutes(context);
        }
        await context.addInitScript(
          ([key, session]) => {
            if (window.sessionStorage.getItem("stand-in-seeded")) return;
            window.sessionStorage.setItem("stand-in-seeded", "1");
            window.localStorage.setItem(key, session);
          },
          [
            `spellwake.auth.v1:${new URL(authOrigin).host}`,
            JSON.stringify(standInSession(options.token, options.subject)),
          ] as const,
        );
        const page = await context.newPage();
        page.on("pageerror", (err) => {
          console.error(`[pageerror] ${err.message}`);
        });
        page.setDefaultTimeout(20_000);
        page.setDefaultNavigationTimeout(45_000);
        await fn({
          page,
          context,
          sent: (p: string) =>
            received
              .filter((r) => r.path === p)
              .map((r) => r.body as Record<string, unknown>),
        });
      } finally {
        await context.close();
      }
    };

    const withGuestPage: PlayE2eRuntime["withGuestPage"] = async (
      options,
      fn,
    ) => {
      const received: { path: string; body: unknown }[] = [];
      const serveApi = async (route: Route): Promise<void> => {
        const req = route.request();
        if (req.method() === "OPTIONS") {
          await route.fulfill({ status: 204, headers: CORS_HEADERS });
          return;
        }
        const url = new URL(req.url());
        const body: unknown = req.postDataJSON();
        if (req.method() === "POST") {
          received.push({ path: url.pathname, body });
        }
        const bearer = (req.headers()["authorization"] ?? "").replace(
          /^Bearer /,
          "",
        );
        const reply = options.answer(req.method(), url, body, bearer);
        await route.fulfill({
          status: reply.code,
          headers: CORS_HEADERS,
          contentType: "application/json",
          body: JSON.stringify(reply.body),
        });
      };

      const context = await browser!.newContext({
        viewport: options.viewport ?? { width: 1440, height: 900 },
        reducedMotion: "reduce",
      });
      try {
        await context.route(`${apiOrigin}/**`, serveApi);
        await context.route(`${authOrigin}/**`, options.serveAuth);
        await context.route("**/*.glb*", (route) => route.abort());
        if (options.extraRoutes) {
          await options.extraRoutes(context);
        }
        if (options.seedSession) {
          await context.addInitScript(
            ([key, session]) => {
              if (window.sessionStorage.getItem("stand-in-seeded")) return;
              window.sessionStorage.setItem("stand-in-seeded", "1");
              window.localStorage.setItem(key, session);
            },
            [
              `spellwake.auth.v1:${new URL(authOrigin).host}`,
              JSON.stringify(
                standInSession(
                  options.seedSession.token,
                  options.seedSession.subject,
                ),
              ),
            ] as const,
          );
        }
        const page = await context.newPage();
        page.on("pageerror", (err) => {
          console.error(`[pageerror] ${err.message}`);
        });
        page.setDefaultTimeout(20_000);
        page.setDefaultNavigationTimeout(45_000);
        await fn({
          page,
          context,
          sent: (p: string) =>
            received
              .filter((r) => r.path === p)
              .map((r) => r.body as Record<string, unknown>),
        });
      } finally {
        await context.close();
      }
    };

    const finish = (): void => {
      const passedCount = checks.filter((c) => c.pass).length;
      const failedCount = checks.length - passedCount;
      console.log(`\n${passedCount} pass\n${failedCount} fail`);
      console.log(
        JSON.stringify(
          {
            suite: suiteName,
            passed: failedCount === 0,
            total: checks.length,
            passedCount,
            failedCount,
            checks,
          },
          null,
          2,
        ),
      );
      if (failedCount > 0) {
        process.exitCode = 1;
      }
    };

    await run({
      origin,
      authOrigin,
      apiOrigin,
      browser,
      checks,
      runCheck,
      withAuthedPage,
      withGuestPage,
      finish,
    });
  } finally {
    await browser?.close();
    await server.close();
    await rm(tempDir, { recursive: true, force: true });
  }
}
