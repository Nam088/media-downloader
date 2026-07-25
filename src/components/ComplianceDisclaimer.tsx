import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

const STORAGE_KEY = "compliance-disclaimer-acknowledged";

export function ComplianceDisclaimer() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(() => localStorage.getItem(STORAGE_KEY) !== "true");

  const acknowledge = () => {
    localStorage.setItem(STORAGE_KEY, "true");
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !next && acknowledge()}>
      <DialogContent
        onEscapeKeyDown={(e) => e.preventDefault()}
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{t("compliance.title")}</DialogTitle>
          <DialogDescription>{t("compliance.body")}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button onClick={acknowledge}>{t("compliance.acknowledge")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
