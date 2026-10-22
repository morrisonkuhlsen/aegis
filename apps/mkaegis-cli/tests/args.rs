//! The command line refuses what it can't take with exit code 2 and reads projects whatever the
//! case of `.aegis` (#63).

use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mkaegis-cli")).args(args).output().unwrap()
}

#[test]
fn usage_errors_exit_with_code_2() {
    for args in [&[][..], &["bogus"], &["info"], &["run", "--bogus"], &["run", "--in"]] {
        let out = cli(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"), "{args:?}");
    }
    // A run that fails is not a usage error.
    let out = cli(&["run", "--cmd", "no.such.command={}"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn projects_in_upper_case_are_projects() {
    let dir = std::env::temp_dir().join(format!("mkaegis-cli-test-{}-case", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let project = dir.join("MAP.AEGIS");
    let out = cli(&["run", "--sample", "--out", project.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read_to_string(&project).unwrap().contains("\"format\": \"aegis\""));
    let out = cli(&["run", "--in", project.to_str().unwrap(), "--cmd", r#"layer.info={"layer":"Cidades"}"#]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
