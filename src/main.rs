#![windows_subsystem = "windows"]

mod apps;
mod config;
mod gpu;
mod running;
mod sysinfo;
mod ui;

use apps::{scan_installed_apps, AppEntry, ScannedApp};
use config::AppConfig;
use gpu::{to_wide, GpuRenderer};
use running::{
    activate_window, close_process_or_window, enumerate_running_windows,
    toggle_or_activate_window,
};
use sysinfo::open_ms_settings;
use std::cell::RefCell;
use std::time::Instant;
use ui::{DockContextMenu, HitTarget, UiState, ViewMode};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, ScreenToClient, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use std::os::windows::process::CommandExt;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, TrackMouseEvent, UnregisterHotKey, MOD_ALT, TME_LEAVE, TRACKMOUSEEVENT,
    VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowExW, FindWindowW,
    GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowRect, IsZoomed, KillTimer,
    LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW, SetCursor, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTCLIENT, HWND_TOPMOST,
    IDC_ARROW, IDC_HAND, MSG, SET_WINDOW_POS_FLAGS, SWP_HIDEWINDOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
    WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST,
    WM_RBUTTONDOWN, WM_SETCURSOR, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_APP_APPS_LOADED: u32 = 0x8000 + 1;
const TIMER_ANIM_ID: usize = 1;
const TIMER_STATUS_ID: usize = 2;
const HOTKEY_TOGGLE_HUB: i32 = 1001;

struct AppCtx {
    hwnd: HWND,
    gpu: GpuRenderer,
    ui: UiState,
    primary_rect: RECT,
    work_rect: RECT,
    last_frame: Instant,
    anim_running: bool,
}

struct PendingWindowPos {
    hwnd: HWND,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    flags: SET_WINDOW_POS_FLAGS,
}

thread_local! {
    static CTX: RefCell<Option<AppCtx>> = const { RefCell::new(None) };
}

fn with_ctx<F: FnOnce(&mut AppCtx)>(f: F) {
    CTX.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut() {
            if let Some(ref mut ctx) = *guard {
                f(ctx);
            }
        }
    });
}

fn apply_window_pos(pos: Option<PendingWindowPos>) {
    if let Some(p) = pos {
        unsafe {
            let _ = SetWindowPos(p.hwnd, HWND_TOPMOST, p.x, p.y, p.w, p.h, p.flags);
        }
    }
}

fn is_auto_start_enabled() -> bool {
    unsafe {
        let sub = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = to_wide("xDock");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
        .is_ok()
        {
            let exists = RegQueryValueExW(
                hkey,
                PCWSTR(name.as_ptr()),
                None,
                None,
                None,
                None,
            )
            .is_ok();
            let _ = RegCloseKey(hkey);
            return exists;
        }
    }
    false
}

fn set_auto_start(enable: bool) {
    unsafe {
        let sub = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = to_wide("xDock");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
        .is_ok()
        {
            if enable {
                if let Ok(exe) = std::env::current_exe() {
                    let cmd = format!("\"{}\"", exe.to_string_lossy());
                    let wide = to_wide(&cmd);
                    let bytes = std::slice::from_raw_parts(
                        wide.as_ptr() as *const u8,
                        wide.len() * 2,
                    );
                    let _ = RegSetValueExW(
                        hkey,
                        PCWSTR(name.as_ptr()),
                        0,
                        REG_SZ,
                        Some(bytes),
                    );
                }
            } else {
                let _ = RegDeleteValueW(hkey, PCWSTR(name.as_ptr()));
            }
            let _ = RegCloseKey(hkey);
        }
    }
    // 关闭时清理可能存在的启动文件夹残留文件
    if !enable {
        if let Ok(ad) = std::env::var("APPDATA") {
            let startup = std::path::PathBuf::from(ad)
                .join("Microsoft\\Windows\\Start Menu\\Programs\\Startup");
            for fname in ["xDock.lnk", "xDock.bat", "xDock.url", "xDock.cmd"] {
                let p = startup.join(fname);
                if p.exists() {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }
}

fn set_taskbar_visible(visible: bool) {
    let cmd = if visible { SW_SHOW } else { SW_HIDE };
    let swp_flag = if visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW };
    let flags = swp_flag | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
    unsafe {
        let primary_cls = to_wide("Shell_TrayWnd");
        if let Ok(hwnd) = FindWindowW(PCWSTR(primary_cls.as_ptr()), None) {
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, cmd);
                let _ = SetWindowPos(hwnd, HWND::default(), 0, 0, 0, 0, flags);
            }
        }
        let sec_cls = to_wide("Shell_SecondaryTrayWnd");
        let mut prev = HWND::default();
        while let Ok(hwnd) = FindWindowExW(None, prev, PCWSTR(sec_cls.as_ptr()), None) {
            if hwnd.0.is_null() {
                break;
            }
            let _ = ShowWindow(hwnd, cmd);
            let _ = SetWindowPos(hwnd, HWND::default(), 0, 0, 0, 0, flags);
            prev = hwnd;
        }
    }
}

fn run_watchdog(parent_pid: u32) {
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
            let _ = WaitForSingleObject(handle, INFINITE);
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }
    set_taskbar_visible(true);
}

fn spawn_watchdog() {
    let pid = unsafe { GetCurrentProcessId() };
    if let Ok(exe) = std::env::current_exe() {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new(exe)
            .arg("--watchdog")
            .arg(pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--watchdog") {
        if let Some(pid_str) = args.next() {
            if let Ok(pid) = pid_str.parse::<u32>() {
                run_watchdog(pid);
                return;
            }
        }
        return;
    }

    std::panic::set_hook(Box::new(|_info| {
        set_taskbar_visible(true);
    }));

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let class_name = to_wide("xDockClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        // 获取主显示器屏幕区域（绕开虚拟副屏）
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(hmon, &mut mi);
        let mon_rect = mi.rcMonitor;
        let work_rect = mi.rcWork;

        let init_w = 900i32;
        let init_h = 156i32;
        let init_x = work_rect.left + ((work_rect.right - work_rect.left) - init_w) / 2;
        let init_y = if work_rect.bottom < mon_rect.bottom {
            work_rect.bottom - init_h - 12
        } else {
            mon_rect.bottom - init_h - 84
        };

        let title = to_wide("xDock");
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            init_x,
            init_y,
            init_w,
            init_h,
            None,
            None,
            hinstance,
            None,
        )
        .expect("CreateWindowExW failed");

        let dpi = GetDpiForWindow(hwnd) as f32;
        let scale = dpi / 96.0;
        let screen_dip_w = (mon_rect.right - mon_rect.left) as f32 / scale;
        let screen_dip_h = (mon_rect.bottom - mon_rect.top) as f32 / scale;

        let mut gpu = GpuRenderer::new(hwnd, init_w as u32, init_h as u32, dpi)
            .expect("GPU Renderer init failed");
        let config = AppConfig::load();
        let mut ui = UiState::new(screen_dip_w, screen_dip_h, config);
        ui.auto_start = is_auto_start_enabled();
        ui.recompute_dock_window_size();
        ui.sys_status.refresh(hwnd);

        let real_w = (ui.dock_win_w * scale).round() as i32;
        let real_h = (ui.dock_win_h * scale).round() as i32;
        let gap_px = (ui.config.dock_bottom_gap as f32 * scale).round() as i32;
        let real_x = work_rect.left + ((work_rect.right - work_rect.left) - real_w) / 2;
        let real_y = mon_rect.bottom - gap_px - real_h;
        ui.logical_w = real_w as f32 / scale;
        ui.logical_h = real_h as f32 / scale;
        let _ = gpu.resize(real_w as u32, real_h as u32, dpi);

        CTX.with(|cell| {
            *cell.borrow_mut() = Some(AppCtx {
                hwnd,
                gpu,
                ui,
                primary_rect: mon_rect,
                work_rect,
                last_frame: Instant::now(),
                anim_running: false,
            });
        });

        let _ = RegisterHotKey(hwnd, HOTKEY_TOGGLE_HUB, MOD_ALT, VK_SPACE.0 as u32);
        let _ = SetTimer(hwnd, TIMER_STATUS_ID, 5000, None);
        spawn_watchdog();
        set_taskbar_visible(false);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            real_x,
            real_y,
            real_w,
            real_h,
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );

        // 首帧渲染 & 启动后台异步扫描
        with_ctx(|ctx| ctx.ui.render(&ctx.gpu));
        spawn_app_scan(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnregisterHotKey(hwnd, HOTKEY_TOGGLE_HUB);
        set_taskbar_visible(true);
    }
}

fn spawn_app_scan(hwnd: HWND) {
    let hwnd_val = hwnd.0 as isize;
    std::thread::spawn(move || {
        let apps = scan_installed_apps();
        let boxed = Box::new(apps);
        let ptr = Box::into_raw(boxed) as isize;
        unsafe {
            let _ = PostMessageW(
                HWND(hwnd_val as *mut _),
                WM_APP_APPS_LOADED,
                WPARAM(0),
                LPARAM(ptr),
            );
        }
    });
}

unsafe fn start_anim_if_needed(ctx: &mut AppCtx) {
    if !ctx.anim_running {
        ctx.anim_running = true;
        ctx.last_frame = Instant::now();
        let _ = timeBeginPeriod(1);
        // 8ms 兼顾 60Hz (16.6ms) 与 120Hz (8.3ms) 显示器，不空转积压
        let _ = SetTimer(ctx.hwnd, TIMER_ANIM_ID, 8, None);
    }
}

unsafe fn stop_anim(ctx: &mut AppCtx) {
    if ctx.anim_running {
        ctx.anim_running = false;
        let _ = KillTimer(ctx.hwnd, TIMER_ANIM_ID);
        let _ = timeEndPeriod(1);
    }
}

/// 准备模式切换并返回需要在 RefCell 作用域外执行的 SetWindowPos 参数（避免同步 WM_SIZE 重入）
unsafe fn prepare_switch_mode(ctx: &mut AppCtx, to_hub: bool) -> PendingWindowPos {
    let scale = ctx.gpu.dpi / 96.0;
    let mon = ctx.primary_rect;
    let work = ctx.work_rect;
    if to_hub {
        ctx.ui.mode = ViewMode::Hub;
        ctx.ui.hub_closing = false;
        ctx.ui.hub_target = 1.0;
        ctx.ui.search_query.clear();
        ctx.ui.apply_filter();
        let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
        let list = enumerate_running_windows(ctx.hwnd, &known);
        ctx.ui.set_running(list, &ctx.gpu);

        let pw = mon.right - mon.left;
        let ph = mon.bottom - mon.top;
        ctx.ui.logical_w = pw as f32 / scale;
        ctx.ui.logical_h = ph as f32 / scale;
        let _ = ctx.gpu.resize(pw as u32, ph as u32, ctx.gpu.dpi);
        start_anim_if_needed(ctx);
        ctx.ui.render(&ctx.gpu);

        PendingWindowPos {
            hwnd: ctx.hwnd,
            x: mon.left,
            y: mon.top,
            w: pw,
            h: ph,
            flags: SWP_SHOWWINDOW,
        }
    } else {
        ctx.ui.mode = ViewMode::Dock;
        ctx.ui.hub_closing = false;
        ctx.ui.hub_target = 0.0;
        ctx.ui.hub_anim = 0.0;
        ctx.ui.recompute_dock_window_size();

        let pw = (ctx.ui.dock_win_w * scale).round() as i32;
        let ph = (ctx.ui.dock_win_h * scale).round() as i32;
        let px = work.left + ((work.right - work.left) - pw) / 2;
        let fg = GetForegroundWindow();
        let is_max_or_full = !fg.0.is_null() && fg != ctx.hwnd && {
            if IsZoomed(fg).as_bool() {
                true
            } else {
                let mut fr = RECT::default();
                GetWindowRect(fg, &mut fr).is_ok()
                    && (fr.right - fr.left) >= (mon.right - mon.left)
                    && (fr.bottom - fr.top) >= (mon.bottom - mon.top)
            }
        };
        let gap_px = if is_max_or_full {
            2
        } else {
            (ctx.ui.config.dock_bottom_gap as f32 * scale).round() as i32
        };
        let py = mon.bottom - gap_px - ph;
        ctx.ui.logical_w = pw as f32 / scale;
        ctx.ui.logical_h = ph as f32 / scale;
        let _ = ctx.gpu.resize(pw as u32, ph as u32, ctx.gpu.dpi);
        start_anim_if_needed(ctx);
        ctx.ui.render(&ctx.gpu);

        PendingWindowPos {
            hwnd: ctx.hwnd,
            x: px,
            y: py,
            w: pw,
            h: ph,
            flags: SWP_SHOWWINDOW | SWP_NOACTIVATE,
        }
    }
}

unsafe fn begin_close_hub(ctx: &mut AppCtx) {
    if ctx.ui.mode == ViewMode::Hub && !ctx.ui.hub_closing {
        ctx.ui.hub_closing = true;
        ctx.ui.hub_target = 0.0;
        start_anim_if_needed(ctx);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => LRESULT(0),
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let mut is_hand = false;
                with_ctx(|ctx| {
                    is_hand = !matches!(ctx.ui.hovered, HitTarget::None | HitTarget::HubBackdrop);
                });
                let cursor_id = if is_hand { IDC_HAND } else { IDC_ARROW };
                if let Ok(hcur) = LoadCursorW(None, cursor_id) {
                    let _ = SetCursor(hcur);
                }
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_APP_APPS_LOADED => {
            let ptr = lparam.0 as *mut Vec<ScannedApp>;
            if !ptr.is_null() {
                let scanned = *Box::from_raw(ptr);
                let mut pending_pos = None;
                with_ctx(|ctx| {
                    let entries: Vec<AppEntry> = scanned
                        .into_iter()
                        .map(|s| {
                            let bitmap = s.icon_raw.and_then(|raw| {
                                ctx.gpu.create_bitmap_from_bgra(
                                    raw.width,
                                    raw.height,
                                    &raw.bgra_premul,
                                )
                            });
                            AppEntry {
                                name: s.name,
                                name_lower: s.name_lower,
                                path: s.path,
                                category: s.category,
                                bitmap,
                                launch_count: 0,
                                badge_count: 0,
                            }
                        })
                        .collect();
                    ctx.ui.set_apps(entries);
                    if ctx.ui.mode == ViewMode::Dock {
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    } else {
                        ctx.ui.render(&ctx.gpu);
                    }
                });
                apply_window_pos(pending_pos);
            } else if wparam.0 == TIMER_STATUS_ID {
                with_ctx(|ctx| {
                    ctx.ui.sys_status.refresh(hwnd);
                    let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
                    let list = enumerate_running_windows(ctx.hwnd, &known);
                    ctx.ui.set_running(list, &ctx.gpu);
                    if ctx.ui.mode == ViewMode::Dock {
                        ctx.ui.render(&ctx.gpu);
                    }
                });
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_TOGGLE_HUB {
                let mut pending_pos = None;
                with_ctx(|ctx| {
                    if ctx.ui.mode == ViewMode::Dock {
                        pending_pos = Some(prepare_switch_mode(ctx, true));
                    } else {
                        begin_close_hub(ctx);
                    }
                });
                apply_window_pos(pending_pos);
            } else if wparam.0 == TIMER_STATUS_ID {
                with_ctx(|ctx| {
                    ctx.ui.sys_status.refresh(hwnd);
                    let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
                    let list = enumerate_running_windows(ctx.hwnd, &known);
                    ctx.ui.set_running(list, &ctx.gpu);
                    if ctx.ui.mode == ViewMode::Dock {
                        ctx.ui.render(&ctx.gpu);
                    }
                });
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x_px = (lparam.0 & 0xFFFF) as i16 as f32;
            let y_px = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let mut pending_pos = None;
            with_ctx(|ctx| {
                if !ctx.ui.mouse_inside {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                }
                let scale = ctx.gpu.dpi / 96.0;
                ctx.ui.mouse_inside = true;
                ctx.ui.mouse_x = x_px / scale;
                ctx.ui.mouse_y = y_px / scale;
                ctx.ui.hovered = ctx.ui.hit_test(ctx.ui.mouse_x, ctx.ui.mouse_y);

                if ctx.ui.mode == ViewMode::Dock && ctx.ui.context_menu.is_none() {
                    let prev_sub = ctx.ui.dock_hover_sub;
                    let mut next_sub = None;
                    match ctx.ui.hovered {
                        HitTarget::DockApp(app_idx) => {
                            if let Some(r_idx) = ctx.ui.running.iter().position(|rw| rw.app_icon_idx == Some(app_idx)) {
                                if ctx.ui.running[r_idx].sub_windows.len() >= 2 {
                                    next_sub = Some((ctx.ui.mouse_x, r_idx));
                                }
                            }
                        }
                        HitTarget::DockRunning(r_idx) => {
                            if ctx.ui.running.get(r_idx).map_or(false, |rw| rw.sub_windows.len() >= 2) {
                                next_sub = Some((ctx.ui.mouse_x, r_idx));
                            }
                        }
                        HitTarget::DockSubWin(_) | HitTarget::DockSubWinClose(_) => {
                            next_sub = prev_sub;
                        }
                        _ => {
                            let sr = ctx.ui.dock_sub_menu_rect;
                            if ctx.ui.mouse_x >= sr.left && ctx.ui.mouse_x <= sr.right && ctx.ui.mouse_y >= sr.top && ctx.ui.mouse_y <= sr.bottom {
                                next_sub = prev_sub;
                            }
                        }
                    }
                    if next_sub.map(|(_, i)| i) != prev_sub.map(|(_, i)| i) {
                        ctx.ui.dock_hover_sub = next_sub;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                }
                start_anim_if_needed(ctx);
            });
            apply_window_pos(pending_pos);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let mut pending_pos = None;
            with_ctx(|ctx| {
                ctx.ui.mouse_inside = false;
                ctx.ui.hovered = HitTarget::None;
                if ctx.ui.dock_hover_sub.is_some() {
                    ctx.ui.dock_hover_sub = None;
                    pending_pos = Some(prepare_switch_mode(ctx, false));
                }
                start_anim_if_needed(ctx);
            });
            apply_window_pos(pending_pos);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x_px = (lparam.0 & 0xFFFF) as i16 as f32;
            let y_px = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let mut pending_pos = None;
            let mut should_rescan = false;
            let mut should_quit = false;

            with_ctx(|ctx| {
                let scale = ctx.gpu.dpi / 96.0;
                let target = ctx.ui.hit_test(x_px / scale, y_px / scale);
                match target {
                    HitTarget::DockHubBtn => {
                        ctx.ui.hub_settings = false;
                        pending_pos = Some(prepare_switch_mode(ctx, true));
                    }
                    HitTarget::DockSettings => {
                        ctx.ui.hub_settings = true;
                        pending_pos = Some(prepare_switch_mode(ctx, true));
                    }
                    HitTarget::DockApp(idx) => {
                        if let Some(rw) = ctx.ui.find_running_for_app(idx) {
                            let (h, p, ep) = (rw.hwnd, rw.pid, rw.exe_path.clone());
                            toggle_or_activate_window(h, p, &ep);
                            ctx.ui.trigger_dock_bounce(idx);
                        } else {
                            ctx.ui.launch_app(idx);
                        }
                        start_anim_if_needed(ctx);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::DockRunning(r_idx) => {
                        if let Some(rw) = ctx.ui.running.get(r_idx) {
                            toggle_or_activate_window(rw.hwnd, rw.pid, &rw.exe_path);
                        }
                        start_anim_if_needed(ctx);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::DockRefresh => should_rescan = true,
                    HitTarget::DockExit => should_quit = true,
                    HitTarget::HubBackdrop => {
                        ctx.ui.hub_settings = false;
                        begin_close_hub(ctx);
                    }
                    HitTarget::HubSettingsBtn => {
                        ctx.ui.hub_settings = !ctx.ui.hub_settings;
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::HubSearchBox => {
                        ctx.ui.search_focused = true;
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::HubCategory(cat) => {
                        ctx.ui.category = cat;
                        ctx.ui.page = 0;
                        ctx.ui.apply_filter();
                        ctx.ui.trigger_page_anim(1.0);
                        start_anim_if_needed(ctx);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::HubRunningApp(r_idx) => {
                        if let Some(rw) = ctx.ui.running.get(r_idx) {
                            activate_window(rw.hwnd, &rw.exe_path);
                            pending_pos = Some(prepare_switch_mode(ctx, false));
                        }
                    }
                    HitTarget::HubRunningClose(r_idx) => {
                        ctx.ui.confirm_close = Some(r_idx);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::ConfirmCloseCancel => {
                        ctx.ui.confirm_close = None;
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::ConfirmCloseYes => {
                        if let Some(r_idx) = ctx.ui.confirm_close.take() {
                            if let Some(rw) = ctx.ui.running.get(r_idx) {
                                close_process_or_window(rw.hwnd, rw.pid);
                                ctx.ui.running.remove(r_idx);
                            }
                        }
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::ToggleQuickEdit => {
                        ctx.ui.quick_edit_mode = !ctx.ui.quick_edit_mode;
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::QuickRemove(idx) => {
                        ctx.ui.toggle_quick_pin(idx);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::HubQuickApp(idx) | HitTarget::HubGridApp(idx) => {
                        if ctx.ui.quick_edit_mode {
                            ctx.ui.toggle_quick_pin(idx);
                            ctx.ui.render(&ctx.gpu);
                        } else {
                            ctx.ui.launch_app(idx);
                            if ctx.ui.config.close_hub_on_launch {
                                pending_pos = Some(prepare_switch_mode(ctx, false));
                            } else {
                                ctx.ui.render(&ctx.gpu);
                            }
                        }
                    }
                    HitTarget::HubPageDot(p) => {
                        let dir = if p >= ctx.ui.page { 1.0 } else { -1.0 };
                        ctx.ui.page = p;
                        ctx.ui.trigger_page_anim(dir);
                        start_anim_if_needed(ctx);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SetZoom(idx) => {
                        ctx.ui.config.dock_zoom = match idx {
                            0 => 1.25,
                            2 => 1.60,
                            _ => 1.42,
                        };
                        ctx.ui.config.save();
                        ctx.ui.recompute_dock_window_size();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SetDockHeight(h) => {
                        ctx.ui.height_input_focused = false;
                        ctx.ui.height_input_buf = h.to_string();
                        ctx.ui.config.dock_height = h as f32;
                        ctx.ui.config.save();
                        ctx.ui.recompute_dock_window_size();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::DockHeightInput => {
                        ctx.ui.height_input_focused = true;
                        ctx.ui.height_input_buf = format!("{}", ctx.ui.config.dock_height as u16);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::DockSubWin(sub_hwnd) => {
                        activate_window(sub_hwnd, "");
                        ctx.ui.dock_hover_sub = None;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::DockSubWinClose(sub_hwnd) => {
                        close_process_or_window(sub_hwnd, 0);
                        let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
                        let list = enumerate_running_windows(ctx.hwnd, &known);
                        ctx.ui.set_running(list, &ctx.gpu);
                        ctx.ui.dock_hover_sub = None;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::RestoreHiddenDock => {
                        ctx.ui.config.hidden_dock_apps.clear();
                        ctx.ui.config.save();
                        ctx.ui.rebuild_dock_indices();
                        let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
                        let list = enumerate_running_windows(ctx.hwnd, &known);
                        ctx.ui.set_running(list, &ctx.gpu);
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::CtxDismiss => {
                        ctx.ui.context_menu = None;
                        ctx.ui.power_menu_open = false;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::CtxOpen => {
                        if let Some(menu) = ctx.ui.context_menu.take() {
                            if let Some(r_idx) = menu.running_idx {
                                if let Some(rw) = ctx.ui.running.get(r_idx) {
                                    activate_window(rw.hwnd, &rw.exe_path);
                                }
                            } else if let Some(app_idx) = menu.app_idx {
                                ctx.ui.launch_app(app_idx);
                            }
                        }
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::CtxAdmin => {
                        if let Some(menu) = ctx.ui.context_menu.take() {
                            let w_path = to_wide(&menu.path.to_string_lossy());
                            let w_verb = to_wide("runas");
                            let _ = windows::Win32::UI::Shell::ShellExecuteW(
                                None,
                                PCWSTR(w_verb.as_ptr()),
                                PCWSTR(w_path.as_ptr()),
                                None,
                                None,
                                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                            );
                        }
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::CtxLocation => {
                        if let Some(menu) = ctx.ui.context_menu.take() {
                            let arg = format!("/select,\"{}\"", menu.path.to_string_lossy());
                            let w_exp = to_wide("explorer.exe");
                            let w_arg = to_wide(&arg);
                            let w_open = to_wide("open");
                            let _ = windows::Win32::UI::Shell::ShellExecuteW(
                                None,
                                PCWSTR(w_open.as_ptr()),
                                PCWSTR(w_exp.as_ptr()),
                                PCWSTR(w_arg.as_ptr()),
                                None,
                                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                            );
                        }
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::CtxHide => {
                        if let Some(menu) = ctx.ui.context_menu.take() {
                            if !ctx.ui.config.hidden_dock_apps.iter().any(|h| h == &menu.name) {
                                ctx.ui.config.hidden_dock_apps.push(menu.name);
                                ctx.ui.config.save();
                            }
                            ctx.ui.rebuild_dock_indices();
                            let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
                            let list = enumerate_running_windows(ctx.hwnd, &known);
                            ctx.ui.set_running(list, &ctx.gpu);
                        }
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::CtxCloseApp => {
                        if let Some(menu) = ctx.ui.context_menu.take() {
                            if let Some(r_idx) = menu.running_idx {
                                if let Some(rw) = ctx.ui.running.get(r_idx) {
                                    close_process_or_window(rw.hwnd, rw.pid);
                                    ctx.ui.running.remove(r_idx);
                                }
                            }
                        }
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::SetBottomGap(idx) => {
                        ctx.ui.config.dock_bottom_gap = match idx {
                            1 => 16,
                            2 => 36,
                            _ => 2,
                        };
                        ctx.ui.config.save();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SetCols(cols) => {
                        ctx.ui.config.hub_cols = cols;
                        ctx.ui.config.save();
                        ctx.ui.apply_filter();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SetAnimPreset(p) => {
                        ctx.ui.config.anim_preset = p;
                        ctx.ui.config.save();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SetGlassOpacity(idx) => {
                        ctx.ui.config.glass_opacity = match idx {
                            0 => 0.78,
                            2 => 0.95,
                            _ => 0.88,
                        };
                        ctx.ui.config.save();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::ToggleCloseOnLaunch => {
                        ctx.ui.config.close_hub_on_launch = !ctx.ui.config.close_hub_on_launch;
                        ctx.ui.config.save();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::ToggleAutoStart => {
                        let next = !ctx.ui.auto_start;
                        set_auto_start(next);
                        ctx.ui.auto_start = is_auto_start_enabled();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::ResetConfig => {
                        ctx.ui.config = AppConfig::default();
                        ctx.ui.config.save();
                        ctx.ui.apply_filter();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SysNet => open_ms_settings("ms-settings:network"),
                    HitTarget::SysVol => {
                        ctx.ui.sys_status.toggle_mute();
                        ctx.ui.render(&ctx.gpu);
                    }
                    HitTarget::SysIme => open_ms_settings("ms-settings:regionlanguage"),
                    HitTarget::SysClock => open_ms_settings("ms-settings:dateandtime"),
                    HitTarget::DockPower => {
                        ctx.ui.context_menu = None;
                        ctx.ui.power_menu_open = !ctx.ui.power_menu_open;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                    HitTarget::PowerLock => {
                        ctx.ui.power_menu_open = false;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                        let _ = std::process::Command::new("rundll32.exe")
                            .args(["user32.dll,LockWorkStation"])
                            .spawn();
                    }
                    HitTarget::PowerSleep => {
                        ctx.ui.power_menu_open = false;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                        let _ = std::process::Command::new("rundll32.exe")
                            .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                            .spawn();
                    }
                    HitTarget::PowerRestart => {
                        ctx.ui.power_menu_open = false;
                        set_taskbar_visible(true);
                        let _ = std::process::Command::new("shutdown.exe")
                            .args(["/r", "/t", "0"])
                            .spawn();
                    }
                    HitTarget::PowerShutdown => {
                        ctx.ui.power_menu_open = false;
                        set_taskbar_visible(true);
                        let _ = std::process::Command::new("shutdown.exe")
                            .args(["/s", "/t", "0"])
                            .spawn();
                    }
                    HitTarget::None => {}
                }
            });

            apply_window_pos(pending_pos);
            if should_rescan {
                spawn_app_scan(hwnd);
            }
            if should_quit {
                set_taskbar_visible(true);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            let x_px = (lparam.0 & 0xFFFF) as i16 as f32;
            let y_px = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let mut pending_pos = None;
            with_ctx(|ctx| {
                if ctx.ui.mode == ViewMode::Dock {
                    let scale = ctx.gpu.dpi / 96.0;
                    let lx = x_px / scale;
                    let ly = y_px / scale;
                    let target = ctx.ui.hit_test(lx, ly);
                    let mut new_menu = None;
                    match target {
                        HitTarget::DockApp(app_idx) => {
                            if let Some(app) = ctx.ui.apps.get(app_idx) {
                                let running_idx = ctx
                                    .ui
                                    .running
                                    .iter()
                                    .position(|rw| rw.app_icon_idx == Some(app_idx));
                                new_menu = Some(DockContextMenu {
                                    x: lx - 40.0,
                                    y: ly,
                                    app_idx: Some(app_idx),
                                    running_idx,
                                    name: app.name.clone(),
                                    path: app.path.clone(),
                                });
                            }
                        }
                        HitTarget::DockRunning(r_idx) => {
                            if let Some(rw) = ctx.ui.running.get(r_idx) {
                                let name = rw
                                    .app_icon_idx
                                    .and_then(|i| ctx.ui.apps.get(i))
                                    .map(|a| a.name.clone())
                                    .unwrap_or_else(|| rw.exe_name.clone());
                                new_menu = Some(DockContextMenu {
                                    x: lx - 40.0,
                                    y: ly,
                                    app_idx: rw.app_icon_idx,
                                    running_idx: Some(r_idx),
                                    name,
                                    path: std::path::PathBuf::from(&rw.exe_path),
                                });
                            }
                        }
                        _ => {
                            if ctx.ui.context_menu.is_some() {
                                ctx.ui.context_menu = None;
                                pending_pos = Some(prepare_switch_mode(ctx, false));
                            }
                        }
                    }
                    if new_menu.is_some() {
                        ctx.ui.context_menu = new_menu;
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    }
                }
            });
            apply_window_pos(pending_pos);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            with_ctx(|ctx| {
                if ctx.ui.mode == ViewMode::Dock {
                    let vr = ctx.ui.sys_vol_rect;
                    if ctx.ui.mouse_x >= vr.left
                        && ctx.ui.mouse_x <= vr.right
                        && ctx.ui.mouse_y >= vr.top
                        && ctx.ui.mouse_y <= vr.bottom
                    {
                        ctx.ui.sys_status.adjust_volume(if delta > 0 { 1 } else { -1 });
                        ctx.ui.render(&ctx.gpu);
                    }
                } else if ctx.ui.mode == ViewMode::Hub && ctx.ui.confirm_close.is_none() {
                    let rp = ctx.ui.running_panel_rect;
                    let qp = ctx.ui.quick_panel_rect;
                    let in_running = ctx.ui.mouse_x >= rp.left
                        && ctx.ui.mouse_x <= rp.right
                        && ctx.ui.mouse_y >= rp.top
                        && ctx.ui.mouse_y <= rp.bottom;
                    let in_quick = ctx.ui.mouse_x >= qp.left
                        && ctx.ui.mouse_x <= qp.right
                        && ctx.ui.mouse_y >= qp.top
                        && ctx.ui.mouse_y <= qp.bottom;
                    if in_running {
                        if delta < 0 {
                            ctx.ui.running_scroll = ctx.ui.running_scroll.saturating_add(1);
                        } else if delta > 0 {
                            ctx.ui.running_scroll = ctx.ui.running_scroll.saturating_sub(1);
                        }
                        ctx.ui.render(&ctx.gpu);
                    } else if in_quick {
                        if delta < 0 {
                            ctx.ui.quick_scroll = ctx.ui.quick_scroll.saturating_add(1);
                        } else if delta > 0 {
                            ctx.ui.quick_scroll = ctx.ui.quick_scroll.saturating_sub(1);
                        }
                        ctx.ui.render(&ctx.gpu);
                    } else if ctx.ui.total_pages > 1 {
                        if delta < 0 && ctx.ui.page + 1 < ctx.ui.total_pages {
                            ctx.ui.page += 1;
                            ctx.ui.trigger_page_anim(1.0);
                            start_anim_if_needed(ctx);
                            ctx.ui.render(&ctx.gpu);
                        } else if delta > 0 && ctx.ui.page > 0 {
                            ctx.ui.page -= 1;
                            ctx.ui.trigger_page_anim(-1.0);
                            start_anim_if_needed(ctx);
                            ctx.ui.render(&ctx.gpu);
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let mut pending_pos = None;
            with_ctx(|ctx| {
                if ctx.ui.mode == ViewMode::Hub {
                    match vk {
                        x if x == VK_ESCAPE.0 => {
                            if ctx.ui.confirm_close.is_some() {
                                ctx.ui.confirm_close = None;
                                ctx.ui.render(&ctx.gpu);
                            } else {
                                ctx.ui.hub_settings = false;
                                begin_close_hub(ctx);
                            }
                        }
                        x if x == VK_LEFT.0 => {
                            if ctx.ui.page > 0 {
                                ctx.ui.page -= 1;
                                ctx.ui.trigger_page_anim(-1.0);
                                start_anim_if_needed(ctx);
                                ctx.ui.render(&ctx.gpu);
                            }
                        }
                        x if x == VK_RIGHT.0 => {
                            if ctx.ui.page + 1 < ctx.ui.total_pages {
                                ctx.ui.page += 1;
                                ctx.ui.trigger_page_anim(1.0);
                                start_anim_if_needed(ctx);
                                ctx.ui.render(&ctx.gpu);
                            }
                        }
                        x if x == VK_RETURN.0 => {
                            if ctx.ui.hub_settings && ctx.ui.height_input_focused {
                                if let Ok(v) = ctx.ui.height_input_buf.parse::<f32>() {
                                    ctx.ui.config.dock_height = v.clamp(30.0, 120.0);
                                    ctx.ui.config.save();
                                    ctx.ui.recompute_dock_window_size();
                                }
                                ctx.ui.height_input_focused = false;
                                ctx.ui.render(&ctx.gpu);
                            } else {
                                let per_page = ctx.ui.config.hub_cols * 5;
                                let slot = ctx.ui.selected_grid_idx.unwrap_or(0);
                                let global = ctx.ui.page * per_page + slot;
                                if let Some(&app_idx) = ctx.ui.filtered_indices.get(global) {
                                    ctx.ui.launch_app(app_idx);
                                    pending_pos = Some(prepare_switch_mode(ctx, false));
                                }
                            }
                        }
                        x if x == VK_BACK.0 => {
                            if ctx.ui.hub_settings && ctx.ui.height_input_focused {
                                ctx.ui.height_input_buf.pop();
                                ctx.ui.render(&ctx.gpu);
                            } else {
                                ctx.ui.search_query.pop();
                                ctx.ui.apply_filter();
                                ctx.ui.render(&ctx.gpu);
                            }
                        }
                        x if x == VK_UP.0 || x == VK_DOWN.0 => {}
                        _ => {}
                    }
                }
            });
            apply_window_pos(pending_pos);
            LRESULT(0)
        }
        WM_CHAR => {
            let ch = char::from_u32(wparam.0 as u32);
            with_ctx(|ctx| {
                if ctx.ui.mode == ViewMode::Hub {
                    if let Some(c) = ch {
                        if ctx.ui.hub_settings && ctx.ui.height_input_focused {
                            if c.is_ascii_digit() && ctx.ui.height_input_buf.len() < 3 {
                                ctx.ui.height_input_buf.push(c);
                                if let Ok(v) = ctx.ui.height_input_buf.parse::<f32>() {
                                    if (30.0..=120.0).contains(&v) {
                                        ctx.ui.config.dock_height = v;
                                        ctx.ui.config.save();
                                        ctx.ui.recompute_dock_window_size();
                                    }
                                }
                                ctx.ui.render(&ctx.gpu);
                            }
                        } else if !ctx.ui.hub_settings && !c.is_control() {
                            ctx.ui.search_query.push(c);
                            ctx.ui.apply_filter();
                            ctx.ui.render(&ctx.gpu);
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIM_ID {
                let mut pending_pos = None;
                with_ctx(|ctx| {
                    // 兜底检测真实鼠标是否仍在窗口内，防止透明窗体漏收 WM_MOUSELEAVE 导致图标无法缩回
                    if ctx.ui.mouse_inside && ctx.ui.mode == ViewMode::Dock {
                        let mut pt = POINT::default();
                        if GetCursorPos(&mut pt).is_ok() && ScreenToClient(hwnd, &mut pt).as_bool() {
                            let scale = ctx.gpu.dpi / 96.0;
                            let lx = pt.x as f32 / scale;
                            let ly = pt.y as f32 / scale;
                            if lx < 0.0 || ly < 0.0 || lx > ctx.ui.logical_w || ly > ctx.ui.logical_h {
                                ctx.ui.mouse_inside = false;
                                ctx.ui.hovered = HitTarget::None;
                            } else {
                                ctx.ui.mouse_x = lx;
                                ctx.ui.mouse_y = ly;
                            }
                        }
                    }

                    let now = Instant::now();
                    let dt = (now - ctx.last_frame).as_secs_f32().clamp(0.001, 0.05);
                    ctx.last_frame = now;
                    let keep = ctx.ui.tick_animations(dt);
                    if ctx.ui.hub_closing && ctx.ui.hub_anim <= 0.02 {
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                        stop_anim(ctx);
                    } else {
                        if keep {
                            ctx.ui.render(&ctx.gpu);
                        }
                        // 只有当所有动画收敛且鼠标已不在 Dock 内时才停止定时器，确保移出时 100% 触发缩回
                        if !keep && !(ctx.ui.mode == ViewMode::Dock && ctx.ui.mouse_inside) {
                            stop_anim(ctx);
                        }
                    }
                });
                apply_window_pos(pending_pos);
            } else if wparam.0 == TIMER_STATUS_ID {
                let mut pending_pos = None;
                with_ctx(|ctx| {
                    ctx.ui.sys_status.refresh(hwnd);
                    let known: Vec<String> = ctx.ui.apps.iter().map(|a| a.name_lower.clone()).collect();
                    let list = enumerate_running_windows(ctx.hwnd, &known);
                    let prev_w = ctx.ui.dock_win_w;
                    ctx.ui.set_running(list, &ctx.gpu);
                    if ctx.ui.mode == ViewMode::Dock && (ctx.ui.dock_win_w - prev_w).abs() > 1.0 {
                        pending_pos = Some(prepare_switch_mode(ctx, false));
                    } else if ctx.ui.mode == ViewMode::Dock {
                        ctx.ui.render(&ctx.gpu);
                    }
                });
                apply_window_pos(pending_pos);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let w = (lparam.0 & 0xFFFF) as u32;
            let h = ((lparam.0 >> 16) & 0xFFFF) as u32;
            if w > 0 && h > 0 {
                with_ctx(|ctx| {
                    let scale = ctx.gpu.dpi / 96.0;
                    ctx.ui.logical_w = w as f32 / scale;
                    ctx.ui.logical_h = h as f32 / scale;
                    let _ = ctx.gpu.resize(w, h, ctx.gpu.dpi);
                });
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as f32;
            with_ctx(|ctx| {
                ctx.gpu.dpi = new_dpi;
                ctx.ui.render(&ctx.gpu);
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            set_taskbar_visible(true);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            set_taskbar_visible(true);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
