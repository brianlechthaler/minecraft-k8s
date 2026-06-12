use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::error::{AppError, Result};
use crate::health;
use crate::k8s;
use crate::mods;
use crate::eula;
use crate::ServerConfig;

#[derive(Parser, Debug)]
#[command(
    name = "minecraft-k8s",
    about = "Tooling for running modded Minecraft servers on Kubernetes"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Validate a server configuration file
    Validate {
        #[arg(short, long)]
        config: PathBuf,
        #[arg(long)]
        mods_dir: Option<PathBuf>,
    },
    /// Render Kubernetes manifests from configuration
    Render {
        #[arg(short, long)]
        config: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Validate Kubernetes manifest YAML on disk
    CheckManifests {
        #[arg(short, long)]
        path: PathBuf,
    },
    /// TCP probe for Kubernetes health checks
    Probe {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, default_value_t = 25565)]
        port: u16,
        #[arg(long, default_value_t = 5)]
        timeout_secs: u64,
    },
    /// Write eula.txt when accepted
    WriteEula {
        #[arg(short, long)]
        output: PathBuf,
    },
}

pub fn entry() -> i32 {
    entry_from(std::env::args())
}

pub fn entry_from<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    match run_from(args) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            exit_code(&err)
        }
    }
}

pub fn run_from<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|e| AppError::Config(e.to_string()))?;
    dispatch(cli.command)
}

pub fn exit_code(err: &AppError) -> i32 {
    match err {
        AppError::EulaNotAccepted => 3,
        AppError::Config(_) => 4,
        AppError::InvalidMod { .. } => 5,
        AppError::Manifest(_) => 6,
        AppError::Io { .. } => 7,
        AppError::ProbeFailed(code) => *code,
    }
}

pub fn dispatch(command: Commands) -> Result<()> {
    match command {
        Commands::Validate { config, mods_dir } => cmd_validate(config, mods_dir),
        Commands::Render { config, output } => cmd_render(config, output),
        Commands::CheckManifests { path } => cmd_check_manifests(path),
        Commands::Probe {
            host,
            port,
            timeout_secs,
        } => cmd_probe(host, port, timeout_secs),
        Commands::WriteEula { output } => cmd_write_eula(output),
    }
}

pub fn cmd_validate(config: PathBuf, mods_dir: Option<PathBuf>) -> Result<()> {
    let content = read_to_string(&config)?;
    let cfg = ServerConfig::from_toml(&content)?;
    cfg.validate()?;

    if let Some(dir) = mods_dir {
        let found = mods::validate_mods_dir(&dir)?;
        println!("validated {} mod(s) in {}", found.len(), dir.display());
    }

    println!("configuration valid for server '{}'", cfg.name);
    Ok(())
}

pub fn cmd_render(config: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let content = read_to_string(&config)?;
    let cfg = ServerConfig::from_toml(&content)?;
    let yaml = k8s::render_manifests_yaml(&cfg)?;

    match output {
        Some(path) => {
            fs::write(&path, &yaml).map_err(|e| AppError::Io {
                path,
                message: e.to_string(),
            })?;
            println!("wrote manifests");
        }
        None => print!("{yaml}"),
    }

    Ok(())
}

pub fn cmd_check_manifests(path: PathBuf) -> Result<()> {
    let content = read_to_string(&path)?;
    let count = k8s::validate_manifest_yaml(&content)?;
    println!("validated {count} manifest document(s) in {}", path.display());
    Ok(())
}

pub fn cmd_probe(host: String, port: u16, timeout_secs: u64) -> Result<()> {
    let code = health::probe_exit_code(&host, port, Duration::from_secs(timeout_secs));
    if code != 0 {
        return Err(AppError::ProbeFailed(code));
    }
    Ok(())
}

pub fn cmd_write_eula(output: PathBuf) -> Result<()> {
    let text = eula::render_eula(true)?;
    fs::write(&output, text).map_err(|e| AppError::Io {
        path: output,
        message: e.to_string(),
    })?;
    Ok(())
}

pub fn read_to_string(path: &PathBuf) -> Result<String> {
    fs::read_to_string(path).map_err(|e| AppError::Io {
        path: path.clone(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    const SAMPLE: &str = r#"
name = "demo"
namespace = "minecraft"
minecraft_version = "1.20.1"
mod_loader = "forge"
forge_version = "47.2.0"
eula = true
"#;

    #[test]
    fn entry_delegates_to_env_args() {
        let _ = entry();
    }

    #[test]
    fn entry_success_and_failure() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();
        assert_eq!(
            entry_from([
                "minecraft-k8s",
                "validate",
                "--config",
                cfg.to_str().unwrap(),
            ]),
            0
        );
        assert_eq!(entry_from(["minecraft-k8s", "nope"]), 4);
    }

    #[test]
    fn exit_code_mapping() {
        assert_eq!(exit_code(&AppError::EulaNotAccepted), 3);
        assert_eq!(exit_code(&AppError::Config("x".into())), 4);
        assert_eq!(
            exit_code(&AppError::InvalidMod {
                path: "a".into(),
                reason: "b".into(),
            }),
            5
        );
        assert_eq!(exit_code(&AppError::Manifest("x".into())), 6);
        assert_eq!(
            exit_code(&AppError::Io {
                path: "a".into(),
                message: "b".into(),
            }),
            7
        );
        assert_eq!(exit_code(&AppError::ProbeFailed(1)), 1);
    }

    #[test]
    fn read_to_string_ok_and_err() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("cfg.toml");
        std::fs::write(&file, SAMPLE).unwrap();
        assert!(read_to_string(&file).unwrap().contains("demo"));
        assert!(read_to_string(&dir.path().join("missing.toml")).is_err());
    }

    #[test]
    fn run_from_validate_command() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();
        run_from([
            "minecraft-k8s",
            "validate",
            "--config",
            cfg.to_str().unwrap(),
        ])
        .unwrap();
    }

    #[test]
    fn run_from_rejects_invalid_cli() {
        assert!(run_from(["minecraft-k8s", "nope"]).is_err());
    }

    #[test]
    fn cmd_validate_and_render_flow() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();

        cmd_validate(cfg.clone(), None).unwrap();

        let out = dir.path().join("all.yaml");
        cmd_render(cfg, Some(out.clone())).unwrap();
        let yaml = std::fs::read_to_string(out).unwrap();
        assert!(yaml.contains("kind: Service"));
    }

    #[test]
    fn check_manifests_command() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();
        let out = dir.path().join("all.yaml");
        cmd_render(cfg, Some(out.clone())).unwrap();
        cmd_check_manifests(out).unwrap();
    }

    #[test]
    fn write_eula_io_error() {
        let dir = TempDir::new().unwrap();
        let err = cmd_write_eula(dir.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, AppError::Io { .. }));
    }


    #[test]
    fn write_eula_command() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("eula.txt");
        cmd_write_eula(path.clone()).unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("eula=true"));
    }

    #[test]
    fn cmd_validate_with_mods_dir() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();
        let mods = dir.path().join("mods");
        std::fs::create_dir(&mods).unwrap();
        std::fs::write(mods.join("test.jar"), b"j").unwrap();
        cmd_validate(cfg, Some(mods)).unwrap();
    }

    #[test]
    fn cmd_render_stdout() {
        let dir = TempDir::new().unwrap();
        let cfg_path = dir.path().join("server.toml");
        let mut f = std::fs::File::create(&cfg_path).unwrap();
        write!(f, "{SAMPLE}").unwrap();
        cmd_render(cfg_path, None).unwrap();
    }

    #[test]
    fn cmd_render_write_error() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();
        let err = cmd_render(cfg, Some(dir.path().to_path_buf())).unwrap_err();
        assert!(matches!(err, AppError::Io { .. }));
    }

    #[test]
    fn cmd_probe_success_and_failure() {
        use std::net::TcpListener;
        use std::thread;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let _ = listener.accept();
        });
        thread::sleep(std::time::Duration::from_millis(50));
        cmd_probe("127.0.0.1".into(), port, 1).unwrap();
        assert_eq!(
            cmd_probe("127.0.0.1".into(), port.saturating_add(1), 1).unwrap_err(),
            AppError::ProbeFailed(1)
        );
    }

    #[test]
    fn run_from_all_subcommands() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("server.toml");
        std::fs::write(&cfg, SAMPLE).unwrap();
        let out = dir.path().join("all.yaml");
        let eula = dir.path().join("eula.txt");

        run_from([
            "minecraft-k8s",
            "render",
            "--config",
            cfg.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .unwrap();

        run_from([
            "minecraft-k8s",
            "check-manifests",
            "--path",
            out.to_str().unwrap(),
        ])
        .unwrap();

        run_from([
            "minecraft-k8s",
            "write-eula",
            "--output",
            eula.to_str().unwrap(),
        ])
        .unwrap();

        run_from([
            "minecraft-k8s",
            "probe",
            "--host",
            "127.0.0.1",
            "--port",
            "9",
            "--timeout-secs",
            "1",
        ])
        .unwrap_err();
    }
}
