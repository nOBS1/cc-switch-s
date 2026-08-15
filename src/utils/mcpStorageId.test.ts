import { describe, expect, it } from "vitest";
import { getMcpLiveId } from "./mcpStorageId";

describe("getMcpLiveId", () => {
  it("hides a Cometix scoped database id from the UI", () => {
    expect(getMcpLiveId("cc-switch-scope:v1:claude-cometix:c2hhcmVk")).toBe(
      "shared",
    );
  });

  it("leaves ordinary MCP ids unchanged", () => {
    expect(getMcpLiveId("context7")).toBe("context7");
  });
});
