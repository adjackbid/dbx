import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const appSource = readFileSync(new URL("../../../App.vue", import.meta.url), "utf8");
const settingsDialogSource = readFileSync(new URL("../../../components/editor/EditorSettingsDialog.vue", import.meta.url), "utf8");

describe("settings page navigation", () => {
  it("replays repeated navigation requests for the same tab", () => {
    expect(appSource).toContain("settingsNavigationRequestId.value += 1");
    expect(appSource).toContain(':navigation-request-id="settingsNavigationRequestId"');
    expect(settingsDialogSource).toContain("navigationRequestId?: number;");
    expect(settingsDialogSource).toMatch(/watch\(\s*\(\) => props\.navigationRequestId,/);
  });

  it("hides the admin-only About tab from non-admins in Web mode", () => {
    // The About tab hosts "Reset All Defaults", which is an instance-wide action.
    expect(settingsDialogSource).toContain('...(isWeb && !userStore.isAdmin ? [] : [{ value: "about" as const');
  });
});
