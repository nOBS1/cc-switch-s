import React from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { useTranslation } from "react-i18next";

interface AppToggleGroupProps {
  apps: Partial<Record<AppId, boolean>>;
  onToggle: (app: AppId, enabled: boolean) => void;
  appIds?: AppId[];
  disabled?: boolean;
}

export const AppToggleGroup: React.FC<AppToggleGroupProps> = ({
  apps,
  onToggle,
  appIds = APP_IDS,
  disabled = false,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-1.5 flex-shrink-0">
      {appIds.map((app) => {
        const { labelKey, icon, activeClass } = APP_ICON_MAP[app];
        const label = t(labelKey);
        const enabled = apps[app];
        return (
          <Tooltip key={app}>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => onToggle(app, !enabled)}
                disabled={disabled}
                aria-label={label}
                aria-pressed={Boolean(enabled)}
                className={`w-7 h-7 rounded-lg flex items-center justify-center transition-all ${
                  enabled ? activeClass : "opacity-35 hover:opacity-70"
                } disabled:cursor-not-allowed`}
              >
                {icon}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              <p>
                {label}
                {enabled ? " ✓" : ""}
              </p>
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
};
