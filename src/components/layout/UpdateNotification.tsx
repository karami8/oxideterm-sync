// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

import { useTranslation } from 'react-i18next';
import { Download, RefreshCw, X, ArrowUpCircle } from 'lucide-react';
import { useUpdateStore, type UpdateStage } from '@/store/updateStore';
import { useAppStore } from '@/store/appStore';
import { Button } from '../ui/button';

/** Stages where the notification banner should be visible */
const VISIBLE_STAGES = new Set<UpdateStage>([
  'available',
  'downloading',
  'verifying',
  'installing',
  'ready',
]);

export const UpdateNotification = () => {
  const { t } = useTranslation();
  const stage = useUpdateStore((s) => s.stage);
  const newVersion = useUpdateStore((s) => s.newVersion);
  const downloadedBytes = useUpdateStore((s) => s.downloadedBytes);
  const totalBytes = useUpdateStore((s) => s.totalBytes);
  const dismiss = useUpdateStore((s) => s.dismiss);
  const skipVersion = useUpdateStore((s) => s.skipVersion);
  const startDownload = useUpdateStore((s) => s.startDownload);
  const restartApp = useUpdateStore((s) => s.restartApp);
  const createTab = useAppStore((s) => s.createTab);

  if (!VISIBLE_STAGES.has(stage) || !newVersion) return null;

  const progressPercent =
    stage === 'downloading' && totalBytes && totalBytes > 0
      ? Math.round((downloadedBytes / totalBytes) * 100)
      : null;

  const handleViewDetails = () => {
    createTab('settings');
  };

  const handleSkip = () => {
    if (newVersion) skipVersion(newVersion);
  };

  return (
    <div className="fixed bottom-10 right-4 z-50 w-80 rounded-lg border border-theme-border bg-theme-bg-elevated/95 shadow-xl shadow-black/20 backdrop-blur-md animate-in slide-in-from-bottom-4 fade-in duration-300">
      {/* Header */}
      <div className="flex items-start gap-3 px-4 pt-3 pb-2">
        <ArrowUpCircle className="h-5 w-5 text-theme-accent shrink-0 mt-0.5" />
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-theme-text">
            {t('settings_view.help.update_available')}
          </p>
          <p className="text-xs text-theme-text-muted mt-0.5">
            v{newVersion}
          </p>
        </div>
        <button
          onClick={dismiss}
          className="shrink-0 p-0.5 rounded-sm text-theme-text-muted hover:text-theme-text hover:bg-theme-bg-hover transition-colors"
          aria-label={t('update_notification.dismiss')}
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {/* Progress bar (downloading) */}
      {stage === 'downloading' && (
        <div className="px-4 pb-2">
          <div className="h-1.5 w-full rounded-full bg-theme-bg-hover overflow-hidden">
            <div
              className="h-full rounded-full bg-theme-accent transition-all duration-300"
              style={{ width: `${progressPercent ?? 0}%` }}
            />
          </div>
          <p className="text-[11px] text-theme-text-muted mt-1">
            {t('settings_view.help.downloading')}
            {progressPercent != null && ` ${progressPercent}%`}
          </p>
        </div>
      )}

      {/* Status text for verifying/installing */}
      {(stage === 'verifying' || stage === 'installing') && (
        <div className="px-4 pb-2">
          <p className="text-xs text-theme-text-muted">
            {stage === 'verifying'
              ? t('settings_view.help.verifying')
              : t('settings_view.help.installing')}
          </p>
        </div>
      )}

      {/* Ready to restart */}
      {stage === 'ready' && (
        <div className="px-4 pb-2">
          <p className="text-xs text-theme-success">
            {t('settings_view.help.ready_to_restart')}
          </p>
        </div>
      )}

      {/* Actions */}
      <div className="flex items-center gap-2 px-4 pb-3">
        {stage === 'available' && (
          <>
            <Button size="sm" className="h-7 text-xs gap-1.5" disabled={stage !== 'available'} onClick={() => void startDownload()}>
              <Download className="h-3.5 w-3.5" />
              {t('settings_view.help.download_install')}
            </Button>
            <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={handleViewDetails}>
              {t('update_notification.view_details')}
            </Button>
            <button
              onClick={handleSkip}
              className="ml-auto text-[11px] text-theme-text-muted hover:text-theme-text transition-colors"
            >
              {t('settings_view.help.skip_version')}
            </button>
          </>
        )}
        {stage === 'ready' && (
          <Button size="sm" className="h-7 text-xs gap-1.5" onClick={() => void restartApp()}>
            <RefreshCw className="h-3.5 w-3.5" />
            {t('settings_view.help.restart_now')}
          </Button>
        )}
        {(stage === 'downloading' || stage === 'verifying' || stage === 'installing') && (
          <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={handleViewDetails}>
            {t('update_notification.view_details')}
          </Button>
        )}
      </div>
    </div>
  );
};
