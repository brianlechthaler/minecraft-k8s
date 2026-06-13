use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::error::{AppError, Result};
use crate::health;
use crate::rcon::{self, RconClient};

const INDEX_HTML: &str = include_str!("../static/index.html");

/// Runtime configuration for the web dashboard.
#[derive(Debug, Clone)]
pub struct DashboardConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub minecraft_host: String,
    pub minecraft_port: u16,
    pub rcon_host: String,
    pub rcon_port: u16,
    pub rcon_password: String,
    pub max_players: u32,
    pub motd: String,
    pub probe_timeout: Duration,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<DashboardConfig>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StatusResponse {
    pub online: bool,
    pub players: Vec<String>,
    pub player_count: usize,
    pub max_players: u32,
    pub motd: String,
    pub rcon_available: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CommandResponse {
    pub output: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/command", post(run_command))
        .with_state(state)
}

pub async fn serve(config: DashboardConfig) -> Result<()> {
    serve_with_shutdown(config, std::future::pending()).await
}

pub(crate) async fn serve_with_shutdown(
    config: DashboardConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", config.bind_host, config.bind_port))
        .await
        .map_err(|e| AppError::Dashboard(format!("bind failed: {e}")))?;

    let state = AppState {
        config: Arc::new(config),
    };
    let app = router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| AppError::Dashboard(format!("server error: {e}")))?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(collect_status(&state.config))
}

async fn run_command(
    State(state): State<AppState>,
    Json(body): Json<CommandRequest>,
) -> Response {
    match execute_command(&state.config, &body.command) {
        Ok(output) => (StatusCode::OK, Json(CommandResponse { output })).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub fn collect_status(config: &DashboardConfig) -> StatusResponse {
    let online = health::check_port(
        &config.minecraft_host,
        config.minecraft_port,
        config.probe_timeout,
    )
    .unwrap_or(false);

    let mut players = Vec::new();
    let mut rcon_available = false;

    if online {
        if let Ok(mut client) = RconClient::connect(
            &config.rcon_host,
            config.rcon_port,
            &config.rcon_password,
            config.probe_timeout,
        ) {
            rcon_available = true;
            if let Ok(response) = client.command("list") {
                players = rcon::parse_player_list(&response);
            }
        }
    }

    StatusResponse {
        online,
        players: players.clone(),
        player_count: players.len(),
        max_players: config.max_players,
        motd: config.motd.clone(),
        rcon_available,
    }
}

pub fn execute_command(config: &DashboardConfig, command: &str) -> Result<String> {
    let command = command.trim();
    if command.is_empty() {
        return Err(AppError::Dashboard("command must not be empty".into()));
    }

    if !is_allowed_command(command) {
        return Err(AppError::Dashboard(format!(
            "command not allowed: {command}"
        )));
    }

    let mut client = RconClient::connect(
        &config.rcon_host,
        config.rcon_port,
        &config.rcon_password,
        config.probe_timeout,
    )?;
    client.command(command)
}

pub fn is_allowed_command(command: &str) -> bool {
    let head = command.split_whitespace().next().unwrap_or("");
    matches!(
        head.to_ascii_lowercase().as_str(),
        "list" | "say" | "stop" | "save-all" | "whitelist" | "kick" | "ban"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use std::net::TcpListener;
    use std::thread;
    use tower::ServiceExt;

    fn test_config(minecraft_port: u16, rcon_port: u16) -> DashboardConfig {
        DashboardConfig {
            bind_host: "127.0.0.1".into(),
            bind_port: 0,
            minecraft_host: "127.0.0.1".into(),
            minecraft_port,
            rcon_host: "127.0.0.1".into(),
            rcon_port,
            rcon_password: "secret".into(),
            max_players: 20,
            motd: "Test MOTD".into(),
            probe_timeout: Duration::from_secs(2),
        }
    }

    fn spawn_mc_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let _ = listener.accept();
        });
        thread::sleep(Duration::from_millis(20));
        port
    }

    fn spawn_rcon_port(response: &str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let response = response.to_string();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let Ok((auth_id, _, _)) = rcon::read_packet(&mut stream) else {
                return;
            };
            let _ = rcon::write_packet(&mut stream, auth_id, 2, "");
            let Ok((cmd_id, _, _)) = rcon::read_packet(&mut stream) else {
                return;
            };
            let _ = rcon::write_packet(&mut stream, cmd_id, 0, &response);
        });
        thread::sleep(Duration::from_millis(20));
        port
    }

    #[tokio::test]
    async fn index_and_status_routes() {
        let mc_port = spawn_mc_port();
        let rcon_port = spawn_rcon_port("There are 1 of a max of 20 players online: steve");
        let config = test_config(mc_port, rcon_port);
        let state = AppState {
            config: Arc::new(config),
        };
        let app = router(state);

        let response = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(Request::get("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn collect_status_offline() {
        let config = test_config(1, 1);
        let status = collect_status(&config);
        assert!(!status.online);
        assert!(!status.rcon_available);
    }

    #[test]
    fn collect_status_online_with_players() {
        let mc_port = spawn_mc_port();
        let rcon_port = spawn_rcon_port("There are 2 of a max of 20 players online: a, b");
        let config = test_config(mc_port, rcon_port);
        let status = collect_status(&config);
        assert!(status.online);
        assert!(status.rcon_available);
        assert_eq!(status.player_count, 2);
    }

    #[test]
    fn command_allowlist() {
        assert!(is_allowed_command("list"));
        assert!(is_allowed_command("say hello"));
        assert!(!is_allowed_command("op steve"));
        assert!(!is_allowed_command(""));
    }

    #[test]
    fn execute_command_rejects_disallowed() {
        let config = test_config(25565, 25575);
        let err = execute_command(&config, "op x").unwrap_err();
        assert!(matches!(err, AppError::Dashboard(_)));
    }

    #[test]
    fn execute_command_rejects_empty() {
        let config = test_config(25565, 25575);
        let err = execute_command(&config, "  ").unwrap_err();
        assert!(matches!(err, AppError::Dashboard(_)));
    }

    #[test]
    fn execute_command_runs_list() {
        let mc_port = spawn_mc_port();
        let rcon_port = spawn_rcon_port("There are 0 of a max of 20 players online:");
        let config = test_config(mc_port, rcon_port);
        let output = execute_command(&config, "list").unwrap();
        assert!(output.contains("players online"));
    }

    #[tokio::test]
    async fn run_command_handler() {
        let mc_port = spawn_mc_port();
        let rcon_port = spawn_rcon_port("There are 0 of a max of 20 players online:");
        let config = test_config(mc_port, rcon_port);
        let state = AppState {
            config: Arc::new(config),
        };
        let app = router(state);

        let response = app
            .clone()
            .oneshot(
                Request::post("/api/command")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"command":"list"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::post("/api/command")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"command":"op bad"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn serve_binds_and_accepts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let config = DashboardConfig {
            bind_host: "127.0.0.1".into(),
            bind_port: port,
            minecraft_host: "127.0.0.1".into(),
            minecraft_port: 1,
            rcon_host: "127.0.0.1".into(),
            rcon_port: 1,
            rcon_password: "x".into(),
            max_players: 10,
            motd: "m".into(),
            probe_timeout: Duration::from_millis(100),
        };

        let handle = tokio::spawn(async move {
            let _ = serve(config).await;
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        let connected = tokio::task::spawn_blocking(move || {
            std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok()
        })
        .await
        .unwrap();
        assert!(connected);
        handle.abort();
    }

    #[tokio::test]
    async fn serve_with_shutdown_exits_cleanly() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let config = DashboardConfig {
            bind_host: "127.0.0.1".into(),
            bind_port: port,
            minecraft_host: "127.0.0.1".into(),
            minecraft_port: 1,
            rcon_host: "127.0.0.1".into(),
            rcon_port: 1,
            rcon_password: "x".into(),
            max_players: 10,
            motd: "m".into(),
            probe_timeout: Duration::from_millis(100),
        };

        serve_with_shutdown(config, async {
            tokio::time::sleep(Duration::from_millis(20)).await;
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn serve_bind_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let config = DashboardConfig {
            bind_host: "127.0.0.1".into(),
            bind_port: port,
            minecraft_host: "127.0.0.1".into(),
            minecraft_port: 1,
            rcon_host: "127.0.0.1".into(),
            rcon_port: 1,
            rcon_password: "x".into(),
            max_players: 10,
            motd: "m".into(),
            probe_timeout: Duration::from_millis(100),
        };

        let err = serve(config).await.unwrap_err();
        assert!(matches!(err, AppError::Dashboard(_)));
        drop(listener);
    }
}
