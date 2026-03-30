// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

// src/components/ide/dialogs/IdeAgentOptInDialog.tsx
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Bot, FolderSync } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from '../../ui/dialog';
import { Checkbox } from '../../ui/checkbox';

interface IdeAgentOptInDialogProps {
  open: boolean;
  onEnable: (remember: boolean) => void;
  onSftpOnly: (remember: boolean) => void;
}

export function IdeAgentOptInDialog({
  open,
  onEnable,
  onSftpOnly,
}: IdeAgentOptInDialogProps) {
  const { t } = useTranslation();
  const [remember, setRemember] = useState(false);

  return (
    <Dialog open={open} onOpenChange={() => { /* prevent dismiss */ }}>
      <DialogContent
        className="max-w-sm p-0 overflow-hidden rounded-lg border border-theme-border/60 shadow-2xl shadow-black/40"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogTitle className="sr-only">
          {t('ide.agent_optin_title')}
        </DialogTitle>
        {/* Body */}
        <div className="flex flex-col items-center gap-3 px-6 pt-6 pb-4">
          {/* Icon */}
          <div className="flex items-center justify-center w-12 h-12 rounded-full bg-theme-accent/10 ring-1 ring-theme-accent/20">
            <Bot className="w-6 h-6 text-theme-accent" />
          </div>

          {/* Title */}
          <h3 className="text-sm font-semibold text-theme-text text-center leading-snug">
            {t('ide.agent_optin_title')}
          </h3>

          {/* Description */}
          <p className="text-xs text-theme-text-muted text-center leading-relaxed">
            {t('ide.agent_optin_desc')}
          </p>

          {/* Benefits */}
          <div className="w-full space-y-1.5 text-xs text-theme-text-muted">
            <div className="flex items-start gap-2">
              <span className="text-emerald-400 mt-0.5">✓</span>
              <span>{t('ide.agent_optin_benefit_watch')}</span>
            </div>
            <div className="flex items-start gap-2">
              <span className="text-emerald-400 mt-0.5">✓</span>
              <span>{t('ide.agent_optin_benefit_git')}</span>
            </div>
            <div className="flex items-start gap-2">
              <span className="text-emerald-400 mt-0.5">✓</span>
              <span>{t('ide.agent_optin_benefit_atomic')}</span>
            </div>
          </div>

          {/* Remember checkbox */}
          <label className="flex items-center gap-2 mt-1 cursor-pointer select-none">
            <Checkbox
              checked={remember}
              onCheckedChange={(v) => setRemember(v === true)}
            />
            <span className="text-xs text-theme-text-muted">
              {t('ide.agent_optin_remember')}
            </span>
          </label>
        </div>

        {/* Actions — split buttons matching useConfirm style */}
        <div className="flex border-t border-theme-border/40">
          <button
            onClick={() => onSftpOnly(remember)}
            className="flex-1 flex items-center justify-center gap-1.5 py-2.5 text-sm font-medium text-theme-text-muted hover:text-theme-text hover:bg-theme-bg-hover transition-colors border-r border-theme-border/40"
          >
            <FolderSync className="w-3.5 h-3.5" />
            {t('ide.agent_optin_sftp_only')}
          </button>
          <button
            onClick={() => onEnable(remember)}
            className="flex-1 flex items-center justify-center gap-1.5 py-2.5 text-sm font-semibold text-theme-accent hover:text-theme-accent/80 hover:bg-theme-accent/10 transition-colors"
          >
            <Bot className="w-3.5 h-3.5" />
            {t('ide.agent_optin_enable')}
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
