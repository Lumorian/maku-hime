use std::process::Command;

fn pkg_config(args: &[&str]) -> Vec<String> {
    match Command::new("pkg-config").args(args).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn main() {
    // 悬浮窗锁定后要保留顶部控制条可点：GTK3 的 Wayland 后端没实现输入区域，
    // 这里编一个极小的 C 垫片直接调 wl_surface.set_input_region。
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rerun-if-changed=src/input_region.c");
        let cflags = pkg_config(&["--cflags", "gtk+-3.0", "wayland-client"]);
        let libs = pkg_config(&["--libs", "gtk+-3.0", "wayland-client"]);
        let mut build = cc::Build::new();
        build.file("src/input_region.c").warnings(false);
        for flag in &cflags {
            if let Some(path) = flag.strip_prefix("-I") {
                build.include(path);
            } else {
                build.flag(flag);
            }
        }
        for flag in &libs {
            if let Some(name) = flag.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={name}");
            } else if let Some(path) = flag.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={path}");
            }
        }
        build.compile("maku_input_region");
    }
    tauri_build::build()
}
