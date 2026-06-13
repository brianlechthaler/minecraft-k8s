use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::config::ServerConfig;
use crate::error::{AppError, Result};

const APP_LABEL: &str = "app.kubernetes.io/name";
const MANAGED_BY: &str = "app.kubernetes.io/managed-by";
const DEFAULT_TOOLS_IMAGE: &str = "ghcr.io/brianlechthaler/minecraft-k8s-tools";
const DEFAULT_RCON_PASSWORD: &str = "minecraft-k8s-rcon";

pub fn labels(cfg: &ServerConfig) -> BTreeMap<String, String> {
    BTreeMap::from([
        (APP_LABEL.into(), cfg.name.clone()),
        ("app.kubernetes.io/component".into(), "minecraft-server".into()),
        (MANAGED_BY.into(), "minecraft-k8s".into()),
    ])
}

pub fn render_namespace(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {
            "name": cfg.namespace,
            "labels": labels(cfg),
        }
    })
}

pub fn render_config_map(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": format!("{}-config", cfg.name),
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "data": {
            "server.properties": render_server_properties(cfg),
        }
    })
}

fn render_server_properties(cfg: &ServerConfig) -> String {
    format!(
        "motd={}\nmax-players={}\nserver-port={}\nonline-mode=true\nenable-rcon=true\n",
        cfg.motd, cfg.max_players, cfg.port
    )
}

pub fn render_pvc(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": format!("{}-data", cfg.name),
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "spec": {
            "accessModes": ["ReadWriteOnce"],
            "resources": {
                "requests": {
                    "storage": cfg.storage_size,
                }
            }
        }
    })
}

pub fn render_mods_pvc(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": format!("{}-mods", cfg.name),
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "spec": {
            "accessModes": ["ReadWriteOnce"],
            "resources": {
                "requests": {
                    "storage": "5Gi",
                }
            }
        }
    })
}

pub fn render_deployment(cfg: &ServerConfig) -> Value {
    let mut env: Vec<Value> = cfg
        .container_env()
        .into_iter()
        .filter(|(name, _)| name != "RCON_PASSWORD")
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect();

    env.push(json!({
        "name": "RCON_PASSWORD",
        "valueFrom": {
            "secretKeyRef": {
                "name": format!("{}-rcon", cfg.name),
                "key": "password",
            }
        }
    }));

    json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": cfg.name,
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "spec": {
            "replicas": cfg.replicas,
            "strategy": {
                "type": "Recreate",
            },
            "selector": {
                "matchLabels": {
                    APP_LABEL: cfg.name,
                }
            },
            "template": {
                "metadata": {
                    "labels": labels(cfg),
                },
                "spec": {
                    "securityContext": {
                        "fsGroup": 1000,
                        "runAsUser": 1000,
                        "runAsNonRoot": true,
                    },
                    "containers": [{
                        "name": "minecraft",
                        "image": cfg.full_image(),
                        "imagePullPolicy": "IfNotPresent",
                        "ports": [
                            { "name": "minecraft", "containerPort": cfg.port, "protocol": "TCP" },
                            { "name": "rcon", "containerPort": 25575, "protocol": "TCP" },
                        ],
                        "env": env,
                        "resources": {
                            "requests": {
                                "memory": "2Gi",
                                "cpu": "500m",
                            },
                            "limits": {
                                "memory": cfg.memory,
                                "cpu": "2",
                            }
                        },
                        "volumeMounts": [
                            { "name": "data", "mountPath": "/data" },
                            { "name": "mods", "mountPath": "/data/mods" },
                        ],
                        "readinessProbe": probe(cfg, "readiness"),
                        "livenessProbe": probe(cfg, "liveness"),
                    }],
                    "volumes": [
                        {
                            "name": "data",
                            "persistentVolumeClaim": {
                                "claimName": format!("{}-data", cfg.name),
                            }
                        },
                        {
                            "name": "mods",
                            "persistentVolumeClaim": {
                                "claimName": format!("{}-mods", cfg.name),
                            }
                        },
                    ]
                }
            }
        }
    })
}

fn probe(cfg: &ServerConfig, kind: &str) -> Value {
    let (initial, period, failure) = if kind == "liveness" {
        (120, 30, 6)
    } else {
        (60, 10, 12)
    };

    json!({
        "exec": {
            "command": [
                "/usr/local/bin/minecraft-k8s",
                "probe",
                "--port",
                cfg.port.to_string(),
            ]
        },
        "initialDelaySeconds": initial,
        "periodSeconds": period,
        "failureThreshold": failure,
        "timeoutSeconds": 5,
    })
}

pub fn render_service(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": format!("{}-mc", cfg.name),
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "spec": {
            "type": "LoadBalancer",
            "selector": {
                APP_LABEL: cfg.name,
            },
            "ports": [{
                "name": "minecraft",
                "port": cfg.port,
                "targetPort": "minecraft",
                "protocol": "TCP",
            }]
        }
    })
}

pub fn render_rcon_secret(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": format!("{}-rcon", cfg.name),
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "type": "Opaque",
        "stringData": {
            "password": DEFAULT_RCON_PASSWORD,
        }
    })
}

pub fn render_rcon_service(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": format!("{}-rcon", cfg.name),
            "namespace": cfg.namespace,
            "labels": labels(cfg),
        },
        "spec": {
            "type": "ClusterIP",
            "selector": {
                APP_LABEL: cfg.name,
            },
            "ports": [{
                "name": "rcon",
                "port": 25575,
                "targetPort": "rcon",
                "protocol": "TCP",
            }]
        }
    })
}

fn dashboard_labels(cfg: &ServerConfig) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            APP_LABEL.into(),
            format!("{}-dashboard", cfg.name),
        ),
        (
            "app.kubernetes.io/component".into(),
            "dashboard".into(),
        ),
        (MANAGED_BY.into(), "minecraft-k8s".into()),
    ])
}

pub fn render_dashboard_deployment(cfg: &ServerConfig) -> Value {
    let mc_host = format!("{}.{}.svc.cluster.local", format!("{}-mc", cfg.name), cfg.namespace);
    let rcon_host = format!("{}.{}.svc.cluster.local", format!("{}-rcon", cfg.name), cfg.namespace);
    let tools_image = format!("{}:{}", DEFAULT_TOOLS_IMAGE, cfg.image_tag);

    json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": format!("{}-dashboard", cfg.name),
            "namespace": cfg.namespace,
            "labels": dashboard_labels(cfg),
        },
        "spec": {
            "replicas": 1,
            "selector": {
                "matchLabels": {
                    APP_LABEL: format!("{}-dashboard", cfg.name),
                }
            },
            "template": {
                "metadata": {
                    "labels": dashboard_labels(cfg),
                },
                "spec": {
                    "containers": [{
                        "name": "dashboard",
                        "image": tools_image,
                        "imagePullPolicy": "IfNotPresent",
                        "args": [
                            "serve",
                            "--bind-host", "0.0.0.0",
                            "--bind-port", "8080",
                            "--minecraft-host", mc_host,
                            "--minecraft-port", cfg.port.to_string(),
                            "--rcon-host", rcon_host,
                            "--rcon-port", "25575",
                            "--rcon-password", DEFAULT_RCON_PASSWORD,
                        ],
                        "ports": [{
                            "name": "http",
                            "containerPort": 8080,
                            "protocol": "TCP",
                        }],
                        "readinessProbe": {
                            "httpGet": {
                                "path": "/api/status",
                                "port": "http",
                            },
                            "initialDelaySeconds": 5,
                            "periodSeconds": 10,
                        },
                        "livenessProbe": {
                            "httpGet": {
                                "path": "/api/status",
                                "port": "http",
                            },
                            "initialDelaySeconds": 10,
                            "periodSeconds": 30,
                        },
                        "resources": {
                            "requests": {
                                "memory": "64Mi",
                                "cpu": "50m",
                            },
                            "limits": {
                                "memory": "128Mi",
                                "cpu": "200m",
                            }
                        }
                    }]
                }
            }
        }
    })
}

pub fn render_dashboard_service(cfg: &ServerConfig) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": format!("{}-dashboard", cfg.name),
            "namespace": cfg.namespace,
            "labels": dashboard_labels(cfg),
        },
        "spec": {
            "type": "ClusterIP",
            "selector": {
                APP_LABEL: format!("{}-dashboard", cfg.name),
            },
            "ports": [{
                "name": "http",
                "port": 8080,
                "targetPort": "http",
                "protocol": "TCP",
            }]
        }
    })
}

pub fn render_all(cfg: &ServerConfig) -> Result<Vec<Value>> {
    cfg.validate()?;
    Ok(vec![
        render_namespace(cfg),
        render_config_map(cfg),
        render_rcon_secret(cfg),
        render_pvc(cfg),
        render_mods_pvc(cfg),
        render_deployment(cfg),
        render_service(cfg),
        render_rcon_service(cfg),
        render_dashboard_deployment(cfg),
        render_dashboard_service(cfg),
    ])
}

pub fn render_manifests_yaml(cfg: &ServerConfig) -> Result<String> {
    let docs = render_all(cfg)?;
    let mut out = String::new();
    for doc in docs {
        let yaml = serde_yaml::to_string(&doc)
            .map_err(|e| AppError::Manifest(e.to_string()))?;
        out.push_str("---\n");
        out.push_str(&yaml);
    }
    Ok(out)
}

pub fn validate_manifest_yaml(content: &str) -> Result<usize> {
    if content.trim().is_empty() {
        return Err(AppError::Manifest("empty manifest".into()));
    }

    let mut count = 0;
    for doc in content.split("\n---") {
        let trimmed = doc.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_yaml::from_str(trimmed)
            .map_err(|e| AppError::Manifest(e.to_string()))?;
        validate_k8s_object(&value)?;
        count += 1;
    }

    if count == 0 {
        return Err(AppError::Manifest("no documents found".into()));
    }

    Ok(count)
}

fn validate_k8s_object(obj: &Value) -> Result<()> {
    let api_version = obj
        .get("apiVersion")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Manifest("missing apiVersion".into()))?;
    let kind = obj
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Manifest("missing kind".into()))?;
    let name = obj
        .pointer("/metadata/name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Manifest(format!("{kind} missing metadata.name")))?;

    if api_version.is_empty() || kind.is_empty() || name.is_empty() {
        return Err(AppError::Manifest("invalid kubernetes metadata".into()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ModLoader, ServerConfig};

    fn sample_config() -> ServerConfig {
        ServerConfig {
            name: "test-server".into(),
            namespace: "minecraft".into(),
            minecraft_version: "1.20.1".into(),
            mod_loader: ModLoader::Forge,
            forge_version: Some("47.2.0".into()),
            memory: "4G".into(),
            port: 25565,
            replicas: 1,
            storage_size: "20Gi".into(),
            eula: true,
            max_players: 10,
            motd: "Test".into(),
            image: "example/mc".into(),
            image_tag: "dev".into(),
            modpack_url: None,
            extra_env: vec![],
        }
    }

    #[test]
    fn render_all_produces_ten_documents() {
        let docs = render_all(&sample_config()).unwrap();
        assert_eq!(docs.len(), 10);
    }

    #[test]
    fn deployment_uses_rcon_secret() {
        let dep = render_deployment(&sample_config());
        let env = dep["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array()
            .unwrap();
        assert!(env.iter().any(|e| {
            e["name"] == "RCON_PASSWORD"
                && e["valueFrom"]["secretKeyRef"]["name"] == "test-server-rcon"
        }));
    }

    #[test]
    fn dashboard_resources_reference_tools_image() {
        let dep = render_dashboard_deployment(&sample_config());
        assert!(dep["spec"]["template"]["spec"]["containers"][0]["image"]
            .as_str()
            .unwrap()
            .contains("minecraft-k8s-tools"));
        let svc = render_dashboard_service(&sample_config());
        assert_eq!(svc["spec"]["ports"][0]["port"], 8080);
    }

    #[test]
    fn rcon_service_is_cluster_ip() {
        let svc = render_rcon_service(&sample_config());
        assert_eq!(svc["spec"]["type"], "ClusterIP");
        assert_eq!(svc["spec"]["ports"][0]["port"], 25575);
    }

    #[test]
    fn deployment_has_mod_volume() {
        let dep = render_deployment(&sample_config());
        let mounts = dep["spec"]["template"]["spec"]["containers"][0]["volumeMounts"]
            .as_array()
            .unwrap();
        assert!(mounts.iter().any(|m| m["mountPath"] == "/data/mods"));
    }

    #[test]
    fn yaml_roundtrip_and_validate() {
        let yaml = render_manifests_yaml(&sample_config()).unwrap();
        assert!(yaml.contains("kind: Deployment"));
        let count = validate_manifest_yaml(&yaml).unwrap();
        assert_eq!(count, 10);
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_manifest_yaml("").is_err());
    }

    #[test]
    fn validate_manifest_only_separators() {
        let err = validate_manifest_yaml(" \n---\n ").unwrap_err();
        assert_eq!(err.to_string(), "manifest error: no documents found");
    }

    #[test]
    fn validate_rejects_empty_metadata_fields() {
        let yaml = "---\napiVersion: ''\nkind: Pod\nmetadata:\n  name: test\n";
        assert!(validate_manifest_yaml(yaml).is_err());
    }

    #[test]
    fn validate_rejects_missing_api_version() {
        let yaml = "---\nkind: Pod\nmetadata:\n  name: test\n";
        assert!(validate_manifest_yaml(yaml).is_err());
    }

    #[test]
    fn validate_rejects_missing_name() {
        let yaml = "---\napiVersion: v1\nkind: Pod\nmetadata: {}\n";
        assert!(validate_manifest_yaml(yaml).is_err());
    }

    #[test]
    fn labels_contain_app_name() {
        let lbl = labels(&sample_config());
        assert_eq!(lbl.get(APP_LABEL).map(String::as_str), Some("test-server"));
    }

    #[test]
    fn service_exposes_port() {
        let svc = render_service(&sample_config());
        assert_eq!(svc["spec"]["ports"][0]["port"], 25565);
    }
}
