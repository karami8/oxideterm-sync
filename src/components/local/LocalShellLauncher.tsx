// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { Terminal, Star, Clock, ChevronRight } from 'lucide-react';
import { useLocalTerminalStore } from '../../store/localTerminalStore';
import { useAppStore } from '../../store/appStore';
import { useSettingsStore } from '../../store/settingsStore';
import { ShellInfo } from '../../types';
import { cn } from '../../lib/utils';

interface LocalShellLauncherProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export const LocalShellLauncher: React.FC<LocalShellLauncherProps> = ({
  open,
  onOpenChange,
}) => {
  const { t } = useTranslation();
  const { shells, defaultShell, createTerminal, loadShells, shellsLoaded } = useLocalTerminalStore();
  const { createTab } = useAppStore();
  const { settings, updateLocalTerminal } = useSettingsStore();
  
  const [customCwd, setCustomCwd] = useState('');
  const [selectedShell, setSelectedShell] = useState<ShellInfo | null>(null);
  const [loading, setLoading] = useState(false);

  // Load shells when dialog opens
  useEffect(() => {
    if (open && !shellsLoaded) {
      loadShells();
    }
  }, [open, shellsLoaded, loadShells]);

  // Set default selection
  useEffect(() => {
    if (open && defaultShell && !selectedShell) {
      setSelectedShell(defaultShell);
    }
  }, [open, defaultShell, selectedShell]);

  // Get recent shells from settings
  const recentShellIds = settings.localTerminal?.recentShellIds || [];
  const defaultShellId = settings.localTerminal?.defaultShellId;

  // Sort shells: default first, then recent, then others
  const sortedShells = [...shells].sort((a, b) => {
    if (a.id === defaultShellId) return -1;
    if (b.id === defaultShellId) return 1;
    const aRecent = recentShellIds.indexOf(a.id);
    const bRecent = recentShellIds.indexOf(b.id);
    if (aRecent !== -1 && bRecent === -1) return -1;
    if (aRecent === -1 && bRecent !== -1) return 1;
    if (aRecent !== -1 && bRecent !== -1) return aRecent - bRecent;
    return 0;
  });

  const handleLaunch = async (shell: ShellInfo) => {
    setLoading(true);
    try {
      const info = await createTerminal({
        shellPath: shell.path,
        cwd: customCwd || undefined,
      });
      
      // Update recent shells
      const newRecentIds = [
        shell.id,
        ...recentShellIds.filter(id => id !== shell.id),
      ].slice(0, 5);
      
      updateLocalTerminal('recentShellIds', newRecentIds);
      
      createTab('local_terminal', info.id);
      onOpenChange(false);
    } catch (err) {
      console.error('Failed to create local terminal:', err);
    } finally {
      setLoading(false);
    }
  };

  const handleSetDefault = (shell: ShellInfo) => {
    updateLocalTerminal('defaultShellId', shell.id);
  };

  const isDefault = (shell: ShellInfo) => shell.id === defaultShellId;
  const isRecent = (shell: ShellInfo) => recentShellIds.includes(shell.id);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[480px] bg-theme-bg-elevated border-theme-border">
        <DialogHeader>
          <DialogTitle className="text-theme-text">{t('local_shell.title')}</DialogTitle>
          <DialogDescription className="text-theme-text-muted">
            {t('local_shell.description')}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 p-4">
          {/* Shell List */}
          <div className="space-y-2">
            <Label className="text-theme-text-muted text-xs uppercase tracking-wider">
              {t('local_shell.available_shells')}
            </Label>
            <div className="space-y-1 max-h-[300px] overflow-y-auto">
              {sortedShells.length === 0 ? (
                <div className="text-center py-4 text-theme-text-muted">
                  {t('local_shell.loading_shells')}
                </div>
              ) : (
                sortedShells.map((shell) => (
                  <div
                    key={shell.id}
                    className={cn(
                      "flex items-center gap-3 p-3 rounded-lg cursor-pointer transition-colors",
                      "hover:bg-theme-bg-hover",
                      selectedShell?.id === shell.id && "bg-theme-bg-hover ring-1 ring-theme-accent"
                    )}
                    onClick={() => setSelectedShell(shell)}
                    onDoubleClick={() => handleLaunch(shell)}
                  >
                    <Terminal className="h-5 w-5 text-theme-text-muted flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-theme-text truncate">
                          {shell.label}
                        </span>
                        {isDefault(shell) && (
                          <Star className="h-3 w-3 text-yellow-500 fill-yellow-500" />
                        )}
                        {!isDefault(shell) && isRecent(shell) && (
                          <Clock className="h-3 w-3 text-theme-text-muted" />
                        )}
                      </div>
                      <div className="text-xs text-theme-text-muted truncate">
                        {shell.path}
                      </div>
                    </div>
                    <ChevronRight className="h-4 w-4 text-theme-text-muted flex-shrink-0" />
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Options */}
          {selectedShell && (
            <div className="space-y-3 pt-2 border-t border-theme-border">
              <div className="space-y-2">
                <Label htmlFor="cwd" className="text-theme-text-muted text-xs">
                  {t('local_shell.working_directory')}
                </Label>
                <Input
                  id="cwd"
                  value={customCwd}
                  onChange={(e) => setCustomCwd(e.target.value)}
                  placeholder={t('local_shell.working_directory_placeholder')}
                  className="bg-theme-bg border-theme-border text-theme-text"
                />
              </div>
            </div>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-theme-border">
          <div>
            {selectedShell && !isDefault(selectedShell) && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => handleSetDefault(selectedShell)}
                className="text-theme-text-muted hover:text-theme-text"
              >
                <Star className="h-3 w-3 mr-1" />
                {t('local_shell.set_default')}
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            <Button
              variant="ghost"
              onClick={() => onOpenChange(false)}
              className="text-theme-text-muted"
            >
              {t('local_shell.cancel')}
            </Button>
            <Button
              onClick={() => selectedShell && handleLaunch(selectedShell)}
              disabled={!selectedShell || loading}
              className="bg-theme-accent hover:bg-theme-accent/90"
            >
              {loading ? t('local_shell.launching') : t('local_shell.launch')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
};
