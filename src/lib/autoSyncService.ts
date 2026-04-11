// src/lib/autoSyncService.ts
//
// Auto Sync Service — manages automatic synchronization based on user settings.
// Handles scheduling, execution, and lifecycle management of auto-sync tasks.
//

import { useSettingsStore } from '../store/settingsStore';
import { api } from './api';
import { dataRefreshService } from './dataRefreshService';

/**
 * Auto-sync interval options in minutes
 */
export const AUTO_SYNC_INTERVALS = [
  { value: 1 },
  { value: 5 },
  { value: 15 },
  { value: 30 },
  { value: 60 },
  { value: 360 },
  { value: 720 },
  { value: 1440 },
] as const;

export type AutoSyncInterval = typeof AUTO_SYNC_INTERVALS[number]['value'];

class AutoSyncService {
  private timer: NodeJS.Timeout | null = null;
  private isRunning = false;
  private isExecuting = false;
  private lastExecutionTime: number | null = null;
  private unsubscribeSettings: (() => void) | null = null;
  private visibilityHandler: (() => void) | null = null;
  private initialized = false;
  private readonly MIN_INTERVAL_MS = 1 * 60 * 1000; // 1 minute minimum

  /**
   * Start the auto-sync service
   */
  start() {
    this.stop(); // Stop any existing timer
    
    const store = useSettingsStore.getState();
    const sync = store.getSync();
    if (!sync.autoSyncEnabled || !sync.backendUrl.trim()) {
      console.log('[AutoSync] Not starting: disabled or no backend URL');
      return;
    }

    const intervalMs = sync.autoSyncInterval * 60 * 1000;
    
    // Enforce minimum interval
    if (intervalMs < this.MIN_INTERVAL_MS) {
      console.warn(`[AutoSync] Interval ${sync.autoSyncInterval} minutes is below minimum, using 1 minute`);
      // Don't start if interval is too short
      return;
    }

    console.log(`[AutoSync] Starting with interval: ${sync.autoSyncInterval} minutes (${intervalMs}ms)`);
    
    // Initial execution check
    this.checkAndExecute();
    
    // Set up periodic timer
    this.timer = setInterval(() => {
      this.checkAndExecute();
    }, intervalMs);
    
    this.isRunning = true;
  }

  /**
   * Stop the auto-sync service
   */
  stop() {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
    this.isRunning = false;
    console.log('[AutoSync] Stopped');
  }

  /**
   * Check if auto-sync should execute and execute if conditions are met
   */
  private async checkAndExecute() {
    if (this.isExecuting) {
      console.log('[AutoSync] Already executing, skipping');
      return;
    }

    const store = useSettingsStore.getState();
    const sync = store.getSync();
    if (!sync.autoSyncEnabled || !sync.backendUrl.trim()) {
      console.log('[AutoSync] Not executing: disabled or no backend URL');
      return;
    }

    // Check if enough time has passed since last execution
    if (this.lastExecutionTime) {
      const elapsedMs = Date.now() - this.lastExecutionTime;
      const requiredMs = sync.autoSyncInterval * 60 * 1000;
      
      if (elapsedMs < requiredMs * 0.9) { // 90% of interval to account for timer drift
        console.log(`[AutoSync] Skipping: only ${Math.round(elapsedMs / 1000)}s elapsed, need ${sync.autoSyncInterval * 60}s`);
        return;
      }
    }

    this.isExecuting = true;
    try {
      await this.executeAutoSync();
      this.lastExecutionTime = Date.now();
    } catch (error) {
      console.error('[AutoSync] Execution failed:', error);
    } finally {
      this.isExecuting = false;
    }
  }

  /**
   * Execute a single auto-sync operation
   */
  private async executeAutoSync(): Promise<void> {
    console.log('[AutoSync] Executing auto-sync...');
    
    const store = useSettingsStore.getState();
    const sync = store.getSync();
    if (!sync) {
      throw new Error('Sync settings not available');
    }

    // Build sync configuration
    const config = {
      backendUrl: sync.backendUrl,
      verifyTls: sync.verifyTls,
      timeoutSecs: sync.timeoutSecs,
      syncMode: 'push' as const,
    };

    // Execute sync
    const status = await api.syncNow(config);
    
    if (status.success) {
      console.log('[AutoSync] Sync successful');
      
      // Auto-sync is always push-only; only update local sync metadata.
      try {
        await dataRefreshService.refreshSettings();
        console.log('[AutoSync] Sync metadata refreshed');
      } catch (refreshError) {
        console.error('[AutoSync] Sync metadata refresh failed:', refreshError);
        // Don't throw here, as the sync itself was successful
      }
    } else {
      console.error('[AutoSync] Sync failed:', status.message);
      throw new Error(`Sync failed: ${status.message}`);
    }
  }

  /**
   * Manually trigger an auto-sync (for testing or manual override)
   */
  async triggerManual(): Promise<boolean> {
    if (this.isExecuting) {
      console.log('[AutoSync] Already executing, cannot trigger manual');
      return false;
    }

    try {
      await this.executeAutoSync();
      return true;
    } catch (error) {
      console.error('[AutoSync] Manual trigger failed:', error);
      return false;
    }
  }

  /**
   * Get service status
   */
  getStatus() {
    const store = useSettingsStore.getState();
    const sync = store.getSync();
    return {
      isRunning: this.isRunning,
      isExecuting: this.isExecuting,
      isEnabled: sync.autoSyncEnabled,
      interval: sync.autoSyncInterval,
      lastExecutionTime: this.lastExecutionTime,
      lastSyncTime: sync.lastAutoSyncTime,
    };
  }

  /**
   * Set up listener for settings changes
   */
  setupSettingsListener() {
    this.unsubscribeSettings?.();
    this.unsubscribeSettings = useSettingsStore.subscribe(
      (state) => state.settings.sync,
      (newSync, oldSync) => {
        if (!newSync || !oldSync) return;
        
        // Check if auto-sync settings changed
        const settingsChanged =
          newSync.autoSyncEnabled !== oldSync.autoSyncEnabled ||
          newSync.autoSyncInterval !== oldSync.autoSyncInterval ||
          newSync.backendUrl !== oldSync.backendUrl;
        
        if (settingsChanged) {
          console.log('[AutoSync] Settings changed, restarting...');
          if (newSync.autoSyncEnabled && newSync.backendUrl.trim()) {
            this.start();
          } else {
            this.stop();
          }
        }
      }
    );
  }

  /**
   * Initialize the service
   */
  initialize() {
    if (this.initialized) {
      console.log('[AutoSync] Service already initialized');
      return;
    }

    this.initialized = true;
    this.setupSettingsListener();
    
    // Start if enabled
    const store = useSettingsStore.getState();
    const sync = store.getSync();
    if (sync.autoSyncEnabled && sync.backendUrl.trim()) {
      this.start();
    }
    
    // Set up visibility change listener - keep auto-sync running in background
    this.visibilityHandler = () => {
      if (document.visibilityState === 'visible') {
        console.log('[AutoSync] App became visible, ensuring auto-sync is running');
        // Ensure auto-sync is running if enabled
        const store = useSettingsStore.getState();
        const sync = store.getSync();
        if (sync.autoSyncEnabled && sync.backendUrl.trim() && !this.isRunning) {
          this.start();
        }
      }
      // When app becomes hidden, we don't stop auto-sync - it continues running in background
    };
    document.addEventListener('visibilitychange', this.visibilityHandler);
    
    console.log('[AutoSync] Service initialized');
  }
}

// Singleton instance
export const autoSyncService = new AutoSyncService();