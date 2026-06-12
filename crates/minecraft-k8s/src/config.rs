use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

/// Supported mod loader backends for the Minecraft server container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModLoader {
    Forge,
    Fabric,
    Quilt,
    Paper,
    Vanilla,
}

impl ModLoader {
    pub fn as_env_value(self) -> &'static str {
        match self {
            Self::Forge => "FORGE",
            Self::Fabric => "FABRIC",
            Self::Quilt => "QUILT",
            Self::Paper => "PAPER",
            Self::Vanilla => "VANILLA",
        }
    }

    pub fn supports_mods(self) -> bool {
        matches!(self, Self::Forge | Self::Fabric | Self::Quilt)
    }
}

impl std::str::FromStr for ModLoader {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "forge" => Ok(Self::Forge),
            "fabric" => Ok(Self::Fabric),
            "quilt" => Ok(Self::Quilt),
            "paper" => Ok(Self::Paper),
            "vanilla" => Ok(Self::Vanilla),
            other => Err(AppError::Config(format!(
                "unknown mod loader: {other}"
            ))),
        }
    }
}

/// Top-level server configuration consumed by the CLI and manifest renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub namespace: String,
    pub minecraft_version: String,
    pub mod_loader: ModLoader,
    #[serde(default)]
    pub forge_version: Option<String>,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    #[serde(default = "default_storage")]
    pub storage_size: String,
    #[serde(default = "default_true")]
    pub eula: bool,
    #[serde(default = "default_max_players")]
    pub max_players: u32,
    #[serde(default = "default_motd")]
    pub motd: String,
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default = "default_image_tag")]
    pub image_tag: String,
    #[serde(default)]
    pub modpack_url: Option<String>,
    #[serde(default)]
    pub extra_env: Vec<EnvVar>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

fn default_memory() -> String {
    "4G".into()
}

fn default_port() -> u16 {
    25565
}

fn default_replicas() -> u32 {
    1
}

fn default_storage() -> String {
    "20Gi".into()
}

fn default_true() -> bool {
    true
}

fn default_max_players() -> u32 {
    20
}

fn default_motd() -> String {
    "Modded Minecraft on Kubernetes".into()
}

fn default_image() -> String {
    "ghcr.io/brianlechthaler/minecraft-k8s-server".into()
}

fn default_image_tag() -> String {
    "latest".into()
}

impl ServerConfig {
    pub fn from_toml(content: &str) -> Result<Self> {
        toml::from_str(content).map_err(|e| AppError::Config(e.to_string()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(AppError::Config("name must not be empty".into()));
        }
        if self.namespace.is_empty() {
            return Err(AppError::Config("namespace must not be empty".into()));
        }
        if self.minecraft_version.is_empty() {
            return Err(AppError::Config(
                "minecraft_version must not be empty".into(),
            ));
        }
        if !self.eula {
            return Err(AppError::EulaNotAccepted);
        }
        if self.replicas != 1 {
            return Err(AppError::Config(
                "replicas must be 1 for stateful Minecraft servers".into(),
            ));
        }
        if self.port == 0 {
            return Err(AppError::Config("port must be greater than 0".into()));
        }
        if self.mod_loader == ModLoader::Forge && self.forge_version.is_none() {
            return Err(AppError::Config(
                "forge_version is required when mod_loader is forge".into(),
            ));
        }
        Ok(())
    }

    pub fn full_image(&self) -> String {
        format!("{}:{}", self.image, self.image_tag)
    }

    pub fn container_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("EULA".into(), "TRUE".into()),
            ("TYPE".into(), self.mod_loader.as_env_value().into()),
            ("VERSION".into(), self.minecraft_version.clone()),
            ("MEMORY".into(), self.memory.clone()),
            ("MAX_PLAYERS".into(), self.max_players.to_string()),
            ("MOTD".into(), self.motd.clone()),
            ("ENABLE_RCON".into(), "true".into()),
            ("RCON_PASSWORD".into(), "minecraft-k8s-rcon".into()),
            ("RCON_PORT".into(), "25575".into()),
            ("USE_AIKAR_FLAGS".into(), "true".into()),
            ("SYNC_SKIP_NEWER_IN_DESTINATION".into(), "false".into()),
        ];

        if let Some(forge) = &self.forge_version {
            env.push(("FORGE_VERSION".into(), forge.clone()));
        }
        if let Some(url) = &self.modpack_url {
            env.push(("MODPACK".into(), url.clone()));
        }

        for extra in &self.extra_env {
            env.push((extra.name.clone(), extra.value.clone()));
        }

        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_FORGE: &str = r#"
name = "survival"
namespace = "minecraft"
minecraft_version = "1.20.1"
mod_loader = "forge"
forge_version = "47.2.0"
eula = true
"#;

    #[test]
    fn parse_valid_forge_config() {
        let cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        assert_eq!(cfg.name, "survival");
        assert_eq!(cfg.mod_loader, ModLoader::Forge);
        assert_eq!(cfg.forge_version.as_deref(), Some("47.2.0"));
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn parse_invalid_toml() {
        let err = ServerConfig::from_toml("name = [").unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn validate_rejects_multiple_replicas() {
        let mut cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.replicas = 2;
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn validate_requires_eula() {
        let mut cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.eula = false;
        assert_eq!(cfg.validate().unwrap_err(), AppError::EulaNotAccepted);
    }

    #[test]
    fn validate_requires_forge_version() {
        let toml = r#"
name = "x"
namespace = "minecraft"
minecraft_version = "1.20.1"
mod_loader = "forge"
eula = true
"#;
        let cfg = ServerConfig::from_toml(toml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn mod_loader_from_str() {
        assert_eq!("forge".parse::<ModLoader>().unwrap(), ModLoader::Forge);
        assert_eq!("fabric".parse::<ModLoader>().unwrap(), ModLoader::Fabric);
        assert!("unknown".parse::<ModLoader>().is_err());
    }

    #[test]
    fn mod_loader_all_env_values() {
        assert_eq!(ModLoader::Forge.as_env_value(), "FORGE");
        assert_eq!(ModLoader::Fabric.as_env_value(), "FABRIC");
        assert_eq!(ModLoader::Quilt.as_env_value(), "QUILT");
        assert_eq!(ModLoader::Paper.as_env_value(), "PAPER");
        assert_eq!(ModLoader::Vanilla.as_env_value(), "VANILLA");
        assert!(ModLoader::Forge.supports_mods());
        assert!(!ModLoader::Vanilla.supports_mods());
    }

    #[test]
    fn validate_rejects_empty_fields() {
        let mut cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.name = String::new();
        assert!(cfg.validate().is_err());

        cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.namespace = String::new();
        assert!(cfg.validate().is_err());

        cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.minecraft_version = String::new();
        assert!(cfg.validate().is_err());

        cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.port = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn container_env_includes_forge_and_modpack() {
        let mut cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        cfg.modpack_url = Some("https://example.com/modpack.zip".into());
        let env = cfg.container_env();
        assert!(env.iter().any(|(k, v)| k == "FORGE_VERSION" && v == "47.2.0"));
        assert!(env.iter().any(|(k, _)| k == "MODPACK"));
        assert!(env.iter().any(|(k, v)| k == "EULA" && v == "TRUE"));
    }

    #[test]
    fn full_image_and_defaults() {
        let cfg = ServerConfig::from_toml(VALID_FORGE).unwrap();
        assert!(cfg.full_image().contains("minecraft-k8s-server"));
        assert_eq!(cfg.port, 25565);
        assert_eq!(cfg.storage_size, "20Gi");
    }

    #[test]
    fn extra_env_merged() {
        let toml = r#"
name = "x"
namespace = "minecraft"
minecraft_version = "1.20.1"
mod_loader = "fabric"
eula = true

[[extra_env]]
name = "FABRIC_LOADER_VERSION"
value = "0.15.11"
"#;
        let cfg = ServerConfig::from_toml(toml).unwrap();
        assert!(cfg.validate().is_ok());
        let env = cfg.container_env();
        assert!(env.iter().any(|(k, v)| k == "FABRIC_LOADER_VERSION" && v == "0.15.11"));
    }
}
