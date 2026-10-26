//! Single running instance: `gigit <path>` forwards the path to an already open window.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("gigit.sock"),
        None => PathBuf::from(format!("/tmp/gigit-{}.sock", unsafe { libc::getuid() })),
    }
}

/// Sends `path` (NUL-terminated; empty to just raise the window) to a running instance.
/// Returns false when no instance is listening.
pub fn try_forward(sock: &Path, path: Option<&Path>) -> bool {
    let Ok(mut stream) = UnixStream::connect(sock) else {
        return false;
    };
    let mut msg = path.map(|p| p.as_os_str().as_bytes().to_vec()).unwrap_or_default();
    msg.push(0);
    stream.write_all(&msg).is_ok()
}

/// Listens for forwarded paths. Replaces a stale socket left by a crashed instance.
pub fn listen(sock: &Path, on_path: impl Fn(Option<PathBuf>) + Send + 'static) -> std::io::Result<()> {
    if sock.exists() {
        if UnixStream::connect(sock).is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "GiGit is already running"));
        }
        std::fs::remove_file(sock)?;
    }
    let listener = UnixListener::bind(sock)?;
    std::thread::Builder::new().name("gigit-ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut buf = Vec::new();
            if BufReader::new(stream).read_until(0, &mut buf).is_err() {
                continue;
            }
            if buf.last() == Some(&0) {
                buf.pop();
            }
            let path = (!buf.is_empty()).then(|| PathBuf::from(OsString::from_vec(buf)));
            on_path(path);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    #[test]
    fn forward_reaches_listener() {
        let d = tempfile::TempDir::new().unwrap();
        let sock = d.path().join("s.sock");
        let (tx, rx) = channel();
        listen(&sock, move |p| tx.send(p).unwrap()).unwrap();
        assert!(try_forward(&sock, Some(Path::new("/home/x/mon dépôt"))));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Some(PathBuf::from("/home/x/mon dépôt")));
        assert!(try_forward(&sock, None));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), None);
    }

    #[test]
    fn forward_without_listener_is_false() {
        let d = tempfile::TempDir::new().unwrap();
        assert!(!try_forward(&d.path().join("none.sock"), None));
    }

    #[test]
    fn stale_socket_is_replaced() {
        let d = tempfile::TempDir::new().unwrap();
        let sock = d.path().join("s.sock");
        drop(std::os::unix::net::UnixListener::bind(&sock).unwrap()); // leaves the file behind
        assert!(sock.exists());
        let (tx, rx) = channel();
        listen(&sock, move |p| tx.send(p).unwrap()).unwrap();
        assert!(try_forward(&sock, Some(Path::new("/a"))));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Some(PathBuf::from("/a")));
    }

    #[test]
    fn live_socket_is_not_stolen() {
        let d = tempfile::TempDir::new().unwrap();
        let sock = d.path().join("s.sock");
        listen(&sock, |_| {}).unwrap();
        assert!(listen(&sock, |_| {}).is_err());
    }
}
