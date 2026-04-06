// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog';
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
import type { TFunction } from 'i18next';
import { useKeybindingStore } from '@/store/keybindingStore';
import {
  type ActionId,
  type ActionScope,
  type KeyCombo,
  getDefaults,
  getBinding,
  getDefaultDefinition,
  findConflicts,
  formatCombo,
} from '@/lib/keybindingRegistry';
import { platform } from '@/lib/platform';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Separator } from '@/components/ui/separator';
import { Search, RotateCw, Download, Upload, X } from 'lucide-react';
import { cn } from '@/lib/utils';

// ─── Types ───────────────────────────────────────────────────────────

type ScopeFilter = 'all' | ActionScope;

type RecordingState = {
  actionId: ActionId;
  combo: KeyCombo | null;
  conflicts: ActionId[];
} | null;

// ─── Helpers ─────────────────────────────────────────────────────────

const SCOPE_ORDER: ActionScope[] = ['global', 'terminal', 'split', 'palette'];

const SCOPE_FILTER_OPTIONS: ScopeFilter[] = ['all', ...SCOPE_ORDER];

function getScopeI18nKey(scope: ActionScope): string {
  return `settings_view.keybindings.scope_${scope}`;
}

function isModifierOnly(e: KeyboardEvent): boolean {
  return ['Control', 'Shift', 'Alt', 'Meta'].includes(e.key);
}

function keyEventToCombo(e: KeyboardEvent): KeyCombo {
  return {
    key: e.key.toLowerCase(),
    ctrl: e.ctrlKey,
    shift: e.shiftKey,
    alt: e.altKey,
    meta: e.metaKey,
  };
}

function isValidCombo(val: unknown): val is KeyCombo {
  if (typeof val !== 'object' || val === null) return false;
  const obj = val as Record<string, unknown>;
  return (
    typeof obj.key === 'string' &&
    typeof obj.ctrl === 'boolean' &&
    typeof obj.shift === 'boolean' &&
    typeof obj.alt === 'boolean' &&
    typeof obj.meta === 'boolean'
  );
}

// ─── Component ───────────────────────────────────────────────────────

type KeybindingEditorSectionProps = {
  onToastSuccess: (msg: string) => void;
  onToastError: (msg: string) => void;
  onConfirm: (opts: { title: string; variant?: 'danger' }) => Promise<boolean>;
};

export const KeybindingEditorSection = ({ onToastSuccess, onToastError, onConfirm }: KeybindingEditorSectionProps) => {
  const { t } = useTranslation();
  const { overrides, setBinding, resetBinding, resetAll } = useKeybindingStore();
  const side = platform.isMac ? 'mac' : 'other';

  const [searchQuery, setSearchQuery] = useState('');
  const [scopeFilter, setScopeFilter] = useState<ScopeFilter>('all');
  const [recording, setRecording] = useState<RecordingState>(null);

  const recordingRef = useRef(recording);
  recordingRef.current = recording;

  const allActions = useMemo(() => getDefaults(), []);

  // ── Filter actions ──

  const filteredActions = allActions.filter((def) => {
    if (scopeFilter !== 'all' && def.scope !== scopeFilter) return false;
    if (searchQuery) {
      const actionLabel = t(`settings_view.keybindings.actions.${def.id}`, def.id);
      const query = searchQuery.toLowerCase();
      if (
        !actionLabel.toLowerCase().includes(query) &&
        !def.id.toLowerCase().includes(query)
      ) {
        return false;
      }
    }
    return true;
  });

  // Group by scope
  const grouped = SCOPE_ORDER
    .map((scope) => ({
      scope,
      actions: filteredActions.filter((d) => d.scope === scope),
    }))
    .filter((g) => g.actions.length > 0);

  // ── Recording keydown handler ──

  useEffect(() => {
    if (!recording) return;

    const handler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      // Escape cancels recording
      if (e.key === 'Escape') {
        setRecording(null);
        return;
      }

      // Ignore modifier-only presses
      if (isModifierOnly(e)) return;

      const combo = keyEventToCombo(e);
      const rec = recordingRef.current;
      if (!rec) return;

      const def = getDefaultDefinition(rec.actionId);
      const conflicts = def ? findConflicts(combo, def.scope, rec.actionId) : [];

      setRecording({ actionId: rec.actionId, combo, conflicts });
    };

    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [!!recording]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Actions ──

  const startRecording = useCallback((actionId: ActionId) => {
    setRecording({ actionId, combo: null, conflicts: [] });
  }, []);

  const confirmRecording = useCallback(() => {
    if (!recording?.combo) return;
    setBinding(recording.actionId, side, recording.combo);
    setRecording(null);
  }, [recording, setBinding, side]);

  const cancelRecording = useCallback(() => {
    setRecording(null);
  }, []);

  const handleResetBinding = useCallback((actionId: ActionId) => {
    resetBinding(actionId, side);
  }, [resetBinding, side]);

  const handleResetAll = useCallback(async () => {
    const confirmed = await onConfirm({
      title: t('settings_view.keybindings.reset_all_confirm'),
      variant: 'danger',
    });
    if (confirmed) {
      resetAll();
    }
  }, [resetAll, onConfirm, t]);

  // ── Import / Export ──

  const handleExport = useCallback(async () => {
    try {
      const savePath = await saveFileDialog({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: 'oxideterm-keybindings.json',
      });
      if (!savePath) return;

      const data: Record<string, { mac?: KeyCombo; other?: KeyCombo }> = {};
      for (const [id, override] of overrides) {
        data[id] = override;
      }
      await writeTextFile(savePath, JSON.stringify(data, null, 2));
      onToastSuccess(t('settings_view.keybindings.export_success'));
    } catch {
      onToastError(t('settings_view.keybindings.export_error'));
    }
  }, [overrides, onToastSuccess, onToastError, t]);

  const handleImport = useCallback(async () => {
    try {
      const selected = await openFileDialog({
        multiple: false,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!selected || typeof selected !== 'string') return;

      const content = await readTextFile(selected);
      const parsed = JSON.parse(content);

      if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
        onToastError(t('settings_view.keybindings.import_invalid'));
        return;
      }

      // Validate keys against known ActionIds
      const validIds = new Set(getDefaults().map((d) => d.id));
      for (const key of Object.keys(parsed)) {
        if (!validIds.has(key as ActionId)) {
          onToastError(t('settings_view.keybindings.import_invalid'));
          return;
        }
        const val = parsed[key];
        if (typeof val !== 'object' || val === null) {
          onToastError(t('settings_view.keybindings.import_invalid'));
          return;
        }
      }

      // Validate KeyCombo structure for each override
      for (const [, val] of Object.entries(parsed)) {
        const o = val as Record<string, unknown>;
        if (o.mac !== undefined && !isValidCombo(o.mac)) {
          onToastError(t('settings_view.keybindings.import_invalid'));
          return;
        }
        if (o.other !== undefined && !isValidCombo(o.other)) {
          onToastError(t('settings_view.keybindings.import_invalid'));
          return;
        }
      }

      // Apply: reset first, then set each
      resetAll();
      for (const [id, override] of Object.entries(parsed)) {
        const o = override as { mac?: KeyCombo; other?: KeyCombo };
        if (o.mac) setBinding(id as ActionId, 'mac', o.mac);
        if (o.other) setBinding(id as ActionId, 'other', o.other);
      }
      onToastSuccess(t('settings_view.keybindings.import_success'));
    } catch (e) {
      onToastError(t('settings_view.keybindings.import_error', { error: String(e) }));
    }
  }, [resetAll, setBinding, onToastSuccess, onToastError, t]);

  // ── Check if an action has user override ──

  const isModified = useCallback((actionId: ActionId): boolean => {
    const override = overrides.get(actionId);
    return override !== undefined && override[side] !== undefined;
  }, [overrides, side]);

  const hasAnyOverrides = overrides.size > 0;

  // ─── Render ────────────────────────────────────────────────────────

  return (
    <div className="space-y-8 animate-in fade-in slide-in-from-bottom-2 duration-300">
      {/* Header */}
      <div>
        <h3 className="text-2xl font-medium text-theme-text-heading mb-2">
          {t('settings_view.keybindings.title')}
        </h3>
        <p className="text-theme-text-muted">
          {t('settings_view.keybindings.description')}
        </p>
      </div>
      <Separator />

      {/* Toolbar: search + scope filter + actions */}
      <div className="flex flex-wrap items-center gap-3">
        {/* Search */}
        <div className="relative flex-1 min-w-[200px] max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-theme-text-muted" />
          <Input
            className="pl-9 h-9"
            placeholder={t('settings_view.keybindings.search_placeholder')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>

        {/* Scope filter pills */}
        <div className="flex gap-1">
          {SCOPE_FILTER_OPTIONS.map((scope) => (
            <Button
              key={scope}
              variant={scopeFilter === scope ? 'secondary' : 'ghost'}
              size="sm"
              className="h-8 px-3 text-xs"
              onClick={() => setScopeFilter(scope)}
            >
              {scope === 'all'
                ? t('settings_view.keybindings.scope_all')
                : t(getScopeI18nKey(scope))}
            </Button>
          ))}
        </div>

        {/* Import / Export / Reset */}
        <div className="flex gap-1 ml-auto">
          <Button variant="ghost" size="sm" className="h-8 gap-1.5" onClick={handleImport}>
            <Upload className="h-3.5 w-3.5" />
            {t('settings_view.keybindings.import')}
          </Button>
          <Button variant="ghost" size="sm" className="h-8 gap-1.5" onClick={handleExport}>
            <Download className="h-3.5 w-3.5" />
            {t('settings_view.keybindings.export')}
          </Button>
          {hasAnyOverrides && (
            <Button variant="ghost" size="sm" className="h-8 gap-1.5 text-red-400 hover:text-red-300" onClick={handleResetAll}>
              <RotateCw className="h-3.5 w-3.5" />
              {t('settings_view.keybindings.reset_all')}
            </Button>
          )}
        </div>
      </div>

      {/* Action table */}
      {grouped.length === 0 ? (
        <div className="text-center text-theme-text-muted py-12">
          {t('settings_view.keybindings.no_results')}
        </div>
      ) : (
        <div className="space-y-6">
          {grouped.map(({ scope, actions }) => (
            <div key={scope} className="rounded-lg border border-theme-border bg-theme-bg-card overflow-hidden">
              {/* Scope header */}
              <div className="px-5 py-3 bg-theme-bg-elevated/50 border-b border-theme-border">
                <h4 className="text-xs font-medium text-theme-text-muted uppercase tracking-wider">
                  {t(getScopeI18nKey(scope))}
                </h4>
              </div>

              {/* Actions list */}
              <div className="divide-y divide-theme-border/30">
                {actions.map((def) => {
                  const currentCombo = getBinding(def.id);
                  const defaultDef = getDefaultDefinition(def.id);
                  const defaultCombo = defaultDef ? defaultDef[side] : undefined;
                  const modified = isModified(def.id);
                  const isRecordingThis = recording?.actionId === def.id;

                  return (
                    <div
                      key={def.id}
                      className={cn(
                        'flex items-center justify-between px-5 py-3 transition-colors',
                        isRecordingThis && 'bg-theme-accent/5 relative z-50'
                      )}
                    >
                      {/* Action name */}
                      <div className="flex items-center gap-3 min-w-0 flex-1">
                        <span className="text-sm text-theme-text truncate">
                          {t(`settings_view.keybindings.actions.${def.id}`, def.id)}
                        </span>
                        {modified && (
                          <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-theme-accent/20 text-theme-accent font-medium">
                            {t('settings_view.keybindings.modified')}
                          </span>
                        )}
                      </div>

                      {/* Shortcut display / recording */}
                      <div className="flex items-center gap-2 shrink-0">
                        {isRecordingThis ? (
                          <RecordingCell
                            recording={recording}
                            onConfirm={confirmRecording}
                            onCancel={cancelRecording}
                            t={t}
                          />
                        ) : (
                          <>
                            <button
                              className="group flex items-center gap-1 cursor-pointer hover:bg-theme-bg-hover rounded px-2 py-1 transition-colors"
                              onClick={() => startRecording(def.id)}
                              title={t('settings_view.keybindings.record_prompt')}
                            >
                              {currentCombo && (
                                <kbd className="px-2 py-0.5 rounded bg-theme-bg text-theme-text text-xs font-mono border border-theme-border/50">
                                  {formatCombo(currentCombo)}
                                </kbd>
                              )}
                            </button>
                            {modified && defaultCombo && (
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-7 w-7 p-0 text-theme-text-muted hover:text-theme-text"
                                onClick={() => handleResetBinding(def.id)}
                                title={t('settings_view.keybindings.reset_to_default') + ` (${formatCombo(defaultCombo)})`}
                              >
                                <RotateCw className="h-3.5 w-3.5" />
                              </Button>
                            )}
                          </>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Recording overlay/backdrop */}
      {recording && (
        <div
          className="fixed inset-0 z-40"
          aria-hidden="true"
          onClick={cancelRecording}
        />
      )}
    </div>
  );
};

// ─── Recording Cell Sub-Component ────────────────────────────────────

type RecordingCellProps = {
  recording: NonNullable<RecordingState>;
  onConfirm: () => void;
  onCancel: () => void;
  t: TFunction;
};

const RecordingCell = ({ recording, onConfirm, onCancel, t }: RecordingCellProps) => {
  const hasConflicts = recording.conflicts.length > 0;

  return (
    <div className="flex items-center gap-2">
      <div className="flex flex-col items-end gap-1">
        {recording.combo ? (
          <>
            <kbd className="px-2 py-0.5 rounded bg-theme-accent/20 text-theme-accent text-xs font-mono border border-theme-accent/30">
              {formatCombo(recording.combo)}
            </kbd>
            {hasConflicts && (
              <span className="text-[11px] text-yellow-400">
                {t('settings_view.keybindings.conflict_warning', {
                  action: t(`settings_view.keybindings.actions.${recording.conflicts[0]}`),
                  scope: t(getScopeI18nKey(getDefaultDefinition(recording.conflicts[0])?.scope ?? 'global')),
                })}
              </span>
            )}
          </>
        ) : (
          <span className="text-xs text-theme-text-muted italic animate-pulse">
            {t('settings_view.keybindings.record_prompt')}
          </span>
        )}
      </div>

      {recording.combo && (
        <Button
          variant="ghost"
          size="sm"
          className={cn(
            'h-7 px-2 text-xs',
            hasConflicts ? 'text-yellow-400 hover:text-yellow-300' : 'text-theme-accent hover:text-theme-accent'
          )}
          onClick={(e) => { e.stopPropagation(); onConfirm(); }}
        >
          {hasConflicts
            ? t('settings_view.keybindings.override_anyway')
            : '✓'}
        </Button>
      )}

      <Button
        variant="ghost"
        size="sm"
        className="h-7 w-7 p-0 text-theme-text-muted hover:text-theme-text"
        onClick={(e) => { e.stopPropagation(); onCancel(); }}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
};
