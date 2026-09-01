import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

export function SkillStorageLocationSettings() {
  const { t } = useTranslation();

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.skillStorage.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.skillStorage.description")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <Button
          type="button"
          size="sm"
          variant="default"
          disabled
          className="min-w-[96px] shadow-sm disabled:opacity-100"
        >
          {t("settings.skillStorage.ccSwitch")}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("settings.skillStorage.ccSwitchHint")}
      </p>
    </section>
  );
}
