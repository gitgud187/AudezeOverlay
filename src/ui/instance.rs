//! One running overlay per user. A second launch hands its request to the first
//! through a socket in the runtime directory and exits, so `audeze-overlay
//! --toggle` bound to a compositor key shows or hides the panel of the running
//! instance.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Request {
    /// Show the panel (what launching the application again does).
    Open,
    /// Show the panel if hidden, hide it if shown.
    Toggle,
}
impl Request {
    fn word(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Toggle => "toggle",
        }
    }
    fn parse(word: &str) -> Option<Self> {
        match word.trim() {
            "open" => Some(Self::Open),
            "toggle" => Some(Self::Toggle),
            _ => None,
        }
    }
}

pub(super) enum Role {
    /// This process is the running instance and listens for requests.
    Primary(Instance),
    /// The request was delivered to the instance that is already running.
    Forwarded,
}

/// Keep the advisory lock for the listener's entire lifetime. The lock file is
/// never unlinked: replacing its inode would let simultaneous owners through.
pub(super) struct Instance {
    pub listener: UnixListener,
    _lock: File,
    path: PathBuf,
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Where the socket lives: inside the sandbox's own runtime directory in a
/// Flatpak, else directly in the user's runtime directory. macOS uses the
/// Application Support directory; its 104-byte `sun_path` limit means a very
/// long home path fails startup rather than silently allowing two instances.
#[cfg(target_os = "linux")]
pub(super) fn socket_path() -> Option<PathBuf> {
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    if !runtime.is_absolute() {
        return None;
    }
    Some(match std::env::var_os("FLATPAK_ID") {
        Some(id) => runtime.join("app").join(id).join("audeze-overlay.sock"),
        None => runtime.join("audeze-overlay.sock"),
    })
}

#[cfg(target_os = "macos")]
pub(super) fn socket_path() -> Option<PathBuf> {
    let settings = audeze_overlay::config::Config::path();
    if !settings.is_absolute() {
        return None;
    }
    Some(settings.parent()?.join("overlay.sock"))
}

#[cfg(target_os = "linux")]
fn try_lock(file: &File) -> std::io::Result<bool> {
    match rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// `rustix` is a Linux-only dependency, so macOS calls `flock` directly.
#[cfg(target_os = "macos")]
fn try_lock(file: &File) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;
    extern "C" {
        fn flock(fd: std::os::fd::RawFd, operation: i32) -> i32;
    }
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    // SAFETY: `flock` only reads the descriptor, which `file` keeps open.
    if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error)
    }
}

/// Delivers `request` to a running instance, or becomes the running instance.
pub(super) fn acquire(path: &Path, request: Request) -> std::io::Result<Role> {
    // Application Support may not exist before the first run.
    #[cfg(target_os = "macos")]
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path.with_extension("lock"))?;
    let owns_lock = try_lock(&lock)?;
    // The winner can hold the lock briefly before binding its socket. A bounded
    // retry forwards to it instead of removing a live socket or starting USB twice.
    for attempt in 0..20 {
        if let Ok(mut stream) = UnixStream::connect(path) {
            stream.set_write_timeout(Some(Duration::from_millis(500)))?;
            stream.write_all(request.word().as_bytes())?;
            return Ok(Role::Forwarded);
        }
        if owns_lock {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let listener = UnixListener::bind(path)?;
            let instance = Instance {
                listener,
                _lock: lock,
                path: path.to_owned(),
            };
            instance.listener.set_nonblocking(true)?;
            return Ok(Role::Primary(instance));
        }
        if attempt < 19 {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Err(std::io::Error::other(
        "could not reach or become the running instance",
    ))
}

/// Reads the request a client sent, if it is a known one. Reads at most a few
/// bytes and never waits long, so a stalled client cannot hold up the window.
#[cfg(target_os = "linux")]
pub(super) fn read_request(mut stream: UnixStream) -> Option<Request> {
    stream.set_nonblocking(false).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok()?;
    let mut buffer = [0u8; 16];
    let count = stream.read(&mut buffer).ok()?;
    Request::parse(std::str::from_utf8(&buffer[..count]).ok()?)
}

/// macOS rejects `SO_RCVTIMEO` on Unix sockets (`EINVAL`), so poll instead.
#[cfg(target_os = "macos")]
pub(super) fn read_request(mut stream: UnixStream) -> Option<Request> {
    stream.set_nonblocking(true).ok()?;
    let mut buffer = [0u8; 16];
    let mut waited = 0;
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(count) => return Request::parse(std::str::from_utf8(&buffer[..count]).ok()?),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if waited >= 100 {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
                waited += 5;
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_socket(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("audeze-instance-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("overlay.sock")
    }

    #[test]
    fn the_first_launch_runs_and_a_second_hands_over_its_request() {
        let path = temp_socket("handover");
        let Role::Primary(instance) = acquire(&path, Request::Open).unwrap() else {
            panic!("the first launch must become the running instance");
        };
        assert!(matches!(
            acquire(&path, Request::Toggle).unwrap(),
            Role::Forwarded
        ));
        // The listener is non-blocking; the request is already queued.
        let (stream, _) = loop {
            match instance.listener.accept() {
                Ok(connection) => break connection,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("{e}"),
            }
        };
        assert_eq!(read_request(stream), Some(Request::Toggle));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn concurrent_launches_have_exactly_one_owner() {
        let path = temp_socket("race");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let role = acquire(&path, Request::Open).unwrap();
                    barrier.wait(); // Retain the winner until all clients have connected.
                    role
                })
            })
            .collect();
        let roles: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(
            roles
                .iter()
                .filter(|r| matches!(r, Role::Primary(_)))
                .count(),
            1
        );
        assert!(path.exists());
        drop(roles);
        assert!(!path.exists());
        assert!(matches!(
            acquire(&path, Request::Open).unwrap(),
            Role::Primary(_)
        ));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_stale_socket_file_is_replaced_not_mistaken_for_a_running_instance() {
        let path = temp_socket("stale");
        // A leftover file nobody listens on, as after a crash.
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        assert!(matches!(
            acquire(&path, Request::Open).unwrap(),
            Role::Primary(_)
        ));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unknown_or_oversized_requests_are_ignored() {
        for (text, expected) in [
            ("open", Some(Request::Open)),
            ("toggle\n", Some(Request::Toggle)),
            ("rm -rf", None),
            ("", None),
        ] {
            let (mut client, server) = UnixStream::pair().unwrap();
            client.write_all(text.as_bytes()).unwrap();
            drop(client);
            assert_eq!(read_request(server), expected, "{text:?}");
        }
    }
}
