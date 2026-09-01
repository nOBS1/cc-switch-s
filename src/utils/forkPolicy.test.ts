import { describe, expect, it } from "vitest";
import { filterManagementApps, isManagementApp } from "./forkPolicy";

describe("private fork app-management policy", () => {
  it("never exposes official Claude surfaces as manageable apps", () => {
    expect(isManagementApp("claude")).toBe(false);
    expect(isManagementApp("claude-desktop")).toBe(false);
    expect(
      filterManagementApps(["claude", "claude-cometix", "claude-desktop"]),
    ).toEqual(["claude-cometix"]);
  });

  it("keeps the isolated Cometix Claude Code entry manageable", () => {
    expect(isManagementApp("claude-cometix")).toBe(true);
  });
});
