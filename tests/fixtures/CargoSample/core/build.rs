// Declares an environment input the way real build scripts do, and generates code into OUT_DIR.
use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-env-changed=SAMPLE_FLAVOR");
    let flavor = env::var("SAMPLE_FLAVOR").unwrap_or_else(|_| "plain".to_string());
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("flavor.rs");
    fs::write(out, format!("pub const FLAVOR: &str = {flavor:?};\n")).unwrap();
}
