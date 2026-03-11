/**
 * SFTP Context Registry
 * 
 * Lightweight module-level registry that SFTPView components register into,
 * allowing sidebarContextProvider to read SFTP browser state for AI context.
 * Follows the same pattern as terminalRegistry.
 */

export interface SftpContextSnapshot {
  /** Current remote working directory */
  remotePath: string;
  /** Remote home directory */
  remoteHome: string;
  /** Selected file/folder names in the current directory */
  selectedFiles: string[];
  /** Node ID this SFTP view is connected to */
  nodeId: string;
  /** Timestamp of last update */
  updatedAt: number;
}

/** nodeId → SftpContextSnapshot */
const registry = new Map<string, SftpContextSnapshot>();

/** Max age before considering a snapshot stale (2 minutes) */
const MAX_AGE_MS = 2 * 60 * 1000;

/**
 * Register or update SFTP context for a node.
 * Called by SFTPView on path/selection changes.
 */
export function registerSftpContext(
  nodeId: string,
  remotePath: string,
  remoteHome: string,
  selectedFiles: string[],
): void {
  registry.set(nodeId, {
    remotePath,
    remoteHome,
    selectedFiles,
    nodeId,
    updatedAt: Date.now(),
  });
}

/**
 * Unregister SFTP context (called on SFTPView unmount).
 */
export function unregisterSftpContext(nodeId: string): void {
  registry.delete(nodeId);
}

/**
 * Get SFTP context for a specific node.
 * Returns null if not registered or stale.
 */
export function getSftpContext(nodeId: string): SftpContextSnapshot | null {
  const entry = registry.get(nodeId);
  if (!entry) return null;

  if (Date.now() - entry.updatedAt > MAX_AGE_MS) {
    registry.delete(nodeId);
    return null;
  }

  return entry;
}

/**
 * Get all active (non-stale) SFTP contexts.
 */
export function getAllSftpContexts(): SftpContextSnapshot[] {
  const now = Date.now();
  const results: SftpContextSnapshot[] = [];

  for (const [nodeId, entry] of registry) {
    if (now - entry.updatedAt > MAX_AGE_MS) {
      registry.delete(nodeId);
    } else {
      results.push(entry);
    }
  }

  return results;
}
