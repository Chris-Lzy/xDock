use std::collections::HashSet;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Controls::IImageList;
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX,
    SHIL_EXTRALARGE, SHIL_JUMBO,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppCategory {
    All,
    Pinned,
    Dev,
    System,
    Daily,
}

impl AppCategory {
    pub fn label(self) -> &'static str {
        match self {
            AppCategory::All => "全部应用",
            AppCategory::Pinned => "常用置顶",
            AppCategory::Dev => "开发创作",
            AppCategory::System => "系统工具",
            AppCategory::Daily => "日常效率",
        }
    }

    pub const TABS: [AppCategory; 5] = [
        AppCategory::All,
        AppCategory::Pinned,
        AppCategory::Dev,
        AppCategory::System,
        AppCategory::Daily,
    ];
}

pub struct RawIconData {
    pub width: u32,
    pub height: u32,
    pub bgra_premul: Vec<u8>,
}

pub struct ScannedApp {
    pub name: String,
    pub name_lower: String,
    pub path: PathBuf,
    pub category: AppCategory,
    pub icon_raw: Option<RawIconData>,
}

pub struct AppEntry {
    pub name: String,
    pub name_lower: String,
    pub path: PathBuf,
    pub category: AppCategory,
    pub bitmap: Option<ID2D1Bitmap1>,
    pub launch_count: u32,
    pub badge_count: u32,
}

pub fn scan_installed_apps() -> Vec<ScannedApp> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let mut search_roots = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        search_roots.push(PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        search_roots.push(PathBuf::from(ad).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(up) = std::env::var("USERPROFILE") {
        search_roots.push(PathBuf::from(up).join("Desktop"));
    }

    let mut files = Vec::new();
    for root in search_roots {
        collect_shortcuts(&root, 0, &mut files);
    }

    let mut seen_names = HashSet::new();
    let mut apps = Vec::new();

    // 优先处理根目录层级（更浅的路径通常是主应用）
    files.sort_by_key(|p| p.components().count());

    for path in files {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = stem.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        let full_lower = path.to_string_lossy().to_lowercase();
        if is_noise_entry(&lower, &full_lower) {
            continue;
        }
        let norm = normalize_app_name(&lower);
        if norm.is_empty() || !seen_names.insert(norm) {
            continue;
        }

        let category = classify_app(&lower, &path);
        let icon_raw = unsafe { extract_icon_bgra(&path) };

        apps.push(ScannedApp {
            name,
            name_lower: lower,
            path,
            category,
            icon_raw,
        });
    }

    apps.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));

    unsafe {
        CoUninitialize();
    }
    apps
}

fn collect_shortcuts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 2 || !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_shortcuts(&p, depth + 1, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let e = ext.to_ascii_lowercase();
            // 只收集 .lnk 快捷方式，过滤散落的 .exe 和 .url 网页链接
            if e == "lnk" {
                out.push(p);
            }
        }
    }
}

fn normalize_app_name(lower: &str) -> String {
    let mut s = lower.to_string();
    for suffix in [
        " - 简体中文",
        "(simplified chinese)",
        "(64-bit)",
        "(32-bit)",
        "(x64)",
        "(x86)",
        "(wow)",
        "简体中文",
    ] {
        s = s.replace(suffix, "");
    }
    s.trim().to_string()
}

fn is_noise_entry(lower: &str, full_lower: &str) -> bool {
    const NOISE: &[&str] = &[
        "uninstall",
        "卸载",
        "readme",
        "manual",
        "license",
        "documentation",
        "help",
        "帮助",
        "website",
        "homepage",
        "官网",
        "release notes",
        "updater",
        "update",
        "repair",
        "修复",
        "installer",
        "安装程序",
        "command prompt",
        "developer command",
        "developer powershell",
        "powershell ise",
        " (x86)",
        "(wow)",
        "verifier",
        "assistant",
        "diagnostic",
        "performance analyzer",
        "performance recorder",
        "app cert kit",
        "software development kit",
        "administrative tools",
        "app manager",
        "autodesk access",
        "visualstyleseditor",
        "vmcreate",
        "voiceaccess",
        "windbg",
        "backup",
        "配置",
        "向导",
        "wizard",
        "tool",
        "sdk",
        "console",
        "debug",
        "cli",
        "daemon",
        "service",
        "helper",
    ];
    if NOISE.iter().any(|k| lower.contains(k)) {
        return true;
    }
    // 排除 Windows Kits / Accessibility / Administrative 子目录里的附属工具
    const NOISE_DIRS: &[&str] = &[
        "\\windows kits\\",
        "\\accessibility\\",
        "\\system tools\\",
        "\\windows powershell\\",
        "\\maintenance\\",
    ];
    NOISE_DIRS.iter().any(|d| full_lower.contains(d))
}

fn classify_app(lower: &str, path: &Path) -> AppCategory {
    let full = path.to_string_lossy().to_lowercase();
    const DEV_KEYS: &[&str] = &[
        "code", "visual studio", "git", "python", "node", "docker", "rust", "cargo",
        "terminal", "powershell", "cmd", "jetbrains", "idea", "pycharm", "clion",
        "android", "blender", "figma", "photoshop", "illustrator", "premiere",
        "after effects", "unreal", "unity", "godot", "cmake", "mingw", "wsl",
        "cursor", "windsurf", "antigravity", "trae", "sublime", "notepad++",
    ];
    if DEV_KEYS.iter().any(|k| lower.contains(k) || full.contains(k)) {
        return AppCategory::Dev;
    }

    const SYS_KEYS: &[&str] = &[
        "settings", "设置", "control panel", "控制面板", "task manager", "任务管理器",
        "registry", "注册表", "device manager", "设备管理器", "disk", "磁盘",
        "resource monitor", "事件查看器", "服务", "services", "narrator", "magnifier",
        "calculator", "计算器", "snipping", "截图", "defender", "安全中心",
        "administrative", "system", "windows tools", "powershell",
    ];
    if SYS_KEYS.iter().any(|k| lower.contains(k) || full.contains(k)) {
        return AppCategory::System;
    }

    AppCategory::Daily
}

/// 从路径提取高清图标，并转换为 Direct2D 所需的预乘 Alpha BGRA32 像素
pub unsafe fn extract_icon_bgra(path: &Path) -> Option<RawIconData> {
    let path_str = path.to_string_lossy();
    if path_str.starts_with("\\\\") {
        return None;
    }
    let wide: Vec<u16> = path_str
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut sfi = SHFILEINFOW::default();
    let res = SHGetFileInfoW(
        PCWSTR(wide.as_ptr()),
        Default::default(),
        Some(&mut sfi),
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_SYSICONINDEX,
    );

    let mut hicon: HICON = HICON::default();
    let mut raw_data: Option<RawIconData> = None;
    if res != 0 {
        for list_id in [SHIL_JUMBO, SHIL_EXTRALARGE] {
            if let Ok(img_list) = SHGetImageList::<IImageList>(list_id as i32) {
                if let Ok(ic) = img_list.GetIcon(sfi.iIcon, 0) {
                    if !ic.is_invalid() {
                        let candidate = hicon_to_premul_bgra(ic);
                        let _ = DestroyIcon(ic);
                        if let Some(d) = candidate {
                            // 若 256x256 JUMBO 只有左上角 <=48x48 有像素，说明原图标较小，回退到 EXTRALARGE
                            if list_id == SHIL_JUMBO && is_tiny_in_jumbo(&d) {
                                continue;
                            }
                            raw_data = Some(d);
                            break;
                        }
                    }
                }
            }
        }
    }

    if raw_data.is_some() {
        return raw_data;
    }

    let mut sfi_fb = SHFILEINFOW::default();
    let ok = SHGetFileInfoW(
        PCWSTR(wide.as_ptr()),
        Default::default(),
        Some(&mut sfi_fb),
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );
    if ok != 0 {
        hicon = sfi_fb.hIcon;
    }

    if hicon.is_invalid() {
        return None;
    }

    let data = hicon_to_premul_bgra(hicon);
    let _ = DestroyIcon(hicon);
    data
}

fn is_tiny_in_jumbo(d: &RawIconData) -> bool {
    if d.width < 128 || d.height < 128 {
        return false;
    }
    // 检查右下 3/4 区域是否有任何非透明像素；若全透明则说明图标缩在左上角
    for y in (d.height / 3)..d.height {
        let row_off = (y * d.width * 4) as usize;
        for x in (d.width / 3)..d.width {
            let a = d.bgra_premul[row_off + (x as usize) * 4 + 3];
            if a > 8 {
                return false;
            }
        }
    }
    true
}

unsafe fn hicon_to_premul_bgra(hicon: HICON) -> Option<RawIconData> {
    let mut info = ICONINFO::default();
    if GetIconInfo(hicon, &mut info).is_err() {
        return None;
    }

    let hbm = if !info.hbmColor.is_invalid() {
        info.hbmColor
    } else {
        info.hbmMask
    };

    let mut bm = BITMAP::default();
    let got = windows::Win32::Graphics::Gdi::GetObjectW(
        hbm,
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut c_void),
    );
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask);
        }
        return None;
    }

    let w = bm.bmWidth as u32;
    let h = bm.bmHeight as u32;
    let mut pixels = vec![0u8; (w * h * 4) as usize];

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        biHeight: -(h as i32), // Top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let hdc = CreateCompatibleDC(None);
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        h,
        Some(pixels.as_mut_ptr() as *mut c_void),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(hdc);

    if !info.hbmColor.is_invalid() {
        let _ = DeleteObject(info.hbmColor);
    }
    if !info.hbmMask.is_invalid() {
        let _ = DeleteObject(info.hbmMask);
    }

    if lines == 0 {
        return None;
    }

    // 检查是否全 0 alpha（部分老式 24 位图标没有 alpha 通道）
    let has_alpha = pixels.chunks_exact(4).any(|px| px[3] != 0);
    if !has_alpha {
        for px in pixels.chunks_exact_mut(4) {
            if px[0] != 0 || px[1] != 0 || px[2] != 0 {
                px[3] = 255;
            }
        }
    } else {
        // 预乘 Alpha（Direct2D 要求）
        for px in pixels.chunks_exact_mut(4) {
            let a = px[3] as u32;
            px[0] = ((px[0] as u32 * a + 127) / 255) as u8;
            px[1] = ((px[1] as u32 * a + 127) / 255) as u8;
            px[2] = ((px[2] as u32 * a + 127) / 255) as u8;
        }
    }

    Some(RawIconData {
        width: w,
        height: h,
        bgra_premul: pixels,
    })
}
