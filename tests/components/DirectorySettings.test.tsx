import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DirectorySettings } from "@/components/settings/DirectorySettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      ({
        "settings.claudeConfigDir": "Official Claude directory",
        "settings.claudeCometixConfigDir": "Cometix directory",
      })[key] ??
      options?.defaultValue ??
      key,
  }),
}));

const resolvedDirs = {
  appConfig: "/config/app",
  claude: "/config/official-claude",
  "claude-cometix": "/config/cometix",
  codex: "/config/codex",
  gemini: "/config/gemini",
  grokbuild: "/config/grokbuild",
  opencode: "/config/opencode",
  openclaw: "/config/openclaw",
  hermes: "/config/hermes",
  pi: "/config/pi",
};

describe("DirectorySettings private-fork boundaries", () => {
  it("only exposes the isolated Cometix Claude directory", () => {
    render(
      <DirectorySettings
        resolvedDirs={resolvedDirs}
        claudeCometixDir="/custom/cometix"
        onAppConfigChange={vi.fn()}
        onBrowseAppConfig={vi.fn()}
        onResetAppConfig={vi.fn()}
        onDirectoryChange={vi.fn()}
        onBrowseDirectory={vi.fn()}
        onResetDirectory={vi.fn()}
      />,
    );

    expect(
      screen.queryByText("Official Claude directory"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByDisplayValue("/config/official-claude"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Cometix directory")).toBeInTheDocument();
    expect(screen.getByDisplayValue("/custom/cometix")).toBeInTheDocument();
  });
});
