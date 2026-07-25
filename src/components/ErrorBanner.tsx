import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { AppError } from "@/types/download";

interface ErrorBannerProps {
  error: AppError | null;
  onDismiss: () => void;
}

/**
 * Maps `AppError.code` to a localized string (`errors.<CODE>` in the locale
 * files); falls back to the backend's English `message` when a code has no
 * translation yet, so unmapped/new error codes never show up blank.
 */
export function ErrorBanner({ error, onDismiss }: ErrorBannerProps) {
  const { t } = useTranslation();
  if (!error) return null;

  const text = t(`errors.${error.code}`, { defaultValue: error.message });

  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      <span>{text}</span>
      <Button variant="ghost" size="icon" className="h-6 w-6 shrink-0" onClick={onDismiss}>
        <X className="h-4 w-4" />
      </Button>
    </div>
  );
}
