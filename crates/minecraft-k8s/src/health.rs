use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::error::{AppError, Result};

/// Checks whether a TCP port is accepting connections (Minecraft server up).
pub fn check_port(host: &str, port: u16, timeout: Duration) -> Result<bool> {
    if port == 0 {
        return Err(AppError::Config("port must be greater than 0".into()));
    }

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| AppError::Config(format!("invalid address: {e}")))?;

    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Probe exit code helper for Kubernetes exec probes.
pub fn probe_exit_code(host: &str, port: u16, timeout: Duration) -> i32 {
    match check_port(host, port, timeout) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(_) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn check_port_rejects_zero() {
        assert!(check_port("127.0.0.1", 0, Duration::from_millis(10)).is_err());
    }

    #[test]
    fn check_port_rejects_bad_host() {
        assert!(check_port("not-a-host", 25565, Duration::from_millis(10)).is_err());
    }

    #[test]
    fn check_port_open_and_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(b"ok");
                let mut buf = [0u8; 16];
                let _ = stream.read(&mut buf);
            }
        });

        thread::sleep(Duration::from_millis(50));
        assert!(check_port("127.0.0.1", port, Duration::from_secs(1)).unwrap());

        let free = port.saturating_add(1);
        assert!(!check_port("127.0.0.1", free, Duration::from_millis(200)).unwrap());
    }

    #[test]
    fn probe_exit_codes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let _ = listener.accept();
        });
        thread::sleep(Duration::from_millis(50));
        assert_eq!(probe_exit_code("127.0.0.1", port, Duration::from_secs(1)), 0);
        assert_eq!(
            probe_exit_code("127.0.0.1", port.saturating_add(1), Duration::from_millis(100)),
            1
        );
        assert_eq!(probe_exit_code("bad", port, Duration::from_millis(10)), 2);
    }
}
