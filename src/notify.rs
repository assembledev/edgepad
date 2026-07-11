use std::env;
use std::ffi::OsStr;
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub fn notify_ready(device_path: &Path) -> Result<bool, String> {
    let payload = ready_payload(device_path);
    let sent = notify(payload.as_bytes())?;
    if sent {
        env::remove_var("NOTIFY_SOCKET");
    }
    Ok(sent)
}

fn ready_payload(device_path: &Path) -> String {
    format!(
        "READY=1\nSTATUS=edgepad {} ready on {}\nMAINPID={}",
        env!("CARGO_PKG_VERSION"),
        device_path.display(),
        std::process::id()
    )
}

fn notify(payload: &[u8]) -> Result<bool, String> {
    let Some(socket) = env::var_os("NOTIFY_SOCKET") else {
        return Ok(false);
    };
    send_notification_to(&socket, payload)
        .map_err(|err| format!("failed to notify systemd that edgepad is ready: {err}"))?;
    Ok(true)
}

fn send_notification_to(socket: &OsStr, payload: &[u8]) -> io::Result<()> {
    let raw_fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if raw_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    let mut address = unsafe { mem::zeroed::<libc::sockaddr_un>() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let socket_bytes = socket.as_bytes();
    let path_offset = mem::size_of::<libc::sa_family_t>();
    let path_len = if let Some(abstract_name) = socket_bytes.strip_prefix(b"@") {
        if abstract_name.len() + 1 > address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "abstract NOTIFY_SOCKET path is too long",
            ));
        }
        for (destination, source) in address.sun_path[1..].iter_mut().zip(abstract_name) {
            *destination = *source as libc::c_char;
        }
        abstract_name.len() + 1
    } else {
        if socket_bytes.len() + 1 > address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "NOTIFY_SOCKET path is too long",
            ));
        }
        for (destination, source) in address.sun_path.iter_mut().zip(socket_bytes) {
            *destination = *source as libc::c_char;
        }
        socket_bytes.len() + 1
    };
    let address_len = (path_offset + path_len) as libc::socklen_t;

    let sent = unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            payload.as_ptr().cast(),
            payload.len(),
            libc::MSG_NOSIGNAL,
            (&address as *const libc::sockaddr_un).cast(),
            address_len,
        )
    };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }
    if sent as usize != payload.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "systemd readiness datagram was truncated",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixDatagram;
    use std::time::Duration;

    #[test]
    fn ready_payload_identifies_version_device_and_main_process() {
        let payload = ready_payload(Path::new("/dev/input/event7"));

        assert!(payload.starts_with("READY=1\n"));
        assert!(payload.contains(&format!("edgepad {} ready", env!("CARGO_PKG_VERSION"))));
        assert!(payload.contains("/dev/input/event7"));
        assert!(payload.contains(&format!("MAINPID={}", std::process::id())));
    }

    #[test]
    #[ignore = "requires permission to create Unix datagram sockets"]
    fn sends_readiness_payload_to_filesystem_notify_socket() {
        let socket_path = std::env::temp_dir().join(format!(
            "edgepad-notify-{}-{}.sock",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_file(&socket_path);
        let receiver = UnixDatagram::bind(&socket_path).expect("notify socket should bind");
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("read timeout should be configured");

        send_notification_to(socket_path.as_os_str(), b"READY=1\nSTATUS=test")
            .expect("notification should send");
        let mut buffer = [0_u8; 128];
        let received = receiver
            .recv(&mut buffer)
            .expect("notification should arrive");

        assert_eq!(&buffer[..received], b"READY=1\nSTATUS=test");
        std::fs::remove_file(socket_path).expect("notify socket should be removed");
    }
}
