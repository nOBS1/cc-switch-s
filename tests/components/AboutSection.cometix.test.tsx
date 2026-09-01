import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getToolVersionsMock } = vi.hoisted(() => ({
  getToolVersionsMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.19.2"),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: false,
    updateInfo: null,
    checkUpdate: vi.fn(),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getToolVersions: getToolVersionsMock,
  },
}));

vi.mock("@/lib/platform", () => ({
  isWindows: () => true,
}));

import { AboutSection } from "@/components/settings/AboutSection";

describe("AboutSection Cometix version source", () => {
  beforeEach(() => {
    getToolVersionsMock.mockClear();
    getToolVersionsMock.mockImplementation(async (tools: string[]) =>
      tools.map((name) => ({
        name,
        version:
          name === "claude"
            ? "official-claude-version"
            : name === "claude-cometix"
              ? "2.1.219"
              : null,
        latest_version: name === "claude-cometix" ? "2.1.220" : null,
        error: null,
        installed_but_broken: false,
        env_type: "windows",
        wsl_distro: null,
      })),
    );
  });

  it("does not probe or render the official Claude installation", async () => {
    render(<AboutSection isPortable={false} />);

    await screen.findByText("settings.cometixVersionSource");

    const requestedTools = getToolVersionsMock.mock.calls.flatMap(
      ([tools]) => tools as string[],
    );
    expect(requestedTools).not.toContain("claude");
    expect(requestedTools).toContain("claude-cometix");
    expect(
      screen.queryByText("official-claude-version"),
    ).not.toBeInTheDocument();
  });

  it("shows that the Cometix latest version comes from GitHub Releases", async () => {
    render(<AboutSection isPortable={false} />);

    expect(
      await screen.findByText("settings.cometixVersionSource"),
    ).toBeInTheDocument();
    expect(screen.getByText("2.1.220")).toBeInTheDocument();
  });

  it("shows an isolated hlclaude command in manual install instructions", async () => {
    render(<AboutSection isPortable={false} />);

    await screen.findByText("settings.cometixVersionSource");
    fireEvent.click(
      screen.getByRole("button", { name: "settings.manualInstallCommands" }),
    );

    const commands = screen.getByText(/@cometix\/claude-code@latest/);
    expect(commands).toHaveTextContent(".local\\share\\hlclaude");
    expect(commands).toHaveTextContent("hlclaude.cmd");
    expect(commands).toHaveTextContent("Test-Path $hlclaudeLauncher");
    expect(commands).not.toHaveTextContent("@anthropic-ai/claude-code");
  });
});
