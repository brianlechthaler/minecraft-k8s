use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::error::{AppError, Result};

const SERVERDATA_AUTH: i32 = 3;
const SERVERDATA_AUTH_RESPONSE: i32 = 2;
const SERVERDATA_EXECCOMMAND: i32 = 2;
const SERVERDATA_RESPONSE_VALUE: i32 = 0;

/// Minimal Minecraft RCON client for server management commands.
pub struct RconClient {
    stream: TcpStream,
    request_id: i32,
}

impl RconClient {
    pub fn connect(host: &str, port: u16, password: &str, timeout: Duration) -> Result<Self> {
        if password.is_empty() {
            return Err(AppError::Rcon("password must not be empty".into()));
        }

        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| AppError::Rcon(format!("invalid address: {e}")))?;

        let stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| AppError::Rcon(format!("connect failed: {e}")))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| AppError::Rcon(e.to_string()))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| AppError::Rcon(e.to_string()))?;

        let mut client = Self {
            stream,
            request_id: 0,
        };
        client.authenticate(password)?;
        Ok(client)
    }

    fn authenticate(&mut self, password: &str) -> Result<()> {
        let id = self.next_id();
        write_packet(&mut self.stream, id, SERVERDATA_AUTH, password)?;

        let (response_id, response_type, _) = read_packet(&mut self.stream)?;
        if response_id != id || response_type != SERVERDATA_AUTH_RESPONSE {
            return Err(AppError::Rcon("authentication failed".into()));
        }

        Ok(())
    }

    pub fn command(&mut self, cmd: &str) -> Result<String> {
        if cmd.trim().is_empty() {
            return Err(AppError::Rcon("command must not be empty".into()));
        }

        let id = self.next_id();
        write_packet(&mut self.stream, id, SERVERDATA_EXECCOMMAND, cmd)?;

        let (_, response_type, body) = read_packet(&mut self.stream)?;
        if response_type != SERVERDATA_RESPONSE_VALUE {
            return Err(AppError::Rcon(format!(
                "unexpected response type: {response_type}"
            )));
        }
        Ok(body)
    }

    fn next_id(&mut self) -> i32 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }
}

pub fn write_packet(stream: &mut TcpStream, id: i32, packet_type: i32, body: &str) -> Result<()> {
    let body_bytes = body.as_bytes();
    let size = (4 + 4 + body_bytes.len() + 2) as i32;
    stream
        .write_all(&size.to_le_bytes())
        .map_err(|e| AppError::Rcon(format!("write failed: {e}")))?;
    stream
        .write_all(&id.to_le_bytes())
        .map_err(|e| AppError::Rcon(format!("write failed: {e}")))?;
    stream
        .write_all(&packet_type.to_le_bytes())
        .map_err(|e| AppError::Rcon(format!("write failed: {e}")))?;
    stream
        .write_all(body_bytes)
        .map_err(|e| AppError::Rcon(format!("write failed: {e}")))?;
    stream
        .write_all(&[0, 0])
        .map_err(|e| AppError::Rcon(format!("write failed: {e}")))?;
    Ok(())
}

pub fn read_packet(stream: &mut TcpStream) -> Result<(i32, i32, String)> {
    let mut size_buf = [0u8; 4];
    stream
        .read_exact(&mut size_buf)
        .map_err(|e| AppError::Rcon(format!("read failed: {e}")))?;
    let size = i32::from_le_bytes(size_buf);
    if size < 10 {
        return Err(AppError::Rcon(format!("invalid packet size: {size}")));
    }

    let payload_len = size as usize;
    let mut payload = vec![0u8; payload_len];
    stream
        .read_exact(&mut payload)
        .map_err(|e| AppError::Rcon(format!("read failed: {e}")))?;

    let id = i32::from_le_bytes(payload[0..4].try_into().unwrap());
    let packet_type = i32::from_le_bytes(payload[4..8].try_into().unwrap());
    let body = String::from_utf8_lossy(&payload[8..payload.len().saturating_sub(2)]).into_owned();
    Ok((id, packet_type, body))
}

/// Parse player names from the `list` command response.
pub fn parse_player_list(response: &str) -> Vec<String> {
    let marker = ": ";
    let Some(idx) = response.find(marker) else {
        return Vec::new();
    };

    let players = response[idx + marker.len()..].trim();
    if players.is_empty() || players.eq_ignore_ascii_case("There are 0 of a max of 0 players online:") {
        return Vec::new();
    }

    players
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn spawn_rcon_server(_password: &str, command_response: &str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let command_response = command_response.to_string();

        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };

            let Ok((auth_id, _, _)) = read_packet(&mut stream) else {
                return;
            };
            let _ = write_packet(&mut stream, auth_id, SERVERDATA_AUTH_RESPONSE, "");

            let Ok((cmd_id, _, _)) = read_packet(&mut stream) else {
                return;
            };
            let _ = write_packet(&mut stream, cmd_id, SERVERDATA_RESPONSE_VALUE, &command_response);
        });

        thread::sleep(Duration::from_millis(20));
        port
    }

    #[test]
    fn connect_and_command() {
        let port = spawn_rcon_server("secret", "There are 2 of a max of 20 players online: steve, alex");
        let mut client = RconClient::connect("127.0.0.1", port, "secret", Duration::from_secs(2)).unwrap();
        let response = client.command("list").unwrap();
        assert!(response.contains("steve"));
    }

    #[test]
    fn connect_rejects_empty_password() {
        let err = RconClient::connect("127.0.0.1", 25575, "", Duration::from_millis(100));
        assert!(matches!(err, Err(AppError::Rcon(_))));
    }

    #[test]
    fn connect_rejects_bad_host() {
        let err =
            RconClient::connect("not-a-host", 25575, "pw", Duration::from_millis(100));
        assert!(matches!(err, Err(AppError::Rcon(_))));
    }

    #[test]
    fn command_rejects_empty() {
        let port = spawn_rcon_server("secret", "ok");
        let mut client = RconClient::connect("127.0.0.1", port, "secret", Duration::from_secs(2)).unwrap();
        assert!(client.command("  ").is_err());
    }

    #[test]
    fn parse_player_list_variants() {
        assert!(parse_player_list("no players here").is_empty());
        assert!(parse_player_list("There are 0 of a max of 20 players online:").is_empty());
        assert!(parse_player_list("Players: ").is_empty());
        assert!(parse_player_list("There are 0 of a max of 0 players online:").is_empty());
        let players = parse_player_list("There are 2 of a max of 20 players online: steve, alex");
        assert_eq!(players, vec!["steve", "alex"]);
    }

    #[test]
    fn read_packet_rejects_short_size() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.write_all(&5i32.to_le_bytes());
        });
        thread::sleep(Duration::from_millis(20));
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        assert!(read_packet(&mut stream).is_err());
    }

    #[test]
    fn auth_failure_on_mismatched_id() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = write_packet(&mut stream, 999, SERVERDATA_AUTH_RESPONSE, "");
        });
        thread::sleep(Duration::from_millis(20));
        let err = RconClient::connect("127.0.0.1", port, "pw", Duration::from_secs(1));
        assert!(matches!(err, Err(AppError::Rcon(_))));
    }

    #[test]
    fn command_rejects_unexpected_response_type() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let Ok((auth_id, _, _)) = read_packet(&mut stream) else {
                return;
            };
            let _ = write_packet(&mut stream, auth_id, SERVERDATA_AUTH_RESPONSE, "");
            let Ok((cmd_id, _, _)) = read_packet(&mut stream) else {
                return;
            };
            let _ = write_packet(&mut stream, cmd_id, SERVERDATA_AUTH, "bad");
        });
        thread::sleep(Duration::from_millis(20));
        let mut client = RconClient::connect("127.0.0.1", port, "secret", Duration::from_secs(2)).unwrap();
        assert!(client.command("list").is_err());
    }

    #[test]
    fn write_packet_fails_on_shutdown_stream() {
        use std::net::Shutdown;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let _ = listener.accept();
        });
        thread::sleep(Duration::from_millis(20));
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        assert!(write_packet(&mut stream, 1, SERVERDATA_AUTH, "x").is_err());
    }

    #[test]
    fn read_packet_fails_on_short_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.write_all(&10i32.to_le_bytes());
        });
        thread::sleep(Duration::from_millis(20));
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        assert!(read_packet(&mut stream).is_err());
    }
}
