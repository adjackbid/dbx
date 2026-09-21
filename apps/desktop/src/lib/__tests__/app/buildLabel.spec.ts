// @vitest-environment happy-dom

import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { afterEach, describe, expect, it, vi } from "vitest";
import LoginPage from "@/components/auth/LoginPage.vue";
import { formatBuildLabel } from "@/lib/app/buildLabel";

const mountedApps: Array<ReturnType<typeof createApp>> = [];

function stubAuthCheck(payload: unknown, ok = true) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({ ok, json: async () => payload }) as unknown as Response),
  );
}

async function mountLoginPage() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const app = createApp(LoginPage);
  app.use(createPinia());
  app.use(
    createI18n({
      legacy: false,
      locale: "en",
      messages: {
        en: {
          common: { showPassword: "Show password" },
          auth: {
            username: "Username",
            password: "Password",
            enterPassword: "Enter password",
            loginDescription: "Sign in to continue",
            login: "Sign in",
            loginFailed: "Sign in failed",
          },
        },
      },
    }),
  );
  app.mount(host);
  mountedApps.push(app);
  await nextTick();
  await vi.waitFor(() => expect(fetch).toHaveBeenCalled());
  await nextTick();
  return host;
}

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("formatBuildLabel", () => {
  it("shows only the version when the build has no extra identity", () => {
    expect(formatBuildLabel({ version: "0.5.82" })).toBe("v0.5.82");
  });

  it("adds the revision and the UTC build time", () => {
    const buildTimeMs = String(Date.UTC(2025, 0, 2, 3, 4, 5));
    expect(formatBuildLabel({ version: "0.5.82", commit: "abc123def456", buildTimeMs })).toBe("v0.5.82 · abc123def456 · 2025-01-02 03:04");
  });

  it("drops unknown revisions and unusable timestamps", () => {
    expect(formatBuildLabel({ version: "0.5.82", commit: "unknown", buildTimeMs: "0" })).toBe("v0.5.82");
    expect(formatBuildLabel({ version: "0.5.82", commit: "  ", buildTimeMs: "not-a-number" })).toBe("v0.5.82");
  });

  it("renders nothing without a version", () => {
    expect(formatBuildLabel(null)).toBe("");
    expect(formatBuildLabel({ commit: "abc123" })).toBe("");
  });
});

describe("LoginPage build label", () => {
  it("surfaces the version, revision and build time reported by /api/auth/check", async () => {
    stubAuthCheck({ required: true, version: "0.5.82", commit: "abc123def456", buildTimeMs: String(Date.UTC(2025, 0, 2, 3, 4, 5)) });
    const host = await mountLoginPage();
    expect(host.querySelector('[data-testid="login-build-label"]')?.textContent?.trim()).toBe("v0.5.82 · abc123def456 · 2025-01-02 03:04");
  });

  it("hides the label when the server reports no version", async () => {
    stubAuthCheck({ required: true });
    const host = await mountLoginPage();
    expect(host.querySelector('[data-testid="login-build-label"]')).toBeNull();
  });

  it("keeps the form usable when the version probe fails", async () => {
    stubAuthCheck({ required: true, version: "0.5.82" }, false);
    const host = await mountLoginPage();
    expect(host.querySelector('[data-testid="login-build-label"]')).toBeNull();
    expect(host.querySelector("form")).not.toBeNull();
  });
});
