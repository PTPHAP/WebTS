use serde_json::json;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn local_operator_settings_are_validated_encrypted_and_do_not_change_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("master.key");
    std::fs::write(&key, hex::encode([17; 32])).unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        include_str!("../../config.example.toml")
            .replace("data/web-ts.db", "db")
            .replace("secrets/master.key", "master.key"),
    )
    .unwrap();
    let invoke = |command: &str, input: Option<&str>| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_web-ts"))
            .current_dir(dir.path())
            .args([command, "config.toml"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = input {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    };
    let secret = "operator-private-smtp-test-value";
    let value=json!({"servers":[],"default_server":"","allow_custom":true,"smtp":{"host":"smtp.example.com","port":465,"username":"mail@example.com","from":"mail@example.com","password":secret}}).to_string();
    let saved = invoke("set-settings", Some(&value));
    assert!(saved.status.success());
    assert!(!String::from_utf8_lossy(&saved.stdout).contains(secret));
    let read = invoke("get-settings", None);
    assert!(read.status.success());
    let restored: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(restored["smtp"]["password"], secret);
    let mut invalid: serde_json::Value = serde_json::from_str(&value).unwrap();
    invalid["smtp"]["port"] = json!(0);
    let bad = invalid.to_string();
    assert!(!invoke("set-settings", Some(&bad)).status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&invoke("get-settings", None).stdout).unwrap(),
        restored
    );
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            assert!(
                !std::fs::read(entry.path())
                    .unwrap()
                    .windows(secret.len())
                    .any(|p| p == secret.as_bytes())
            );
        }
    }
    assert_eq!(std::fs::read_to_string(key).unwrap(), hex::encode([17; 32]));
}
