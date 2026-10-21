//! `mkaegis --version` prints the version and exits without opening a window.

use std::process::Command;

#[test]
fn prints_the_version_without_a_window() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_mkaegis")).arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("mkaegis {}", env!("CARGO_PKG_VERSION")), "{flag}");
    }
}
