import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { WindowSettings } from "@/components/settings/WindowSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("@/lib/platform", () => ({
  isLinux: () => false,
}));

describe("WindowSettings private fork boundary", () => {
  it("does not expose controls that modify official Claude", () => {
    const settings = {
      launchOnStartup: false,
      minimizeToTrayOnClose: true,
      enableClaudePluginIntegration: true,
      skipClaudeOnboarding: true,
    } as SettingsFormState;

    render(<WindowSettings settings={settings} onChange={vi.fn()} />);

    expect(
      screen.queryByText("settings.enableClaudePluginIntegration"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("settings.skipClaudeOnboarding"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("settings.launchOnStartup")).toBeInTheDocument();
    expect(screen.getByText("settings.minimizeToTray")).toBeInTheDocument();
  });
});
