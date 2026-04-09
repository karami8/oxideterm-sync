// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! File system watcher using inotify (Linux) or polling fallback.
//!
//! Runs in a dedicated thread, sends `watch/event` notifications
//! through a channel that the main loop consumes.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(target_os = "linux")]
use std::path::PathBuf;

use std::path::Path;

#[cfg(target_os = "linux")]
use std::time::Instant;

use crate::protocol::WatchEvent;

fn should_ignore_entry(name: &str, ignore: &[String]) -> bool {
    ignore.iter().any(|ig| ig == name)
        || name.starts_with(".oxtmp.")
        || name.ends_with(".oxswp")
        || name == ".git"
        || name == "node_modules"
        || name == ".hg"
        || name == "__pycache__"
        || name == "target"
}

/// Watcher handle — manages background watch threads.
pub struct Watcher {
    /// Channel to receive watch events from background threads.
    pub rx: mpsc::Receiver<WatchEvent>,
    tx: mpsc::Sender<WatchEvent>,
    /// Active watch sessions.
    watches: Arc<Mutex<HashMap<String, WatchHandle>>>,
}

struct WatchHandle {
    /// Signal the watch thread to stop.
    stop: Arc<Mutex<bool>>,
}

impl Watcher {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rx,
            tx,
            watches: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start watching a directory path.
    pub fn start(&self, path: String, ignore: Vec<String>) -> Result<(), String> {
        let mut watches = self.watches.lock().map_err(|e| e.to_string())?;

        // Already watching?
        if watches.contains_key(&path) {
            return Ok(());
        }

        let stop = Arc::new(Mutex::new(false));
        let handle = WatchHandle {
            stop: Arc::clone(&stop),
        };

        let tx = self.tx.clone();
        let watch_path = path.clone();

        std::thread::spawn(move || {
            watch_thread(&watch_path, &ignore, &tx, &stop);
        });

        watches.insert(path, handle);
        Ok(())
    }

    /// Stop watching a directory path.
    pub fn stop(&self, path: &str) -> Result<(), String> {
        let mut watches = self.watches.lock().map_err(|e| e.to_string())?;
        if let Some(handle) = watches.remove(path) {
            if let Ok(mut stop) = handle.stop.lock() {
                *stop = true;
            }
        }
        Ok(())
    }

    /// Stop all watches (shutdown).
    pub fn stop_all(&self) {
        if let Ok(mut watches) = self.watches.lock() {
            for (_, handle) in watches.drain() {
                if let Ok(mut stop) = handle.stop.lock() {
                    *stop = true;
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Linux inotify implementation
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "linux")]
fn watch_thread(
    path: &str,
    ignore: &[String],
    tx: &mpsc::Sender<WatchEvent>,
    stop: &Arc<Mutex<bool>>,
) {
    use inotify::{Inotify, WatchMask};
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};

    let mut inotify = match Inotify::init() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[agent] Failed to init inotify: {}", e);
            return;
        }
    };

    // Set inotify fd to non-blocking so read_events won't block forever
    let fd = inotify.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }

    // Watch the root directory
    let mask = WatchMask::CREATE
        | WatchMask::DELETE
        | WatchMask::MODIFY
        | WatchMask::MOVED_FROM
        | WatchMask::MOVED_TO
        | WatchMask::CLOSE_WRITE;

    // Map watch descriptor → path for nested directories
    let mut wd_to_path: HashMap<inotify::WatchDescriptor, PathBuf> = HashMap::new();

    // Add watchers recursively
    add_watches_recursive(Path::new(path), ignore, &mut inotify, mask, &mut wd_to_path);

    let mut buffer = [0; 4096];

    // Debounce: accumulate events for 100ms before sending
    let mut pending: HashMap<String, (String, Instant)> = HashMap::new();
    let debounce_duration = Duration::from_millis(100);

    loop {
        // Check stop signal
        if let Ok(s) = stop.lock() {
            if *s {
                break;
            }
        }

        // Read events with timeout
        match inotify.read_events(&mut buffer) {
            Ok(events) => {
                for event in events {
                    let dir_path = wd_to_path
                        .get(&event.wd)
                        .cloned()
                        .unwrap_or_else(|| PathBuf::from(path));

                    let file_path = if let Some(name) = &event.name {
                        dir_path.join(name)
                    } else {
                        dir_path.clone()
                    };

                    let file_path_str = file_path.to_string_lossy().to_string();

                    if let Some(name) = file_path.file_name() {
                        let name_str = name.to_string_lossy();
                        if should_ignore_entry(&name_str, ignore) {
                            continue;
                        }
                    }

                    let kind = if event.mask.contains(inotify::EventMask::CREATE)
                        || event.mask.contains(inotify::EventMask::MOVED_TO)
                    {
                        // If a new directory was created, add a watcher for it
                        if event.mask.contains(inotify::EventMask::ISDIR) {
                            add_watches_recursive(
                                &file_path,
                                ignore,
                                &mut inotify,
                                mask,
                                &mut wd_to_path,
                            );
                        }
                        "create"
                    } else if event.mask.contains(inotify::EventMask::DELETE)
                        || event.mask.contains(inotify::EventMask::MOVED_FROM)
                    {
                        "delete"
                    } else if event.mask.contains(inotify::EventMask::MODIFY)
                        || event.mask.contains(inotify::EventMask::CLOSE_WRITE)
                    {
                        "modify"
                    } else {
                        continue;
                    };

                    // Debounce: update pending event
                    pending.insert(file_path_str, (kind.to_string(), Instant::now()));
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No events ready — check debounce queue
            }
            Err(e) => {
                eprintln!("[agent] inotify read error: {}", e);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        }

        // Flush debounced events
        let now = Instant::now();
        let mut flushed = Vec::new();
        for (path_key, (kind, timestamp)) in &pending {
            if now.duration_since(*timestamp) >= debounce_duration {
                let _ = tx.send(WatchEvent {
                    path: path_key.clone(),
                    kind: kind.clone(),
                });
                flushed.push(path_key.clone());
            }
        }
        for key in flushed {
            pending.remove(&key);
        }

        // Small sleep to avoid busy-looping
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
fn add_watches_recursive(
    dir: &Path,
    ignore: &[String],
    inotify: &mut inotify::Inotify,
    mask: inotify::WatchMask,
    wd_map: &mut HashMap<inotify::WatchDescriptor, PathBuf>,
) {
    // Add watch for this directory
    match inotify.watches().add(dir, mask) {
        Ok(wd) => {
            wd_map.insert(wd, dir.to_path_buf());
        }
        Err(e) => {
            eprintln!("[agent] Failed to watch {}: {}", dir.display(), e);
            return;
        }
    }

    // Recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Skip ignored directories
            if should_ignore_entry(&name_str, ignore) {
                continue;
            }

            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    add_watches_recursive(&entry.path(), ignore, inotify, mask, wd_map);
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Non-Linux polling fallback
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(not(target_os = "linux"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct PollSnapshot {
    is_dir: bool,
    len: u64,
    mtime_secs: u64,
}

#[cfg(not(target_os = "linux"))]
fn metadata_mtime_secs(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn collect_poll_snapshot(
    dir: &Path,
    ignore: &[String],
    snapshot: &mut HashMap<String, PollSnapshot>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if should_ignore_entry(&name, ignore) {
            continue;
        }

        let path = entry.path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let path_string = path.to_string_lossy().to_string();
        let is_dir = metadata.is_dir();
        snapshot.insert(
            path_string,
            PollSnapshot {
                is_dir,
                len: metadata.len(),
                mtime_secs: metadata_mtime_secs(&metadata),
            },
        );

        if is_dir {
            collect_poll_snapshot(&path, ignore, snapshot);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn emit_poll_diffs(
    previous: &HashMap<String, PollSnapshot>,
    current: &HashMap<String, PollSnapshot>,
    tx: &mpsc::Sender<WatchEvent>,
) {
    for (path, snapshot) in current {
        match previous.get(path) {
            None => {
                let _ = tx.send(WatchEvent {
                    path: path.clone(),
                    kind: "create".to_string(),
                });
            }
            Some(prev) => {
                if prev != snapshot && (!snapshot.is_dir || prev.is_dir != snapshot.is_dir) {
                    let _ = tx.send(WatchEvent {
                        path: path.clone(),
                        kind: "modify".to_string(),
                    });
                }
            }
        }
    }

    for path in previous.keys() {
        if !current.contains_key(path) {
            let _ = tx.send(WatchEvent {
                path: path.clone(),
                kind: "delete".to_string(),
            });
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn watch_thread(
    path: &str,
    ignore: &[String],
    tx: &mpsc::Sender<WatchEvent>,
    stop: &Arc<Mutex<bool>>,
) {
    eprintln!("[agent] File watching uses polling fallback on this platform");

    let root = Path::new(path);
    let mut previous = HashMap::new();
    collect_poll_snapshot(root, ignore, &mut previous);

    loop {
        if let Ok(s) = stop.lock() {
            if *s {
                break;
            }
        }

        std::thread::sleep(Duration::from_secs(1));

        let mut current = HashMap::new();
        collect_poll_snapshot(root, ignore, &mut current);
        emit_poll_diffs(&previous, &current, tx);
        previous = current;
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod polling_tests {
    use super::*;

    #[test]
    fn detects_create_modify_delete_diffs() {
        let (tx, rx) = mpsc::channel();

        let previous = HashMap::new();
        let current = HashMap::from([(
            "/tmp/demo.txt".to_string(),
            PollSnapshot {
                is_dir: false,
                len: 3,
                mtime_secs: 1,
            },
        )]);

        emit_poll_diffs(&previous, &current, &tx);
        assert_eq!(rx.recv().unwrap().kind, "create");

        let previous = current.clone();
        let current = HashMap::from([(
            "/tmp/demo.txt".to_string(),
            PollSnapshot {
                is_dir: false,
                len: 4,
                mtime_secs: 2,
            },
        )]);

        emit_poll_diffs(&previous, &current, &tx);
        assert_eq!(rx.recv().unwrap().kind, "modify");

        let previous = current;
        let current = HashMap::new();
        emit_poll_diffs(&previous, &current, &tx);
        assert_eq!(rx.recv().unwrap().kind, "delete");
    }

    #[test]
    fn ignores_configured_temp_patterns() {
        assert!(should_ignore_entry(".oxtmp.123", &[]));
        assert!(should_ignore_entry("demo.oxswp", &[]));
        assert!(should_ignore_entry("node_modules", &[]));
        assert!(should_ignore_entry("custom", &["custom".to_string()]));
        assert!(!should_ignore_entry("src", &[]));
    }
}
