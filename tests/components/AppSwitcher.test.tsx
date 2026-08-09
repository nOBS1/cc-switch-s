import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        "apps.claudeCode": "Claude Code",
        "apps.claudeCometix": "Claude Code（Cometix）",
      })[key] ?? key,
  }),
}));

describe("AppSwitcher", () => {
  it("shows Claude Code Cometix as an additive top-level app", () => {
    render(<AppSwitcher activeApp="claude" onSwitch={vi.fn()} />);

    expect(
      screen.getByRole("button", { name: "Claude Code（Cometix）" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Claude Code" }),
    ).toBeInTheDocument();
  });
});
