import { describe, expect, it } from "vitest";
import { filterManagementApps, isManagementApp } from "./forkPolicy";

describe("private fork app-management policy", () => {
  it("never exposes Claude Desktop as a manageable app", () => {
    expect(isManagementApp("claude-desktop")).toBe(false);
    expect(
      filterManagementApps(["claude", "claude-cometix", "claude-desktop"]),
    ).toEqual(["claude", "claude-cometix"]);
  });

  it("keeps the official and Cometix Claude Code entries independent", () => {
    expect(isManagementApp("claude")).toBe(true);
    expect(isManagementApp("claude-cometix")).toBe(true);
  });
});
