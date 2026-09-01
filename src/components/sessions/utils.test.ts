import { describe, expect, it } from "vitest";
import type { SessionMeta } from "@/types";
import { isSessionResumable } from "./utils";

const session = (providerId: string, resumeCommand?: string): SessionMeta => ({
  providerId,
  sessionId: "session-123",
  resumeCommand,
});

describe("isSessionResumable", () => {
  it("keeps official Claude history view-only", () => {
    expect(
      isSessionResumable(session("claude", "claude --resume 'session-123'")),
    ).toBe(false);
  });

  it("keeps Cometix resume available", () => {
    expect(
      isSessionResumable(
        session("claude-cometix", "hlclaude --resume 'session-123'"),
      ),
    ).toBe(true);
  });
});
