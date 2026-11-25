use crate::apps::{extract_icon_bgra, RawIconData};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER,
    SW_MINIMIZE, SW_RESTORE, SW_SHOWNORMAL, WM_CLOSE, WS_EX_TOOLWINDOW,
};

#[derive(Clone)]
pub struct SubWindow {
    pub hwnd: isize,
    pub title: String,
    pub visible: bool,
}

#[derive(Clone)]
pub struct RunningWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    pub exe_name: String,
    pub exe_path: String,
    pub unread_count: u32,
    pub sub_windows: Vec<SubWindow>,
    pub app_icon_idx: Option<usize>,
    pub custom_bitmap: Option<ID2D1Bitmap1>,
    pub raw_icon: Option<RawIconDataPtr>,
}

#[derive(Clone)]
pub struct RawIconDataPtr(pub std::sync::Arc<RawIconData>);

pub fn enumerate_running_windows(self_hwnd: HWND, known_app_names: &[String]) -> Vec<RunningWindow> {
    struct WinInfo {
        hwnd: isize,
        title: String,
        is_alt_tab: bool,
        is_real_window: bool,
        visible: bool,
    }
    struct EnumState {
        self_hwnd: isize,
        pid_windows: HashMap<u32, Vec<WinInfo>>,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut EnumState);
        if hwnd.0 as isize == state.self_hwnd {
            return BOOL(1);
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }

        let mut cls_buf = [0u16; 128];
        let cls_len = GetClassNameW(hwnd, &mut cls_buf);
        let cls = String::from_utf16_lossy(&cls_buf[..cls_len as usize]);
        if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "xDockClass") {
            return BOOL(1);
        }

        let visible = IsWindowVisible(hwnd).as_bool();
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let is_tool = (ex_style & WS_EX_TOOLWINDOW.0) != 0;
        let has_owner = GetWindow(hwnd, GW_OWNER).map_or(false, |o| !o.0.is_null());

        let len = GetWindowTextLengthW(hwnd);
        let title = if len > 0 {
            let mut title_buf = vec![0u16; (len + 1) as usize];
            let got = GetWindowTextW(hwnd, &mut title_buf);
            String::from_utf16_lossy(&title_buf[..got as usize])
                .trim()
                .to_string()
        } else {
            String::new()
        };

        let mut rc = RECT::default();
        let has_size = GetWindowRect(hwnd, &mut rc).is_ok()
            && (rc.right - rc.left) > 150
            && (rc.bottom - rc.top) > 100;

        // 过滤 WPS KAccountWnd 等内部隐藏通信窗口
        let is_internal_hidden = !visible
            || title.starts_with("KAccountWnd")
            || title.contains("Kso_class")
            || title.contains("GDI+ Window")
            || title.contains("OLEChannelWnd")
            || cls.contains("KAccountWnd")
            || cls.contains("MSCTFIME");

        if is_internal_hidden {
            return BOOL(1);
        }

        let is_alt_tab = visible && !is_tool && !has_owner && !title.is_empty();
        // 微信独立聊天/小程序等窗口可能带有 GW_OWNER，只要可见、非工具窗口且有正常尺寸和标题，也纳入多窗口识别
        let is_real_window = visible && !is_tool && !title.is_empty() && has_size;

        state.pid_windows.entry(pid).or_default().push(WinInfo {
            hwnd: hwnd.0 as isize,
            title,
            is_alt_tab,
            is_real_window,
            visible,
        });

        BOOL(1)
    }

    let mut enum_state = EnumState {
        self_hwnd: self_hwnd.0 as isize,
        pid_windows: HashMap::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut enum_state as *mut _ as isize));
    }

    let self_pid = unsafe { GetCurrentProcessId() };
    let mut grouped: HashMap<String, RunningWindow> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut seen_sub_hwnds = HashSet::new();

    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    let pid = pe.th32ProcessID;
                    if pid > 4 && pid != self_pid {
                        if let Some((exe_name, exe_path)) = get_process_info(pid) {
                            let lower_name = exe_name.to_lowercase();
                            let lower_path = exe_path.to_lowercase();

                            // 将 WeChatAppEx（微信内置公众号/小程序窗口）的可见独立窗口归并到 Weixin 主应用的多窗口中
                            let is_wechat_sub = lower_name.contains("wechatappex");
                            let is_service_or_sub =
                                !is_wechat_sub && is_service_or_subprocess(&lower_name, &lower_path);
                            let group_key = if is_wechat_sub {
                                "weixin".to_string()
                            } else {
                                lower_name.clone()
                            };

                            if !is_service_or_sub {
                                let wins = enum_state.pid_windows.get(&pid);
                                let has_alt_tab = wins
                                    .map_or(false, |ws| ws.iter().any(|w| w.is_alt_tab || w.is_real_window));

                                let is_user_dir = !lower_path.starts_with("c:\\windows\\")
                                    && !lower_path.contains("\\windowsapps\\microsoftwindows.");
                                let in_known_apps = known_app_names.iter().any(|k| {
                                    k == &lower_name
                                        || k.contains(&lower_name)
                                        || lower_name.contains(k)
                                });
                                let is_tray_app = !is_wechat_sub
                                    && is_user_dir
                                    && wins.is_some()
                                    && (in_known_apps || is_common_tray_app(&lower_name));

                                if has_alt_tab || is_tray_app {
                                    // 收集该进程的所有有效可见子窗口（用于多窗口弹出列表与精准唤起）
                                    let mut subs: Vec<SubWindow> = Vec::new();
                                    let mut best_hwnd = 0isize;
                                    let mut best_title = exe_name.clone();
                                    let mut best_score = 0u8;

                                    if let Some(ws) = wins {
                                        for w in ws {
                                            let score = if w.is_alt_tab {
                                                4
                                            } else if w.is_real_window {
                                                3
                                            } else {
                                                0
                                            };
                                            if score > best_score {
                                                best_score = score;
                                                best_hwnd = w.hwnd;
                                                if !w.title.is_empty() {
                                                    best_title = w.title.clone();
                                                }
                                            }

                                            if (w.is_alt_tab || w.is_real_window) && seen_sub_hwnds.insert(w.hwnd) {
                                                subs.push(SubWindow {
                                                    hwnd: w.hwnd,
                                                    title: w.title.clone(),
                                                    visible: w.visible,
                                                });
                                            }
                                        }
                                    }

                                    let unread_count = parse_unread_from_title(&best_title);

                                    if let Some(existing) = grouped.get_mut(&group_key) {
                                        // 合并多进程同款应用（如多个 Edge/Explorer/Weixin+WeChatAppEx 窗口）
                                        for sw in subs {
                                            existing.sub_windows.push(sw);
                                        }
                                        if best_score >= 3 && existing.hwnd == 0 {
                                            existing.hwnd = best_hwnd;
                                            existing.title = best_title;
                                        }
                                        existing.unread_count =
                                            existing.unread_count.max(unread_count);
                                    } else if !is_wechat_sub {
                                        let raw_icon = extract_icon_bgra(Path::new(&exe_path))
                                            .map(|d| RawIconDataPtr(std::sync::Arc::new(d)));
                                        order.push(group_key.clone());
                                        grouped.insert(
                                            group_key,
                                            RunningWindow {
                                                hwnd: best_hwnd,
                                                pid,
                                                title: best_title,
                                                exe_name,
                                                exe_path,
                                                unread_count,
                                                sub_windows: subs,
                                                app_icon_idx: None,
                                                custom_bitmap: None,
                                                raw_icon,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }

    let mut list: Vec<RunningWindow> = order
        .into_iter()
        .filter_map(|k| grouped.remove(&k))
        .collect();
    list.sort_by(|a, b| a.exe_name.to_lowercase().cmp(&b.exe_name.to_lowercase()));
    list
}

fn parse_unread_from_title(title: &str) -> u32 {
    for (open, close) in [('(', ')'), ('（', '）'), ('[', ']')] {
        if let Some(s) = title.rfind(open) {
            if let Some(e) = title[s + open.len_utf8()..].find(close) {
                let inner = title[s + open.len_utf8()..s + open.len_utf8() + e].trim();
                let digits: String = inner.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<u32>() {
                    if n > 0 && n < 9999 {
                        return n;
                    }
                }
            }
        }
    }
    if title.contains("新消息") || title.contains("未读") {
        return 1;
    }
    0
}

fn is_common_tray_app(lower: &str) -> bool {
    const TRAY_NAMES: &[&str] = &[
        "weixin", "wechat", "qq", "clash", "quark", "seafile", "ugreen", "sunlogin", "oray",
        "wps", "dingtalk", "feishu", "lark", "steam", "epic", "discord", "telegram", "snipaste",
        "everything", "powertoys", "listary", "utools", "bandizip", "potplayer", "antigravity",
    ];
    TRAY_NAMES.iter().any(|k| lower.contains(k))
}

fn is_service_or_subprocess(lower: &str, lower_path: &str) -> bool {
    if lower_path.contains("\\windows\\system32\\") || lower_path.contains("\\windows\\syswow64\\") {
        return !matches!(lower, "notepad" | "mspaint" | "taskmgr" | "cmd" | "powershell");
    }
    const SKIP_EXACT_OR_SUB: &[&str] = &[
        "conhost",
        "crashpad",
        "updater",
        "helper",
        "broker",
        "service",
        "daemon",
        "agent",
        "elevated",
        "webview2",
        "msedgewebview2",
        "runtime",
        "nvcontainer",
        "nvdisplay",
        "igfx",
        "rtkauduservice",
        "securityhealth",
        "startmenuexperiencehost",
        "shellexperiencehost",
        "searchhost",
        "textinputhost",
        "applicationframehost",
        "systemsettings",
        "lockapp",
        "ctfmon",
        "sihost",
        "taskhostw",
        "fontdrvhost",
        "audiodg",
        "widgetservice",
        "widgets",
        "phoneexperiencehost",
        "language_server",
        "wechatappex",
        "wpscloudsvr",
        "setthreadaffinitymask",
        "mihomo",
        "hans-window",
    ];
    if SKIP_EXACT_OR_SUB.iter().any(|k| lower.contains(k)) {
        return true;
    }
    lower.ends_with("svr") || lower.ends_with("svc") || lower.ends_with("appex")
}

unsafe fn get_process_info(pid: u32) -> Option<(String, String)> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 512];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        PWSTR(buf.as_mut_ptr()),
        &mut size,
    );
    let _ = CloseHandle(handle);
    if ok.is_ok() && size > 0 {
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        let stem = std::path::Path::new(&full)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())?;
        Some((stem, full))
    } else {
        None
    }
}

/// 切换/唤起已有窗口：若目标窗口当前已处于前台且未最小化，则最小化它；否则还原并置前激活
pub fn toggle_or_activate_window(hwnd_raw: isize, pid: u32, exe_path: &str) {
    if hwnd_raw != 0 {
        let hwnd = HWND(hwnd_raw as *mut _);
        unsafe {
            let fg = GetForegroundWindow();
            let mut fg_pid = 0u32;
            if !fg.0.is_null() {
                GetWindowThreadProcessId(fg, Some(&mut fg_pid));
            }
            if IsWindowVisible(hwnd).as_bool()
                && !IsIconic(hwnd).as_bool()
                && (fg == hwnd || (pid != 0 && fg_pid == pid))
            {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
                return;
            }
        }
    }
    activate_window(hwnd_raw, exe_path);
}

/// 唤起已有窗口：仅对可见主窗口置前；若当前隐藏在系统托盘（hwnd==0），通过单实例 exe 安全唤醒（绝不拉出 KAccountWnd 等隐藏通信窗口）
pub fn activate_window(hwnd_raw: isize, exe_path: &str) {
    if hwnd_raw != 0 {
        let hwnd = HWND(hwnd_raw as *mut _);
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = BringWindowToTop(hwnd);
                let _ = SetForegroundWindow(hwnd);
                return;
            }
        }
    }
    if !exe_path.is_empty() {
        let w_path: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
        let w_open: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = ShellExecuteW(
                None,
                PCWSTR(w_open.as_ptr()),
                PCWSTR(w_path.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}

pub fn close_process_or_window(hwnd_raw: isize, pid: u32) {
    if hwnd_raw != 0 {
        let hwnd = HWND(hwnd_raw as *mut _);
        unsafe {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            if IsWindowVisible(hwnd).as_bool() {
                return;
            }
        }
    }
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(handle, 0);
            let _ = CloseHandle(handle);
        }
    }
}
