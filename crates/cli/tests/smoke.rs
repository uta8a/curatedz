use std::process::Command;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_curatedz"));
    command.args(["--config", "fixtures/topics.yaml"]);
    command.current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    command
}

#[test]
fn cli_collect_repeat_replay_and_dry_run() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("archive");
    let first = cli()
        .args(["--fixtures", "fixtures", "--run-id", "smoke", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let again = cli()
        .args(["--run-id", "smoke", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(first.stdout, again.stdout);
    let replay = cli()
        .args(["--replay", "smoke", "--run-id", "replay", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(replay.status.success());
    let manifest: serde_json::Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(manifest["counts"]["documents"], 0);
    assert_eq!(manifest["counts"]["discoveries"], 2);
    let dry_output = directory.path().join("dry");
    assert!(
        cli()
            .args(["--fixtures", "fixtures", "--dry-run", "--output"])
            .arg(&dry_output)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!dry_output.exists());
    assert!(
        !cli()
            .args(["--validate", "--topic", "missing"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
