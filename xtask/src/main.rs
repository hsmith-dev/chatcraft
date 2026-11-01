//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`, `git`) are invoked through
//! `std::process::Command`.

mod assets;
mod layers;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  assets    check that every icon/image/font/asset is attributed in ATTRIBUTION.md
  layers    enforce the crate dependency layering (docs/architecture.md)
  wasm      cargo check --target wasm32-unknown-unknown for the wasm-safe crates
  test      workspace tests (cargo nextest when installed, else cargo test), then doctests
  parity    regenerate docs/parity-checklist.md from crates/engine/src/catalog.rs
  ci        fmt --check, clippy -D warnings, test, doctest, assets, layers, wasm (stops at first failure)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("layers") => cmd_layers(),
        Some("wasm") => cmd_wasm(),
        Some("test") => cmd_test().and_then(|()| cmd_doctest()),
        Some("parity") => cmd_parity(),
        Some("ci") => cmd_ci(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (docs/architecture.md)\n");
    println!("{:<24} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c.deps.iter().filter(|d| d.workspace).map(|d| layers::short_name(&d.name).to_string()).collect();
        println!("{:<24} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Crates that must build for wasm32: L0–L4, the MCP crate and the egui front end.
/// `net` (tokio sockets) is native-only by design.
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 4 || matches!(layers::short_name(&c.name), "ui-egui" | "mcp"),
            Some(layers::Class::Standalone) => true,
            _ => false,
        })
        .map(|c| c.name)
        .collect())
}

fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown", "--keep-going"]);
    for pkg in &set {
        c.args(["-p", pkg]);
    }
    c.args(["-p", "chatcraft-web"]);
    run(c, &format!("cargo check --target wasm32-unknown-unknown -p {} -p chatcraft-web", set.join(" -p ")))?;
    println!("\nwasm32-unknown-unknown check: ok ({} crates + chatcraft-web)", set.len());
    Ok(())
}

const TEST_TARGETS: [&str; 4] = ["--workspace", "--lib", "--bins", "--tests"];

fn nextest_available() -> bool {
    cargo().args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn cmd_test() -> Result<(), String> {
    let mut c = cargo();
    if nextest_available() {
        c.args(["nextest", "run"]).args(TEST_TARGETS);
        run(c, "cargo nextest run --workspace")
    } else {
        eprintln!("note: cargo-nextest is not installed, running cargo test (cargo install cargo-nextest --locked)");
        c.arg("test").args(TEST_TARGETS);
        run(c, "cargo test --workspace")
    }
}

fn cmd_doctest() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --workspace --doc")
}

fn cmd_parity() -> Result<(), String> {
    let out = cargo().args(["run", "-q", "-p", "chatcraft-cli", "--", "parity", "--markdown"]).output().map_err(|e| format!("chatcraft-cli: {e}"))?;
    if !out.status.success() {
        return Err(format!("chatcraft-cli parity failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let path = root().join("docs/parity-checklist.md");
    std::fs::write(&path, &out.stdout).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        ("test", Box::new(cmd_test)),
        ("doctest", Box::new(cmd_doctest)),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}
