import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        "apps.claudeCode": "Claude Code",
        "apps.claudeCometix": "Claude Code（Cometix）",
        "apps.claudeDesktop": "Claude Desktop",
      })[key] ?? key,
  }),
}));

describe("AppSwitcher", () => {
  it("only exposes the isolated Cometix Claude entry in the private fork", () => {
    render(<AppSwitcher activeApp="claude" onSwitch={vi.fn()} />);

    expect(
      screen.getByRole("button", { name: "Claude Code（Cometix）" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Claude Code" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Claude Desktop" }),
    ).not.toBeInTheDocument();
  });
});
