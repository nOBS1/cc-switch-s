import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";

describe("ProviderEmptyState", () => {
  it("labels the Cometix import as a copy from official Claude Code", () => {
    const onImport = vi.fn();

    render(
      <ProviderEmptyState
        appId="claude-cometix"
        onCreate={vi.fn()}
        onImport={onImport}
      />,
    );

    expect(
      screen.getByText("provider.noProvidersDescriptionCometix"),
    ).toBeInTheDocument();
    const importButton = screen.getByRole("button", {
      name: "provider.importOfficialClaude",
    });
    fireEvent.click(importButton);
    expect(onImport).toHaveBeenCalledTimes(1);
  });
});
