//! One running overlay per user. A second launch hands its request to the first
//! through a socket in the runtime directory and exits, so `audeze-overlay
//! --toggle` bound to a compositor key shows or hides the panel of the running
//! instance.
use std::{
    io::{Read, Write},
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
    Primary(UnixListener),
    /// The request was delivered to the instance that is already running.
    Forwarded,
}

/// Where the socket lives: inside the sandbox's own runtime directory in a
/// Flatpak, else directly in the user's runtime directory.
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

/// Delivers `request` to a running instance, or becomes the running instance.
pub(super) fn acquire(path: &Path, request: Request) -> std::io::Result<Role> {
    for _ in 0..2 {
        if let Ok(mut stream) = UnixStream::connect(path) {
            stream.set_write_timeout(Some(Duration::from_millis(500)))?;
            stream.write_all(request.word().as_bytes())?;
            return Ok(Role::Forwarded);
        }
        // Nothing answered: the file is stale or absent. Another launch racing
        // us may bind first, which the second connection attempt catches.
        let _ = std::fs::remove_file(path);
        match UnixListener::bind(path) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                return Ok(Role::Primary(listener));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(
        "could not reach or become the running instance",
    ))
}

/// Reads the request a client sent, if it is a known one. Reads at most a few
/// bytes and never waits long, so a stalled client cannot hold up the window.
pub(super) fn read_request(mut stream: UnixStream) -> Option<Request> {
    stream.set_nonblocking(false).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok()?;
    let mut buffer = [0u8; 16];
    let count = stream.read(&mut buffer).ok()?;
    Request::parse(std::str::from_utf8(&buffer[..count]).ok()?)
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
        let Role::Primary(listener) = acquire(&path, Request::Open).unwrap() else {
            panic!("the first launch must become the running instance");
        };
        assert!(matches!(
            acquire(&path, Request::Toggle).unwrap(),
            Role::Forwarded
        ));
        // The listener is non-blocking; the request is already queued.
        let (stream, _) = loop {
            match listener.accept() {
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
