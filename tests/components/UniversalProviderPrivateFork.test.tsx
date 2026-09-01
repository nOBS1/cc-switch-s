import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { UniversalProviderCard } from "@/components/universal/UniversalProviderCard";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import {
  createUniversalProviderFromPreset,
  universalProviderPresets,
} from "@/config/universalProviderPresets";
import type { UniversalProvider } from "@/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@/hooks/useDarkMode", () => ({ useDarkMode: () => false }));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <span aria-hidden="true" />,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: () => <div data-testid="json-editor" />,
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    title,
    children,
    footer,
  }: {
    isOpen: boolean;
    title: React.ReactNode;
    children: React.ReactNode;
    footer: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        <h1>{title}</h1>
        {children}
        <footer>{footer}</footer>
      </div>
    ) : null,
}));

const legacyClaudeProvider: UniversalProvider = {
  id: "legacy",
  name: "Legacy gateway",
  providerType: "newapi",
  apps: { claude: true, codex: false, gemini: false },
  baseUrl: "https://legacy.example.com",
  apiKey: "legacy-key",
  models: {
    claude: { model: "legacy-claude-model" },
  },
};

describe("private-fork universal providers", () => {
  it("keeps every preset away from official Claude", () => {
    for (const preset of universalProviderPresets) {
      expect(preset.defaultApps.claude).toBe(false);
      expect(preset.defaultModels.claude).toBeUndefined();

      const provider = createUniversalProviderFromPreset(
        preset,
        "preset-provider",
        "https://api.example.com",
        "api-key",
      );
      expect(provider.apps.claude).toBe(false);
      expect(provider.models.claude).toBeUndefined();
    }
  });

  it("does not advertise a legacy official-Claude assignment on cards", () => {
    render(
      <UniversalProviderCard
        provider={legacyClaudeProvider}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onSync={vi.fn()}
        onDuplicate={vi.fn()}
      />,
    );

    expect(screen.queryByText("Claude")).not.toBeInTheDocument();
    expect(screen.getByText("未启用任何应用")).toBeInTheDocument();
  });

  it("hides official Claude and sanitizes it from an edited provider", async () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    render(
      <UniversalProviderFormModal
        isOpen
        onClose={onClose}
        onSave={onSave}
        editingProvider={legacyClaudeProvider}
      />,
    );

    expect(screen.queryByText("Claude Code")).not.toBeInTheDocument();
    expect(
      screen.queryByPlaceholderText("claude-sonnet-4-20250514"),
    ).not.toBeInTheDocument();

    const saveButton = screen.getByRole("button", { name: "添加" });
    await waitFor(() => expect(saveButton).toBeEnabled());
    fireEvent.click(saveButton);

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    const [saved] = onSave.mock.calls[0] as [UniversalProvider];
    expect(saved.apps).toEqual({
      claude: false,
      codex: false,
      gemini: false,
    });
    expect(saved.models.claude).toBeUndefined();
    expect(saved.apps).not.toHaveProperty("claude-cometix");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
