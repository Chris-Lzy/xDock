use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/xdock.ico");
    println!("cargo:rerun-if-changed=assets/xdock.rc");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let res_file = out_dir.join("xdock.res");

    // 查找 Windows SDK 的 rc.exe 编译图标资源
    let mut rc_exe = None;
    let kits_root = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(&kits_root) {
        let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        versions.sort();
        versions.reverse();
        for v in versions {
            let cand = v.join("x64").join("rc.exe");
            if cand.exists() {
                rc_exe = Some(cand);
                break;
            }
        }
    }

    if let Some(rc) = rc_exe {
        let status = std::process::Command::new(rc)
            .args([
                "/nologo",
                "/fo",
                &res_file.to_string_lossy(),
                "assets/xdock.rc",
            ])
            .status();
        if let Ok(st) = status {
            if st.success() && res_file.exists() {
                println!("cargo:rustc-link-arg={}", res_file.to_string_lossy());
            }
        }
    }
}
