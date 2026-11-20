//! The repository's own guards (scripts/check-*.py) must reject what they exist to reject.
//! Each test builds a small throwaway workspace, runs the script on it, and reads the verdict.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> io::Result<Self> {
        let dir = std::env::temp_dir().join(format!("agent-build-checks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); // leftovers from an aborted run
        fs::create_dir_all(&dir)?;
        Ok(Scratch(dir))
    }

    fn write(&self, rel: &str, text: &str) -> io::Result<()> {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0); // best effort
    }
}

fn run(script: &str, root: &Path) -> io::Result<(bool, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts").join(script);
    let out = Command::new("python3").arg(path).arg(root).output()?;
    Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
}

const ROOT: &str = "[package]\nname = \"agent-build\"\n\n[dependencies]\nagent-build-core = { path = \"crates/agent-build-core\" }\n";
const CORE: &str = "[package]\nname = \"agent-build-core\"\n";
const SYS: &str = "[package]\nname = \"agent-build-sys\"\n\n[dependencies]\nagent-build-core = { path = \"../agent-build-core\" }\n";

// A9.1
#[test]
fn deps_check_rejects_third_party_crates_and_an_impure_core() -> TestResult {
    let ws = Scratch::new("deps")?;
    ws.write("Cargo.toml", ROOT)?;
    ws.write("crates/agent-build-core/Cargo.toml", CORE)?;
    ws.write("crates/agent-build-sys/Cargo.toml", SYS)?;
    assert_eq!(run("check-deps.py", &ws.0)?, (true, "dependencies ok\n".into()));

    ws.write("crates/agent-build-sys/Cargo.toml", &format!("{SYS}serde = \"1\"\n"))?;
    let (ok, out) = run("check-deps.py", &ws.0)?;
    assert!(!ok && out.contains("serde is not a workspace path dependency"), "{out}");

    ws.write("crates/agent-build-sys/Cargo.toml", SYS)?;
    let impure = format!("{CORE}\n[dev-dependencies]\nagent-build-sys = {{ path = \"../agent-build-sys\" }}\n");
    ws.write("crates/agent-build-core/Cargo.toml", &impure)?;
    let (ok, out) = run("check-deps.py", &ws.0)?;
    assert!(!ok && out.contains("core must not depend on anything"), "{out}");
    Ok(())
}

const TABLE: &str = "| 编号 | 情形 | 防 | 验证 | 状态 |\n| --- | --- | --- | --- | --- |\n\
| A1 | a | b | 冒烟 | 待实现 |\n| A1.1 | a | b | 单元 | 已实现 |\n| A2 | a | b | 集成 | 已实现 |\n";

// A9.2
#[test]
fn cases_check_rejects_a_table_and_tests_that_disagree() -> TestResult {
    let ws = Scratch::new("cases")?;
    ws.write("docs/cases.md", TABLE)?;
    ws.write("crates/x/src/lib.rs", "// A1.1\n#[test]\nfn guarded() {}\n")?;
    ws.write("tests/t.rs", "// A2\n#[test]\nfn guarded() {}\n")?;
    let (ok, out) = run("check-cases.py", &ws.0)?;
    assert!(ok && out.contains("3 cases; 2 implemented, 1 pending"), "{out}");

    ws.write("crates/x/src/lib.rs", "#[test]\nfn untagged_unit_is_fine() {}\n")?;
    ws.write("tests/t.rs", "// A2\n#[test]\nfn guarded() {}\n#[test]\nfn untagged() {}\n// A9\n#[test]\n#[ignore]\nfn unknown() {}\n")?;
    let (ok, out) = run("check-cases.py", &ws.0)?;
    assert!(!ok, "{out}");
    assert!(out.contains("A1.1: marked 已实现 with 单元, but no tagged test"), "{out}");
    assert!(out.contains("tests/t.rs:4: test has no case tag"), "{out}");
    assert!(out.contains("tag A9 is not in docs/cases.md"), "{out}");

    ws.write("docs/cases.md", &format!("{TABLE}| A1.1 | a | b | 单元 | 已实现 |\n| A3 | a | b | 集成 |\n"))?;
    let (ok, out) = run("check-cases.py", &ws.0)?;
    assert!(!ok && out.contains("id A1.1 repeated") && out.contains("expected 5 cells"), "{out}");
    Ok(())
}
