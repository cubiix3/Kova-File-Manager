//! Native directory notifications. Handles and all filesystem calls stay on
//! one background thread; consumers only exchange paths through bounded state.
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::{
            FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION,
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification,
            FindFirstChangeNotificationW, FindNextChangeNotification,
        },
        System::Threading::WaitForSingleObject,
    },
    core::PCWSTR,
};

const TICK: Duration = Duration::from_millis(50);
const QUIET: Duration = Duration::from_millis(150);
const MAX_DELAY: Duration = Duration::from_millis(600);
const RETRY: Duration = Duration::from_secs(2);

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this thread owns the notification handle and no wait is pending.
        unsafe {
            let _ = FindCloseChangeNotification(self.0);
        }
    }
}

#[derive(Default)]
struct Burst {
    first: Option<Instant>,
    last: Option<Instant>,
}
impl Burst {
    fn changed(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }
    fn ready(&self, now: Instant) -> bool {
        self.first
            .is_some_and(|t| now.duration_since(t) >= MAX_DELAY)
            || self.last.is_some_and(|t| now.duration_since(t) >= QUIET)
    }
}
struct Watch {
    recursive: bool,
    handle: Option<Handle>,
    retry: Instant,
    burst: Burst,
}

pub struct DirectoryWatcher {
    commands: mpsc::SyncSender<()>,
    desired: Arc<Mutex<Option<HashMap<PathBuf, bool>>>>,
    dirty: Arc<Mutex<HashSet<PathBuf>>>,
}
impl DirectoryWatcher {
    pub fn new() -> std::io::Result<Self> {
        let (commands, input) = mpsc::sync_channel::<()>(1);
        let desired = Arc::new(Mutex::new(None::<HashMap<PathBuf, bool>>));
        let latest = desired.clone();
        let dirty = Arc::new(Mutex::new(HashSet::new()));
        let output = dirty.clone();
        std::thread::Builder::new()
            .name("kova-directory-watch".into())
            .spawn(move || {
                let mut watches: HashMap<PathBuf, Watch> = HashMap::new();
                loop {
                    match input.recv_timeout(TICK) {
                        Ok(()) => {
                            let Some(paths) =
                                latest.lock().unwrap_or_else(|e| e.into_inner()).take()
                            else {
                                continue;
                            };
                            watches.retain(|path, watch| paths.get(path) == Some(&watch.recursive));
                            output
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .retain(|p| paths.contains_key(p));
                            for (path, recursive) in paths {
                                watches.entry(path).or_insert_with(|| Watch {
                                    recursive,
                                    handle: None,
                                    retry: Instant::now(),
                                    burst: Burst::default(),
                                });
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    for (path, watch) in &mut watches {
                        let now = Instant::now();
                        if watch.handle.is_none() && now >= watch.retry {
                            let wide: Vec<u16> =
                                path.as_os_str().encode_wide().chain(Some(0)).collect();
                            // SAFETY: wide is NUL terminated and lives throughout the call.
                            // Subtree monitoring is reserved for recursive search/sizes.
                            watch.handle = unsafe {
                                FindFirstChangeNotificationW(
                                    PCWSTR(wide.as_ptr()),
                                    watch.recursive,
                                    FILE_NOTIFY_CHANGE_FILE_NAME
                                        | FILE_NOTIFY_CHANGE_DIR_NAME
                                        | FILE_NOTIFY_CHANGE_ATTRIBUTES
                                        | FILE_NOTIFY_CHANGE_SIZE
                                        | FILE_NOTIFY_CHANGE_LAST_WRITE
                                        | FILE_NOTIFY_CHANGE_CREATION,
                                )
                            }
                            .ok()
                            .map(Handle);
                            watch.retry = now + RETRY;
                            // Reconcile the gap before registration, or periodically retry
                            // unavailable/unsupported directories without blocking the UI.
                            watch.burst.changed(now);
                        }
                        if let Some(handle) = &watch.handle {
                            // SAFETY: owned live notification handle, nonblocking wait.
                            let result = unsafe { WaitForSingleObject(handle.0, 0) };
                            if result == WAIT_OBJECT_0 {
                                watch.burst.changed(now);
                                // Re-arm immediately, before enumeration, so writes that
                                // arrive during a refresh produce another notification.
                                if unsafe { FindNextChangeNotification(handle.0) }.is_err() {
                                    watch.handle = None;
                                    watch.retry = now + RETRY;
                                }
                            } else if result != WAIT_TIMEOUT {
                                watch.handle = None;
                                watch.retry = now + RETRY;
                                watch.burst.changed(now);
                            }
                        }
                        if watch.burst.ready(now) {
                            output
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(path.clone());
                            watch.burst = Burst::default();
                        }
                    }
                }
            })?;
        Ok(Self {
            commands,
            desired,
            dirty,
        })
    }
    pub fn set_paths(&self, paths: HashMap<PathBuf, bool>) {
        *self.desired.lock().unwrap_or_else(|e| e.into_inner()) = Some(paths);
        let _ = self.commands.try_send(());
    }
    pub fn take_changes(&self) -> HashSet<PathBuf> {
        std::mem::take(&mut *self.dirty.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notifications_coalesce_without_starving_continuous_writes() {
        let start = Instant::now();
        let mut burst = Burst::default();
        burst.changed(start);
        burst.changed(start + Duration::from_millis(100));
        assert!(!burst.ready(start + Duration::from_millis(200)));
        assert!(burst.ready(start + Duration::from_millis(250)));
        for i in 2..=6 {
            burst.changed(start + Duration::from_millis(i * 100));
        }
        assert!(burst.ready(start + MAX_DELAY));
    }

    #[test]
    fn real_notifications_follow_external_writes_rename_and_delete() {
        let root = std::env::temp_dir().join(format!(
            "kova-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let watcher = DirectoryWatcher::new().unwrap();
        watcher.set_paths(HashMap::from([(root.clone(), true)]));
        let wait = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if watcher.take_changes().contains(&root) {
                    break;
                }
                assert!(Instant::now() < deadline, "missing native notification");
                std::thread::sleep(TICK);
            }
        };
        wait(); // Registration acknowledged before changing the directory.
        let file = root.join("program.exe");
        std::fs::write(&file, b"first").unwrap();
        wait();
        std::fs::write(&file, b"changed executable contents").unwrap();
        wait();
        let renamed = root.join("renamed.exe");
        std::fs::rename(&file, &renamed).unwrap();
        wait();
        std::fs::remove_file(&renamed).unwrap();
        wait();
        let folder = root.join("Folder");
        std::fs::create_dir(&folder).unwrap();
        wait();
        std::fs::write(folder.join("child.txt"), b"new child").unwrap();
        wait();
        // Ordinary browsing must ignore unrelated deep activity, while a mode
        // switch to recursive search/sizes must observe it again.
        let deep = folder.join("Deep");
        std::fs::create_dir(&deep).unwrap();
        let nested = deep.join("log.txt");
        std::fs::write(&nested, b"initial").unwrap();
        wait();
        watcher.set_paths(HashMap::from([(root.clone(), false)]));
        wait();
        std::thread::sleep(QUIET + TICK);
        watcher.take_changes();
        std::fs::write(&nested, b"unrelated application log update").unwrap();
        std::thread::sleep(MAX_DELAY + QUIET);
        assert!(
            watcher.take_changes().is_empty(),
            "shallow browsing watched the whole subtree"
        );
        std::fs::write(root.join("direct.txt"), b"visible new file").unwrap();
        wait();
        watcher.set_paths(HashMap::from([(root.clone(), true)]));
        wait();
        std::fs::write(&nested, b"recursive update").unwrap();
        wait();
        watcher.set_paths(HashMap::new());
        drop(watcher);
        std::thread::sleep(Duration::from_millis(150));
        std::fs::remove_dir_all(root).unwrap();
    }
}
