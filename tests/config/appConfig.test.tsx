import { describe, expect, it } from "vitest";
import { isAdditiveAppId, isSessionAppId } from "@/config/appConfig";

describe("appConfig provider lifecycle", () => {
  it.each(["opencode", "openclaw", "hermes", "pi"])(
    "classifies %s as additive",
    (appId) => {
      expect(isAdditiveAppId(appId)).toBe(true);
    },
  );

  it.each([
    "claude",
    "claude-cometix",
    "claude-desktop",
    "codex",
    "gemini",
    "grokbuild",
  ])("does not classify %s as additive", (appId) => {
    expect(isAdditiveAppId(appId)).toBe(false);
  });

  it("supports independent Cometix history without enabling Claude Desktop", () => {
    expect(isSessionAppId("claude")).toBe(true);
    expect(isSessionAppId("claude-cometix")).toBe(true);
    expect(isSessionAppId("claude-desktop")).toBe(false);
  });
});
