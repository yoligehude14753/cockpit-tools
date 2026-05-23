#[cfg(target_os = "macos")]
use swift_rs::SwiftLinker;

use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "macos")]
fn link_macos_swift_runtime_rpaths() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn go_target_from_rust_target(target: &str) -> Option<(&'static str, &'static str)> {
    let goos = if target.contains("windows") {
        "windows"
    } else if target.contains("apple-darwin") {
        "darwin"
    } else if target.contains("linux") {
        "linux"
    } else {
        return None;
    };

    let goarch = if target.starts_with("x86_64") {
        "amd64"
    } else if target.starts_with("aarch64") {
        "arm64"
    } else if target.starts_with("i686") {
        "386"
    } else if target.starts_with("armv7") {
        "arm"
    } else {
        return None;
    };

    Some((goos, goarch))
}

fn build_cockpit_cliproxy_sidecar() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is required"));
    let target = std::env::var("TARGET").expect("TARGET is required");
    println!("cargo:rustc-env=COCKPIT_RUST_TARGET={target}");
    let Some((goos, goarch)) = go_target_from_rust_target(&target) else {
        panic!("unsupported sidecar build target: {target}");
    };

    let sidecar_dir = manifest_dir.join("../sidecars/cockpit-cliproxy");
    let output_dir = sidecar_dir.join("bin");
    let extension = if goos == "windows" { ".exe" } else { "" };
    let output = output_dir.join(format!("cockpit-cliproxy-{target}{extension}"));

    println!("cargo:rerun-if-env-changed=COCKPIT_SKIP_CLIPROXY_BUILD");
    println!(
        "cargo:rerun-if-changed={}",
        sidecar_dir.join("go.mod").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        sidecar_dir.join("go.sum").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        sidecar_dir.join("main.go").display()
    );
    if std::env::var("COCKPIT_SKIP_CLIPROXY_BUILD").ok().as_deref() == Some("1") && output.exists()
    {
        return;
    }

    std::fs::create_dir_all(&output_dir).expect("failed to create cockpit-cliproxy bin dir");
    let status = Command::new("go")
        .current_dir(&sidecar_dir)
        .env("GOOS", goos)
        .env("GOARCH", goarch)
        .env("CGO_ENABLED", "0")
        .arg("build")
        .arg("-trimpath")
        .arg("-ldflags")
        .arg("-s -w")
        .arg("-o")
        .arg(&output)
        .arg(".")
        .status()
        .expect("failed to start go build for cockpit-cliproxy");

    if !status.success() {
        panic!("go build for cockpit-cliproxy failed with status: {status}");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_cockpit_cliproxy_sidecar();

    #[cfg(target_os = "macos")]
    {
        SwiftLinker::new("12.0")
            .with_package("MacosNativeMenuSwift", "native/macos-native-menu")
            .link();
        link_macos_swift_runtime_rpaths();
    }

    tauri_build::build()
}
