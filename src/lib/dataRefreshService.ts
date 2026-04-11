// src/lib/dataRefreshService.ts
//
// Data Refresh Service — handles refreshing application data after synchronization.
// Ensures UI reflects the latest state after sync operations.
//

import { useAppStore } from '../store/appStore';
import { useSettingsStore } from '../store/settingsStore';
import { useSessionTreeStore } from '../store/sessionTreeStore';
import { useLocalTerminalStore } from '../store/localTerminalStore';
import { useToastStore } from '../hooks/useToast';

/**
 * Data Refresh Service
 * 
 * This service coordinates refreshing of various application data stores
 * after synchronization operations. It ensures that the UI reflects the
 * latest state from the backend.
 */
class DataRefreshService {
  private isRefreshing = false;

  private updateLastAutoSyncTime() {
    try {
      const store = useSettingsStore.getState();
      const sync = store.getSync();
      if (sync.autoSyncEnabled) {
        store.updateSync('lastAutoSyncTime', new Date().toISOString());
        console.log('[DataRefresh] Last auto-sync time updated');
      }
    } catch (error) {
      console.warn('[DataRefresh] Failed to update last sync time:', error);
      // Non-critical error, don't mark as failure
    }
  }

  /**
   * Refresh all application data after synchronization
   * 
   * This method should be called after any sync operation (manual or auto)
   * to ensure the UI reflects the latest state.
   * 
   * @param options.refreshConnections - Whether to refresh connection data (default: true)
   * @param options.refreshForwards - Whether to refresh port forward data (default: true)
   * @param options.refreshSettings - Whether to refresh settings data (default: true)
   * @param options.refreshTerminals - Whether to refresh terminal data (default: true)
   * @param options.showToast - Whether to show success/error toasts (default: true)
   * @returns Promise<boolean> - true if all refreshes succeeded, false otherwise
   */
  async refreshAfterSync(options: {
    refreshConnections?: boolean;
    refreshForwards?: boolean;
    refreshSettings?: boolean;
    refreshTerminals?: boolean;
    showToast?: boolean;
  } = {}): Promise<boolean> {
    const {
      refreshConnections = true,
      refreshForwards = true,
      refreshSettings = true,
      refreshTerminals = true,
      showToast = true,
    } = options;

    // Prevent concurrent refresh operations
    if (this.isRefreshing) {
      console.log('[DataRefresh] Already refreshing, skipping');
      return false;
    }

    this.isRefreshing = true;
    let success = true;
    const errors: string[] = [];

    try {
      console.log('[DataRefresh] Starting post-sync refresh');

      // 1. Refresh connections (SSH connections)
      if (refreshConnections) {
        try {
          await useAppStore.getState().refreshConnections();
          console.log('[DataRefresh] Connections refreshed');
        } catch (error) {
          success = false;
          errors.push(`连接刷新失败: ${error}`);
          console.error('[DataRefresh] Failed to refresh connections:', error);
        }
      }

      // 2. Refresh saved connections (sidebar list)
      if (refreshConnections) {
        try {
          await useAppStore.getState().loadSavedConnections();
          console.log('[DataRefresh] Saved connections refreshed');
          // Trigger event for sidebar to update
          window.dispatchEvent(new CustomEvent('saved-connections-changed'));
        } catch (error) {
          success = false;
          errors.push(`保存的连接刷新失败: ${error}`);
          console.error('[DataRefresh] Failed to refresh saved connections:', error);
        }
      }

      // 3. Refresh session tree (topology and node states)
      if (refreshConnections) {
        try {
          await useSessionTreeStore.getState().fetchTree();
          console.log('[DataRefresh] Session tree refreshed');
        } catch (error) {
          success = false;
          errors.push(`会话树刷新失败: ${error}`);
          console.error('[DataRefresh] Failed to refresh session tree:', error);
        }
      }

      // 4. Refresh port forwards (if applicable)
      if (refreshForwards) {
        try {
          // Note: Forward refresh is typically handled by connection refresh
          // but we might need to trigger specific forward updates
          console.log('[DataRefresh] Forward refresh would be triggered here');
        } catch (error) {
          success = false;
          errors.push(`端口转发刷新失败: ${error}`);
          console.error('[DataRefresh] Failed to refresh forwards:', error);
        }
      }

      // 5. Refresh settings (if sync includes settings)
      if (refreshSettings) {
        try {
          this.updateLastAutoSyncTime();
          console.log('[DataRefresh] Settings refresh completed');
        } catch (error) {
          success = false;
          errors.push(`设置刷新失败: ${error}`);
          console.error('[DataRefresh] Failed to refresh settings:', error);
        }
      }

      // 6. Refresh local terminals
      if (refreshTerminals) {
        try {
          await useLocalTerminalStore.getState().refreshTerminals();
          console.log('[DataRefresh] Local terminals refreshed');
        } catch (error) {
          success = false;
          errors.push(`本地终端刷新失败: ${error}`);
          console.error('[DataRefresh] Failed to refresh local terminals:', error);
        }
      }

      console.log(`[DataRefresh] Refresh completed with ${success ? 'success' : 'errors'}`);
      if (!success && showToast && errors.length > 0) {
        const { addToast } = useToastStore.getState();
        addToast({
          title: '数据刷新部分失败',
          description: errors.join('；'),
          variant: 'warning',
          duration: 5000,
        });
      }
      return success;

    } catch (error) {
      console.error('[DataRefresh] Unexpected error during refresh:', error);
      if (showToast) {
        const { addToast } = useToastStore.getState();
        addToast({
          title: '数据刷新失败',
          description: `未知错误: ${error}`,
          variant: 'error',
          duration: 5000,
        });
      }
      return false;
    } finally {
      this.isRefreshing = false;
    }
  }

  /**
   * Refresh only connection-related data
   * 
   * Useful for sync operations that only affect connections
   */
  async refreshConnections(): Promise<boolean> {
    return this.refreshAfterSync({
      refreshConnections: true,
      refreshForwards: true,
      refreshSettings: false,
      refreshTerminals: false,
      showToast: false,
    });
  }

  /**
   * Refresh only settings-related data
   * 
   * Useful for sync operations that only affect settings
   */
  async refreshSettings(): Promise<boolean> {
    return this.refreshAfterSync({
      refreshConnections: false,
      refreshForwards: false,
      refreshSettings: true,
      refreshTerminals: false,
      showToast: false,
    });
  }

  /**
   * Quick refresh - minimal data refresh for performance
   * 
   * Used for auto-sync operations where we don't want to disrupt the user
   */
  async quickRefresh(): Promise<boolean> {
    return this.refreshAfterSync({
      refreshConnections: true,
      refreshForwards: false,
      refreshSettings: false,
      refreshTerminals: false,
      showToast: false,
    });
  }

  /**
   * Full refresh - refresh all data with user notification
   * 
   * Used for manual sync operations where user expects feedback
   */
  async fullRefresh(): Promise<boolean> {
    return this.refreshAfterSync({
      refreshConnections: true,
      refreshForwards: true,
      refreshSettings: true,
      refreshTerminals: true,
      showToast: true,
    });
  }

  /**
   * Check if refresh is currently in progress
   */
  isRefreshingInProgress(): boolean {
    return this.isRefreshing;
  }
}

// Export singleton instance
export const dataRefreshService = new DataRefreshService();

// Export types
export type { DataRefreshService };

// Export for testing
export const __test__ = {
  DataRefreshService,
};