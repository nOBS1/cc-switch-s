import { beforeEach, describe, expect, it, vi } from "vitest";
import { checkAllEnvConflicts } from "@/lib/api/env";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe("checkAllEnvConflicts", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(
      (_command: string, { app }: { app: string }) =>
        Promise.resolve([
          {
            varName: `${app.toUpperCase()}_API_KEY`,
            varValue: `${app}-value`,
            sourceType: "system",
            sourcePath: "test-environment",
          },
        ]),
    );
  });

  it("继续扫描 Codex、Gemini 和 GrokBuild，但不读取官方 Claude 环境变量", async () => {
    const result = await checkAllEnvConflicts();

    expect(invokeMock).toHaveBeenCalledTimes(3);
    expect(invokeMock).not.toHaveBeenCalledWith("check_env_conflicts", {
      app: "claude",
    });
    expect(invokeMock).toHaveBeenCalledWith("check_env_conflicts", {
      app: "codex",
    });
    expect(invokeMock).toHaveBeenCalledWith("check_env_conflicts", {
      app: "gemini",
    });
    expect(invokeMock).toHaveBeenCalledWith("check_env_conflicts", {
      app: "grokbuild",
    });
    expect(Object.keys(result).sort()).toEqual([
      "codex",
      "gemini",
      "grokbuild",
    ]);
    expect(result.codex).toEqual([
      expect.objectContaining({
        varName: "CODEX_API_KEY",
        varValue: "codex-value",
      }),
    ]);
  });
});
