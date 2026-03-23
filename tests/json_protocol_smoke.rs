use serde_json::Value;
use std::process::Command;

fn run_socat(args: &[&str]) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_socat");
    let out = Command::new(bin).args(args).output().expect("run socat");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8(out.stdout).expect("stdout utf8");
    (code, stdout)
}

fn assert_envelope_shape(v: &Value) {
    let required = [
        "schema_version",
        "ok",
        "command",
        "input",
        "plan",
        "result",
        "error",
        "next_actions",
        "version",
        "timestamp",
    ];
    for key in required {
        assert!(v.get(key).is_some(), "missing key: {key}");
    }
}

#[test]
fn inventory_uses_unified_json_envelope() {
    let (code, out) = run_socat(&["--json", "inventory"]);
    assert_eq!(code, 0, "unexpected exit code, out={out}");

    let v: Value = serde_json::from_str(&out).expect("valid json");
    assert_envelope_shape(&v);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "inventory");
    assert_eq!(v["schema_version"], "1.0.0");
}

#[test]
fn invalid_address_has_stable_error_code() {
    let (code, out) = run_socat(&["--json", "check", "bad-address"]);
    assert_eq!(code, 0, "unexpected exit code, out={out}");

    let v: Value = serde_json::from_str(&out).expect("valid json");
    assert_envelope_shape(&v);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "E_ADDR_PARSE");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid")
    );
}

#[test]
fn run_input_json_executes_plan_mode() {
    let mut path = std::env::temp_dir();
    path.push(format!("socat-rs-run-input-{}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{
  "mode": "plan",
  "from": "tcp://127.0.0.1:8080",
  "to": "stdio://",
  "json": true
}"#,
    )
    .expect("write json input");

    let path_s = path.to_str().expect("path str").to_string();
    let (code, out) = run_socat(&["run", "--input-json", &path_s]);
    let _ = std::fs::remove_file(path);

    assert_eq!(code, 0, "unexpected exit code, out={out}");
    let v: Value = serde_json::from_str(&out).expect("valid json");
    assert_envelope_shape(&v);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "plan");
    assert!(
        v["plan"]["executable_command"]
            .as_str()
            .unwrap_or_default()
            .contains("socat link --from")
    );
}
