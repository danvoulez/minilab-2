use std::{path::PathBuf, process::Command};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn logline_compile_lowers_ir_node_with_constitutional_runtime() {
    let output = Command::new(env!("CARGO_BIN_EXE_minilab"))
        .args(["logline", "compile"])
        .arg(fixture("outbound_send_ir.json"))
        .output()
        .expect("failed to run minilab binary");

    assert!(
        output.status.success(),
        "minilab failed with status {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("outbound_send_ir.compile.json")).unwrap())
            .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn logline_walk_routes_ir_node_through_constitutional_runtime() {
    let output = Command::new(env!("CARGO_BIN_EXE_minilab"))
        .args(["logline", "walk"])
        .arg(fixture("outbound_send_ir.json"))
        .output()
        .expect("failed to run minilab binary");

    assert!(
        output.status.success(),
        "minilab failed with status {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("outbound_send_ir.walk.json")).unwrap())
            .unwrap();

    assert_eq!(actual, expected);
}
