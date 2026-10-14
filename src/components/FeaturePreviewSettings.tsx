import { openUrl } from "@tauri-apps/plugin-opener";
import { PREVIEW_FEATURES } from "../lib/features";
import { WEB_URL } from "../lib/supabase";
import { useFeaturePreviewStore } from "../stores/featurePreviewStore";
import { Label } from "./ui/label";
import { Switch } from "./ui/switch";

export function FeaturePreviewSettings() {
  const flags = useFeaturePreviewStore((s) => s.flags);
  const setPreviewFeature = useFeaturePreviewStore((s) => s.setPreviewFeature);

  return (
    <div className="space-y-6" data-testid="feature-preview-settings">
      <div>
        <h2 className="text-base font-semibold">Feature Preview</h2>
        <p className="text-sm text-muted-foreground mt-1">
          Turn on experimental features. Off hides them in the UI and blocks the
          matching backend commands. Dev builds start with every preview flag
          on. Release builds use the shipped default until you change a switch.
        </p>
      </div>
      <ul className="space-y-4">
        {PREVIEW_FEATURES.map((feature) => {
          const docsUrl = `${WEB_URL}${feature.docsPath}`;
          return (
            <li
              key={feature.id}
              className="flex items-center justify-between gap-4 rounded-md border border-border p-4"
            >
              <div className="min-w-0">
                <button
                  type="button"
                  className="text-sm font-medium text-primary underline-offset-2 hover:underline"
                  onClick={() => void openUrl(docsUrl)}
                >
                  {feature.title}
                </button>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <Label htmlFor={`preview-${feature.id}`} className="sr-only">
                  {feature.title}
                </Label>
                <Switch
                  id={`preview-${feature.id}`}
                  aria-label={feature.title}
                  checked={flags[feature.id]}
                  onCheckedChange={(checked) =>
                    void setPreviewFeature(feature.id, checked)
                  }
                />
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
