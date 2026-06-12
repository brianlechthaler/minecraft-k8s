use std::process::Command;

#[test]
fn binary_entry_runs_validate() {
    let bin = env!("CARGO_BIN_EXE_minecraft-k8s");
    let root = env!("CARGO_MANIFEST_DIR");
    let config = format!("{root}/../../config/server.toml");
    let output = Command::new(bin)
        .args(["validate", "--config", &config])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
}

#[test]
fn binary_probe_fails_when_port_closed() {
    let bin = env!("CARGO_BIN_EXE_minecraft-k8s");
    let output = Command::new(bin)
        .args(["probe", "--host", "127.0.0.1", "--port", "1", "--timeout-secs", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}
