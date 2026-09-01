import { describe, expect, it } from "vitest";
import {
  MCP_APP_IDS,
  PROXY_APP_IDS,
  SKILLS_APP_IDS,
  isAdditiveAppId,
  isSessionAppId,
} from "@/config/appConfig";

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

  it("excludes official Claude from every writable private-fork surface", () => {
    expect(SKILLS_APP_IDS).not.toContain("claude");
    expect(MCP_APP_IDS).not.toContain("claude");
    expect(PROXY_APP_IDS).not.toContain("claude");

    expect(SKILLS_APP_IDS).toContain("claude-cometix");
    expect(MCP_APP_IDS).toContain("claude-cometix");
  });
});
