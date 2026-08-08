//! Embeds path-free build context for the Phase 0A evidence harness.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PREFIX: &str = "SPINAL_PHASE0A_BUILD_";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=policy");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../spinal/Cargo.toml");
    println!("cargo:rerun-if-changed=../../spinal/src");

    let manifest = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    let workspace = manifest
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    if let Some(workspace) = workspace.as_deref() {
        emit_git_watch_paths(workspace);
        emit_checkout(workspace);
        emit_cargo_lock(workspace);
    } else {
        unavailable("CHECKOUT", "manifest-directory");
        unavailable("CARGO_LOCK", "manifest-directory");
    }
    emit_rustc();
    emit_triples();
}

fn emit_checkout(workspace: &Path) {
    let Some(head) = git_output(workspace, &["rev-parse", "--verify", "HEAD"]) else {
        unavailable("CHECKOUT", "git-command");
        return;
    };
    let Some(status) = git_output(
        workspace,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    ) else {
        unavailable("CHECKOUT", "git-command");
        return;
    };
    let Ok(head) = std::str::from_utf8(&head.stdout) else {
        unavailable("CHECKOUT", "git-output");
        return;
    };
    let head = head.trim();
    if !matches!(head.len(), 40 | 64) || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        unavailable("CHECKOUT", "git-output");
        return;
    }

    available("CHECKOUT");
    emit("CHECKOUT_HEAD", &head.to_ascii_lowercase());
    emit(
        "CHECKOUT_DIRTY",
        if status.stdout.is_empty() {
            "false"
        } else {
            "true"
        },
    );
    emit("CHECKOUT_STATUS_SHA256", &sha256(&status.stdout));
}

fn emit_cargo_lock(workspace: &Path) {
    let path = workspace.join("Cargo.lock");
    let Ok(bytes) = fs::read(path) else {
        unavailable("CARGO_LOCK", "read");
        return;
    };
    available("CARGO_LOCK");
    emit("CARGO_LOCK_SHA256", &sha256(&bytes));
    emit("CARGO_LOCK_SIZE", &bytes.len().to_string());
}

fn emit_rustc() {
    let Some(rustc) = env::var_os("RUSTC") else {
        unavailable("RUSTC", "environment");
        return;
    };
    let Ok(output) = Command::new(rustc).arg("-vV").output() else {
        unavailable("RUSTC", "command");
        return;
    };
    if !output.status.success() {
        unavailable("RUSTC", "command");
        return;
    }
    let Ok(text) = std::str::from_utf8(&output.stdout) else {
        unavailable("RUSTC", "output");
        return;
    };
    let Some(release) = line_value(text, "release") else {
        unavailable("RUSTC", "output");
        return;
    };
    let Some(rustc_host) = line_value(text, "host") else {
        unavailable("RUSTC", "output");
        return;
    };
    let commit = line_value(text, "commit-hash").filter(|value| *value != "unknown");
    if !safe_token(release) || !safe_token(rustc_host) || commit.is_some_and(|v| !safe_token(v)) {
        unavailable("RUSTC", "output");
        return;
    }

    available("RUSTC");
    emit("RUSTC_VV_SHA256", &sha256(&output.stdout));
    emit("RUSTC_RELEASE", release);
    emit("RUSTC_COMMIT_HASH", commit.unwrap_or(""));
    emit("RUSTC_HOST", rustc_host);
}

fn emit_triples() {
    let host = env::var("HOST").ok().filter(|value| safe_token(value));
    let target = env::var("TARGET").ok().filter(|value| safe_token(value));
    match (host, target) {
        (Some(host), Some(target)) => {
            available("TRIPLES");
            emit("BUILD_HOST_TRIPLE", &host);
            emit("TARGET_TRIPLE", &target);
        }
        _ => unavailable("TRIPLES", "environment"),
    }
}

fn git_output(workspace: &Path, arguments: &[&str]) -> Option<Output> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(workspace)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .ok()?;
    output.status.success().then_some(output)
}

fn emit_git_watch_paths(workspace: &Path) {
    let git_dir = git_path(workspace, &["rev-parse", "--absolute-git-dir"]);
    let common_dir = git_path(
        workspace,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    if let Some(git_dir) = git_dir.as_deref() {
        watch(git_dir.join("HEAD"));
        watch(git_dir.join("index"));
    }
    if let Some(common_dir) = common_dir.as_deref() {
        watch(common_dir.join("packed-refs"));
        if let Some(symbolic_ref) = git_text(workspace, &["symbolic-ref", "--quiet", "HEAD"])
            && safe_git_ref(&symbolic_ref)
        {
            watch(common_dir.join(symbolic_ref));
        }
    }
}

fn git_path(workspace: &Path, arguments: &[&str]) -> Option<PathBuf> {
    let value = git_text(workspace, arguments)?;
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

fn git_text(workspace: &Path, arguments: &[&str]) -> Option<String> {
    let output = git_output(workspace, arguments)?;
    let value = std::str::from_utf8(&output.stdout).ok()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn safe_git_ref(value: &str) -> bool {
    value.starts_with("refs/")
        && !value.contains("..")
        && !value.contains('\\')
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn watch(path: PathBuf) {
    println!("cargo:rerun-if-changed={}", path.display());
}

fn line_value<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'=' && byte != b'\'' && byte != b'"')
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn available(component: &str) {
    emit(&format!("{component}_STATE"), "available");
    emit(&format!("{component}_ERROR"), "");
}

fn unavailable(component: &str, reason: &str) {
    emit(&format!("{component}_STATE"), "unavailable");
    emit(&format!("{component}_ERROR"), reason);
}

fn emit(name: &str, value: &str) {
    println!("cargo:rustc-env={PREFIX}{name}={value}");
}
