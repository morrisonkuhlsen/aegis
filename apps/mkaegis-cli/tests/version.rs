//! `mkaegis-cli --version` names the workspace version, which a release tag must match.

use std::process::Command;

#[test]
fn prints_the_version() {
    for flag in ["--version", "-V", "version"] {
        let out = Command::new(env!("CARGO_BIN_EXE_mkaegis-cli")).arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("mkaegis-cli {}", env!("CARGO_PKG_VERSION")), "{flag}");
    }
}
