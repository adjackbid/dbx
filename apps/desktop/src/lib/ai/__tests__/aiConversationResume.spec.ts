import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const assistantSource = readFileSync(new URL("../../../components/editor/AiAssistant.vue", import.meta.url), "utf8");

describe("AI conversation resume", () => {
  it("loads the account's conversations on mount and resumes the matching one", () => {
    // Conversations are stored per account on the server; the list is loaded on
    // mount so a reload or a new sign-in can resume the last conversation.
    expect(assistantSource).toContain("conversations.value = await loadAiConversations().catch(() => []);");
    expect(assistantSource).toContain("conversationsLoaded = true;");
    expect(assistantSource).toContain("restoreLastConversation();");
    expect(assistantSource).toMatch(/const previous = conversations\.value\.find\(\(conv\) => conv\.connectionName === connectionName\)/);
  });

  it("resumes at most once so an explicit new chat is not overwritten", () => {
    expect(assistantSource).toMatch(/function restoreLastConversation\(\) \{\s*if \(conversationRestored \|\| !conversationsLoaded\) return;/);
    expect(assistantSource).toMatch(/function startNewChat\(\) \{\s*clearMessages\(\);\s*conversationRestored = true;/);
  });

  it("retries the resume when the connection resolves after mount", () => {
    expect(assistantSource).toMatch(/watch\(\s*\(\) => props\.connection\?\.name,\s*\(\) => restoreLastConversation\(\),\s*\);/);
  });
});
