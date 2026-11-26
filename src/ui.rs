use crate::apps::{AppCategory, AppEntry};
use crate::config::AppConfig;
use crate::gpu::{point_f, rect_f, rgba, to_wide, GpuRenderer};
use crate::running::RunningWindow;
use crate::sysinfo::SystemStatus;
use std::collections::HashMap;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_GRADIENT_STOP, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1RenderTarget, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Dock,
    Hub,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    None,
    DockHubBtn,
    DockApp(usize),
    DockRunning(usize),
    DockSettings,
    DockPower,
    DockRefresh,
    DockExit,
    PowerLock,
    PowerSleep,
    PowerRestart,
    PowerShutdown,
    HubBackdrop,
    HubSearchBox,
    HubSettingsBtn,
    HubCategory(AppCategory),
    HubRunningApp(usize),
    HubRunningClose(usize),
    HubQuickApp(usize),
    HubGridApp(usize),
    HubPageDot(usize),
    SetZoom(u8),
    SetDockHeight(u16),
    DockHeightInput,
    SetBottomGap(u8),
    SetCols(usize),
    SetAnimPreset(u8),
    SetGlassOpacity(u8),
    ToggleCloseOnLaunch,
    ToggleAutoStart,
    RestoreHiddenDock,
    ResetConfig,
    ConfirmCloseYes,
    ConfirmCloseCancel,
    ToggleQuickEdit,
    QuickRemove(usize),
    SysNet,
    SysVol,
    SysIme,
    SysClock,
    CtxOpen,
    CtxAdmin,
    CtxLocation,
    CtxHide,
    CtxCloseApp,
    CtxDismiss,
    DockSubWin(isize),
    DockSubWinClose(isize),
}

#[derive(Clone)]
pub struct DockContextMenu {
    pub x: f32,
    pub y: f32,
    pub app_idx: Option<usize>,
    pub running_idx: Option<usize>,
    pub name: String,
    pub path: std::path::PathBuf,
}

pub struct UiState {
    pub mode: ViewMode,
    pub hub_settings: bool,
    pub quick_edit_mode: bool,
    pub auto_start: bool,
    pub confirm_close: Option<usize>,
    pub context_menu: Option<DockContextMenu>,
    pub power_menu_open: bool,
    pub dock_hover_sub: Option<(f32, usize)>,
    pub dock_sub_menu_rect: D2D_RECT_F,
    pub height_input_focused: bool,
    pub height_input_buf: String,
    pub running_scroll: usize,
    pub quick_scroll: usize,
    pub running_panel_rect: D2D_RECT_F,
    pub quick_panel_rect: D2D_RECT_F,
    pub sys_vol_rect: D2D_RECT_F,
    pub sys_status: SystemStatus,
    pub config: AppConfig,
    pub apps: Vec<AppEntry>,
    pub running: Vec<RunningWindow>,
    pub running_icon_cache: HashMap<String, ID2D1Bitmap1>,
    pub dock_indices: Vec<usize>,
    pub dock_running_extra: Vec<usize>,
    pub filtered_indices: Vec<usize>,
    pub quick_indices: Vec<usize>,
    pub category: AppCategory,
    pub search_query: String,
    pub search_focused: bool,
    pub page: usize,
    pub total_pages: usize,

    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_inside: bool,
    pub hovered: HitTarget,

    // 动画状态 (二阶弹簧 + 平滑缓动)
    pub hub_anim: f32,
    pub hub_target: f32,
    pub hub_closing: bool,
    pub dock_scales: Vec<f32>,
    pub dock_vels: Vec<f32>,
    pub dock_bounce: Vec<f32>,
    pub card_hovers: Vec<f32>,
    pub page_anim: f32,
    pub page_dir: f32,
    pub selected_grid_idx: Option<usize>,

    // 缓存字体格式
    fmt_title: Option<IDWriteTextFormat>,
    fmt_body: Option<IDWriteTextFormat>,
    fmt_small: Option<IDWriteTextFormat>,
    fmt_center_small: Option<IDWriteTextFormat>,
    fmt_badge: Option<IDWriteTextFormat>,

    // 缓存逻辑尺寸（DIP）
    pub logical_w: f32,
    pub logical_h: f32,
    pub dock_win_w: f32,
    pub dock_win_h: f32,
    pub hub_origin_x: f32,
    pub hub_origin_y: f32,
    pub screen_dip_w: f32,
    pub screen_dip_h: f32,

    // 布局命中框缓存
    hit_rects: Vec<(D2D_RECT_F, HitTarget)>,
}

impl UiState {
    pub fn new(screen_dip_w: f32, screen_dip_h: f32, config: AppConfig) -> Self {
        Self {
            mode: ViewMode::Dock,
            hub_settings: false,
            quick_edit_mode: false,
            auto_start: false,
            confirm_close: None,
            context_menu: None,
            power_menu_open: false,
            dock_hover_sub: None,
            dock_sub_menu_rect: rect_f(0.0, 0.0, 0.0, 0.0),
            height_input_focused: false,
            height_input_buf: format!("{}", config.dock_height as u16),
            running_scroll: 0,
            quick_scroll: 0,
            running_panel_rect: rect_f(0.0, 0.0, 0.0, 0.0),
            quick_panel_rect: rect_f(0.0, 0.0, 0.0, 0.0),
            sys_vol_rect: rect_f(0.0, 0.0, 0.0, 0.0),
            sys_status: SystemStatus::default(),
            config,
            apps: Vec::new(),
            running: Vec::new(),
            running_icon_cache: HashMap::new(),
            dock_indices: Vec::new(),
            dock_running_extra: Vec::new(),
            filtered_indices: Vec::new(),
            quick_indices: Vec::new(),
            category: AppCategory::All,
            search_query: String::new(),
            search_focused: true,
            page: 0,
            total_pages: 1,
            mouse_x: -1000.0,
            mouse_y: -1000.0,
            mouse_inside: false,
            hovered: HitTarget::None,
            hub_anim: 0.0,
            hub_target: 0.0,
            hub_closing: false,
            dock_scales: Vec::new(),
            dock_vels: Vec::new(),
            dock_bounce: Vec::new(),
            card_hovers: Vec::new(),
            page_anim: 1.0,
            page_dir: 0.0,
            selected_grid_idx: None,
            fmt_title: None,
            fmt_body: None,
            fmt_small: None,
            fmt_center_small: None,
            fmt_badge: None,
            logical_w: 880.0,
            logical_h: 120.0,
            dock_win_w: 880.0,
            dock_win_h: 120.0,
            hub_origin_x: screen_dip_w * 0.5 - 180.0,
            hub_origin_y: screen_dip_h - 60.0,
            screen_dip_w,
            screen_dip_h,
            hit_rects: Vec::new(),
        }
    }

    pub unsafe fn ensure_fonts(&mut self, gpu: &GpuRenderer) {
        if self.fmt_title.is_some() {
            return;
        }
        let font_name = to_wide("Microsoft YaHei UI");
        let locale = to_wide("zh-CN");
        let f = &gpu.dwrite_factory;

        if let Ok(fmt) = f.CreateTextFormat(
            PCWSTR(font_name.as_ptr()),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            18.0,
            PCWSTR(locale.as_ptr()),
        ) {
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            self.fmt_title = Some(fmt);
        }

        if let Ok(fmt) = f.CreateTextFormat(
            PCWSTR(font_name.as_ptr()),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            13.5,
            PCWSTR(locale.as_ptr()),
        ) {
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            self.fmt_body = Some(fmt);
        }

        if let Ok(fmt) = f.CreateTextFormat(
            PCWSTR(font_name.as_ptr()),
            None,
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.5,
            PCWSTR(locale.as_ptr()),
        ) {
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            self.fmt_small = Some(fmt);
        }

        if let Ok(fmt) = f.CreateTextFormat(
            PCWSTR(font_name.as_ptr()),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.5,
            PCWSTR(locale.as_ptr()),
        ) {
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            self.fmt_center_small = Some(fmt);
        }

        if let Ok(fmt) = f.CreateTextFormat(
            PCWSTR(font_name.as_ptr()),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.0,
            PCWSTR(locale.as_ptr()),
        ) {
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            self.fmt_badge = Some(fmt);
        }
    }

    pub fn set_apps(&mut self, apps: Vec<AppEntry>) {
        self.apps = apps;
        self.card_hovers = vec![0.0; self.apps.len()];
        self.rebuild_dock_indices();
        self.refresh_quick_list();
        self.apply_filter();
    }

    pub fn rebuild_dock_indices(&mut self) {
        self.dock_indices.clear();

        // 优先挑选常用或开发/日常应用进入底部 Dock（最多 8 个），跳过用户隐藏的图标
        let preferred = [
            "chrome", "edge", "firefox", "code", "terminal", "explorer", "wechat", "微信",
            "notepad", "antigravity", "powershell",
        ];
        for kw in preferred {
            if self.dock_indices.len() >= 8 {
                break;
            }
            if let Some(idx) = self.apps.iter().position(|a| {
                a.name_lower.contains(kw)
                    && !self
                        .config
                        .hidden_dock_apps
                        .iter()
                        .any(|h| h == &a.name)
            }) {
                if !self.dock_indices.contains(&idx) {
                    self.dock_indices.push(idx);
                }
            }
        }
        for (i, a) in self.apps.iter().enumerate() {
            if self.dock_indices.len() >= 8 {
                break;
            }
            if !self.dock_indices.contains(&i)
                && !self
                    .config
                    .hidden_dock_apps
                    .iter()
                    .any(|h| h == &a.name)
            {
                self.dock_indices.push(i);
            }
        }

        let total_dock = self.dock_indices.len() + self.dock_running_extra.len();
        self.dock_scales = vec![1.0; total_dock];
        self.dock_vels = vec![0.0; total_dock];
        self.dock_bounce = vec![0.0; total_dock];
        self.recompute_dock_window_size();
    }

    pub fn find_running_for_app(&self, app_idx: usize) -> Option<&RunningWindow> {
        self.running.iter().find(|rw| rw.app_icon_idx == Some(app_idx))
    }

    pub unsafe fn set_running(&mut self, mut list: Vec<RunningWindow>, gpu: &GpuRenderer) {
        for app in &mut self.apps {
            app.badge_count = 0;
        }
        let mut extra = Vec::new();
        for (r_idx, rw) in list.iter_mut().enumerate() {
            let exe_low = rw.exe_name.to_lowercase();
            let title_low = rw.title.to_lowercase();
            rw.app_icon_idx = self.apps.iter().position(|a| {
                a.name_lower == exe_low
                    || a.name_lower.contains(&exe_low)
                    || (!exe_low.is_empty() && title_low.contains(&a.name_lower))
                    || a.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map_or(false, |s| s.eq_ignore_ascii_case(&rw.exe_name))
            });
            if let Some(idx) = rw.app_icon_idx {
                let app_name = self.apps[idx].name.clone();
                self.apps[idx].badge_count = self.apps[idx].badge_count.max(rw.unread_count);
                let is_hidden = self.config.hidden_dock_apps.iter().any(|h| h == &app_name);
                if !is_hidden && !self.dock_indices.contains(&idx) && extra.len() < 6 {
                    extra.push(r_idx);
                }
            } else if let Some(cached) = self.running_icon_cache.get(&rw.exe_path) {
                rw.custom_bitmap = Some(cached.clone());
            } else if let Some(ref raw) = rw.raw_icon {
                if let Some(bmp) =
                    gpu.create_bitmap_from_bgra(raw.0.width, raw.0.height, &raw.0.bgra_premul)
                {
                    self.running_icon_cache.insert(rw.exe_path.clone(), bmp.clone());
                    rw.custom_bitmap = Some(bmp);
                }
            }
        }
        self.running = list;
        self.dock_running_extra = extra;
        let total_dock = self.dock_indices.len() + self.dock_running_extra.len();
        self.dock_scales.resize(total_dock, 1.0);
        self.dock_vels.resize(total_dock, 0.0);
        self.dock_bounce.resize(total_dock, 0.0);
        self.recompute_dock_window_size();
    }

    pub fn dock_metrics(&self) -> (f32, f32, f32, f32) {
        let pill_h = self.config.dock_height.clamp(30.0, 120.0);
        let base_icon = (pill_h * 0.68).clamp(20.0, 76.0);
        let slot_base = base_icon + (pill_h * 0.22).clamp(8.0, 16.0);
        let hub_btn = (pill_h * 0.72).clamp(22.0, 72.0);
        (pill_h, base_icon, slot_base, hub_btn)
    }

    pub fn recompute_dock_window_size(&mut self) {
        let (pill_h, _base_icon, slot_base, hub_btn) = self.dock_metrics();
        let count = (self.dock_indices.len() + self.dock_running_extra.len()).max(1);
        let max_extra = (self.config.dock_zoom - 1.0) * (slot_base * 2.4);
        let extra_sep = if self.dock_running_extra.is_empty() { 0.0 } else { 14.0 };
        let main_pill_w = 24.0 + hub_btn + 16.0 + (count as f32) * slot_base + extra_sep + 20.0 + max_extra;
        let side_pill_w = 446.0;
        self.dock_win_w = (main_pill_w + 14.0 + side_pill_w + 64.0).min(self.screen_dip_w - 16.0);
        self.dock_win_h = if self.context_menu.is_some() || self.power_menu_open {
            pill_h + 224.0
        } else if self.dock_hover_sub.is_some() {
            pill_h + 230.0
        } else {
            pill_h + 68.0
        };
    }

    pub fn refresh_quick_list(&mut self) {
        if !self.config.pinned_apps.is_empty() {
            let mut indices = Vec::new();
            for name in &self.config.pinned_apps {
                if let Some(idx) = self.apps.iter().position(|a| &a.name == name) {
                    if !indices.contains(&idx) {
                        indices.push(idx);
                    }
                }
            }
            self.quick_indices = indices;
            return;
        }
        let mut indices: Vec<usize> = (0..self.apps.len()).collect();
        indices.sort_by(|&a, &b| {
            self.apps[b]
                .launch_count
                .cmp(&self.apps[a].launch_count)
                .then_with(|| a.cmp(&b))
        });
        indices.truncate(6);
        self.quick_indices = indices;
    }

    pub fn toggle_quick_pin(&mut self, app_idx: usize) {
        let Some(name) = self.apps.get(app_idx).map(|a| a.name.clone()) else {
            return;
        };
        if self.config.pinned_apps.is_empty() {
            // 首次编辑时将当前展示的快速直达列表作为初始固定项
            self.config.pinned_apps = self
                .quick_indices
                .iter()
                .filter_map(|&i| self.apps.get(i).map(|a| a.name.clone()))
                .collect();
        }
        if let Some(pos) = self.config.pinned_apps.iter().position(|n| n == &name) {
            self.config.pinned_apps.remove(pos);
        } else {
            self.config.pinned_apps.push(name);
        }
        self.config.save();
        self.refresh_quick_list();
    }

    pub fn apply_filter(&mut self) {
        let q = self.search_query.trim().to_lowercase();
        self.filtered_indices = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, app)| {
                let cat_ok = match self.category {
                    AppCategory::All => true,
                    AppCategory::Pinned => app.launch_count > 0 || self.dock_indices.iter().any(|&d| self.apps.get(d).map_or(false, |da| da.name == app.name)),
                    c => app.category == c,
                };
                if !cat_ok {
                    return false;
                }
                if q.is_empty() {
                    true
                } else {
                    app.name_lower.contains(&q)
                }
            })
            .map(|(i, _)| i)
            .collect();

        let per_page = self.config.hub_cols * 5;
        self.total_pages = ((self.filtered_indices.len() + per_page - 1) / per_page).max(1);
        if self.page >= self.total_pages {
            self.page = 0;
        }
        self.selected_grid_idx = if self.filtered_indices.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    pub fn trigger_page_anim(&mut self, dir: f32) {
        self.page_anim = 0.0;
        self.page_dir = dir;
    }

    pub fn trigger_dock_bounce(&mut self, app_idx: usize) {
        if let Some(pos) = self.dock_indices.iter().position(|&i| i == app_idx) {
            if let Some(b) = self.dock_bounce.get_mut(pos) {
                *b = 1.0;
            }
        }
    }

    /// 推进二阶弹簧与平滑缓动动画，返回是否仍有动画需要下一帧继续渲染
    pub fn tick_animations(&mut self, dt: f32) -> bool {
        let mut active = false;
        let exp_fast = 1.0 - (-dt * 18.0).exp();
        let exp_page = 1.0 - (-dt * 15.0).exp();

        // Hub 展开/收起动画（支持双向平滑过渡）
        let diff = self.hub_target - self.hub_anim;
        if diff.abs() > 0.003 {
            self.hub_anim += diff * exp_fast;
            active = true;
        } else {
            self.hub_anim = self.hub_target;
        }

        // Hub 翻页/分类切换交叉过渡
        if (1.0 - self.page_anim).abs() > 0.004 {
            self.page_anim += (1.0 - self.page_anim) * exp_page;
            active = true;
        } else {
            self.page_anim = 1.0;
        }

        // Dock 图标 macOS 风格高斯波浪放大 + 临界阻尼平滑缩回 + 点击弹跳
        if self.mode == ViewMode::Dock {
            let (pill_h, base_icon, slot_base, hub_btn) = self.dock_metrics();
            let fixed_count = self.dock_indices.len();
            let count = fixed_count + self.dock_running_extra.len();
            let extra_sep = if self.dock_running_extra.is_empty() { 0.0 } else { 14.0 };
            let push_scale = base_icon * 1.04;
            let extra_w: f32 = self.dock_scales.iter().take(count).map(|s| (s - 1.0) * push_scale).sum();
            let main_pill_w = 24.0 + hub_btn + 16.0 + (count.max(1) as f32) * slot_base + extra_sep + 14.0 + extra_w;
            let total_w = main_pill_w + 14.0 + 446.0;
            let start_x = (self.logical_w - total_w) * 0.5;
            let mut cursor_x = start_x + 14.0 + hub_btn + 20.0;
            let pill_y = self.logical_h - pill_h - 2.0;

            let zoom_delta = self.config.dock_zoom - 1.0;
            let follow_rate = match self.config.anim_preset {
                0 => 8.5,
                2 => 16.0,
                _ => 12.0,
            };
            let k = 1.0 - (-dt * follow_rate).exp();

            for i in 0..count {
                if i == fixed_count && extra_sep > 0.0 {
                    cursor_x += extra_sep;
                }
                let s = self.dock_scales[i];
                let slot_w = slot_base + (s - 1.0) * push_scale;
                let cx = cursor_x + slot_w * 0.5;
                cursor_x += slot_w;

                let target = if self.context_menu.is_none()
                    && self.mouse_inside
                    && self.mouse_y >= pill_y - 38.0
                    && self.mouse_y <= pill_y + pill_h + 8.0
                    && self.mouse_x >= start_x
                    && self.mouse_x <= start_x + main_pill_w
                {
                    let dist = (self.mouse_x - cx).abs();
                    let sigma = 72.0f32;
                    if dist < 170.0 {
                        let gauss = (-0.5 * (dist / sigma) * (dist / sigma)).exp();
                        1.0 + zoom_delta * gauss
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                let diff = target - self.dock_scales[i];
                if diff.abs() > 0.0015 {
                    self.dock_scales[i] += diff * k;
                    active = true;
                } else {
                    self.dock_scales[i] = target;
                }
                self.dock_vels[i] = 0.0;

                if let Some(b) = self.dock_bounce.get_mut(i) {
                    if *b > 0.003 {
                        *b *= (-dt * 8.0).exp();
                        active = true;
                    } else {
                        *b = 0.0;
                    }
                }
            }
        }

        // Hub 卡片悬停指数缓动
        if self.mode == ViewMode::Hub {
            for (i, h) in self.card_hovers.iter_mut().enumerate() {
                let target = if matches!(self.hovered, HitTarget::HubGridApp(idx) if idx == i) {
                    1.0
                } else {
                    0.0
                };
                if (*h - target).abs() > 0.004 {
                    *h += (target - *h) * exp_fast;
                    active = true;
                } else {
                    *h = target;
                }
            }
        }

        active
    }

    pub fn hit_test(&self, x: f32, y: f32) -> HitTarget {
        // 反向遍历（上层优先）
        for (r, target) in self.hit_rects.iter().rev() {
            if x >= r.left && x <= r.right && y >= r.top && y <= r.bottom {
                return *target;
            }
        }
        if self.mode == ViewMode::Hub {
            HitTarget::HubBackdrop
        } else {
            HitTarget::None
        }
    }

    pub fn launch_app(&mut self, app_idx: usize) {
        self.trigger_dock_bounce(app_idx);
        if let Some(app) = self.apps.get_mut(app_idx) {
            app.launch_count += 1;
            let path = app.path.clone();
            self.refresh_quick_list();
            launch_path(&path);
        }
    }

    pub unsafe fn render(&mut self, gpu: &GpuRenderer) {
        self.ensure_fonts(gpu);
        self.hit_rects.clear();

        let ctx = &gpu.d2d_rt;
        ctx.BeginDraw();
        ctx.Clear(Some(&rgba(0, 0, 0, 0.0)));

        if self.mode == ViewMode::Dock && self.hub_anim <= 0.01 {
            self.render_dock(gpu);
        } else {
            self.render_hub(gpu);
        }

        let _ = ctx.EndDraw(None, None);
        let _ = gpu.present(true);
    }

    unsafe fn render_dock(&mut self, gpu: &GpuRenderer) {
        let ctx = &gpu.d2d_rt;
        let d2d_ctx = &gpu.d2d_ctx;
        let (pill_h, base_icon, slot_base, hub_btn) = self.dock_metrics();
        let fixed_count = self.dock_indices.len();
        let count = (fixed_count + self.dock_running_extra.len()).max(1);
        let extra_sep = if self.dock_running_extra.is_empty() { 0.0 } else { 14.0 };
        let push_scale = base_icon * 1.04;
        let extra_w: f32 = self.dock_scales.iter().take(count).map(|s| (s - 1.0) * push_scale).sum();
        let main_pill_w = 24.0 + hub_btn + 16.0 + (count as f32) * slot_base + extra_sep + 14.0 + extra_w;
        let side_pill_w = 446.0;
        let gap = 14.0;
        let total_w = main_pill_w + gap + side_pill_w;
        let start_x = (self.logical_w - total_w) * 0.5;
        let pill_y = self.logical_h - pill_h - 2.0;

        // 1. 左侧主岛（Dock 胶囊）
        let main_rect = rect_f(start_x, pill_y, main_pill_w, pill_h);
        draw_glass_panel(
            ctx,
            main_rect,
            (pill_h * 0.32).clamp(16.0, 28.0),
            rgba(16, 19, 28, self.config.glass_opacity),
            rgba(255, 255, 255, 0.14),
        );

        // 2. Hub 按钮（呼吸极光光环）并记录其在屏幕上的原点坐标
        let hub_pad_y = (pill_h - hub_btn) * 0.5;
        let hub_rect = rect_f(start_x + 14.0, pill_y + hub_pad_y, hub_btn, hub_btn);
        let dock_win_left = (self.screen_dip_w - self.logical_w) * 0.5;
        let dock_win_top = self.screen_dip_h - self.config.dock_bottom_gap as f32 - self.logical_h;
        self.hub_origin_x = dock_win_left + (hub_rect.left + hub_rect.right) * 0.5;
        self.hub_origin_y = dock_win_top + (hub_rect.top + hub_rect.bottom) * 0.5;
        let hub_hovered = self.hovered == HitTarget::DockHubBtn;
        let hub_bg = if hub_hovered {
            rgba(91, 140, 255, 0.28)
        } else {
            rgba(91, 140, 255, 0.16)
        };
        draw_rounded_fill(ctx, hub_rect, 14.0, hub_bg);
        draw_rounded_stroke(ctx, hub_rect, 14.0, rgba(125, 165, 255, 0.55), 1.2);
        // 绘制 2x2 极简启动台网格图标（自适应缩放）
        let dot_sz = (hub_btn * 0.19).clamp(6.5, 12.0);
        let dot_step = dot_sz * 1.32;
        let gx = hub_rect.left + (hub_btn - (dot_step + dot_sz)) * 0.5;
        let gy = hub_rect.top + (hub_btn - (dot_step + dot_sz)) * 0.5;
        if let Ok(dot_brush) = ctx.CreateSolidColorBrush(&rgba(235, 242, 255, 0.95), None) {
            for r in 0..2 {
                for c in 0..2 {
                    let dr = rect_f(gx + c as f32 * dot_step, gy + r as f32 * dot_step, dot_sz, dot_sz);
                    let rr = D2D1_ROUNDED_RECT {
                        rect: dr,
                        radiusX: 2.5,
                        radiusY: 2.5,
                    };
                    ctx.FillRoundedRectangle(&rr, &dot_brush);
                }
            }
        }
        self.hit_rects.push((hub_rect, HitTarget::DockHubBtn));

        // 分割线
        if let Ok(sep_brush) = ctx.CreateSolidColorBrush(&rgba(255, 255, 255, 0.12), None) {
            ctx.DrawLine(
                point_f(hub_rect.right + 10.0, pill_y + 14.0),
                point_f(hub_rect.right + 10.0, pill_y + pill_h - 14.0),
                &sep_brush,
                1.0,
                None,
            );
        }

        // 3. Dock 应用图标（macOS 风格：底边基准向上生长 + 水平推开 + 双三次插值 + 点击弹跳）
        let mut cursor_x = hub_rect.right + 20.0;
        let mut tooltip: Option<(f32, f32, String)> = None;
        let base_bottom = pill_y + pill_h - (pill_h - base_icon) * 0.5;

        for (i, &app_idx) in self.dock_indices.iter().enumerate() {
            let scale = *self.dock_scales.get(i).unwrap_or(&1.0);
            let bounce = *self.dock_bounce.get(i).unwrap_or(&0.0);
            let bounce_y = (bounce * std::f32::consts::PI * 2.0).sin().abs() * bounce * 20.0;
            let slot_w = slot_base + (scale - 1.0) * push_scale;
            let sz = base_icon * scale;
            let cx = cursor_x + slot_w * 0.5;
            cursor_x += slot_w;

            // 底边对齐向上生长（macOS Dock 核心手感）
            let bottom_y = base_bottom - (scale - 1.0) * 6.0 - bounce_y;
            let top_y = bottom_y - sz;
            let ir = rect_f(cx - sz * 0.5, top_y, sz, sz);

            let hit_r = rect_f(cx - slot_w * 0.5, pill_y - 14.0, slot_w, pill_h + 18.0);
            self.hit_rects.push((hit_r, HitTarget::DockApp(app_idx)));

            if let Some(app) = self.apps.get(app_idx) {
                if let Some(ref bmp) = app.bitmap {
                    d2d_ctx.DrawBitmap(
                        bmp,
                        Some(&ir),
                        1.0,
                        D2D1_INTERPOLATION_MODE_LINEAR,
                        None,
                        None,
                    );
                } else {
                    draw_rounded_fill(ctx, ir, 12.0, rgba(70, 80, 110, 0.65));
                }

                // 消息提示红点/数字角标
                if app.badge_count > 0 {
                    draw_notification_badge(ctx, self.fmt_badge.as_ref(), ir.right - 6.0, ir.top + 4.0, app.badge_count);
                }

                // 底部指示点（运行中常亮蓝点，悬停加宽）
                let is_running = self.find_running_for_app(app_idx).is_some();
                if is_running || app.launch_count > 0 || self.hovered == HitTarget::DockApp(app_idx) {
                    let dot_w = if self.hovered == HitTarget::DockApp(app_idx) { 12.0 } else { 5.0 };
                    let dot_r = rect_f(cx - dot_w * 0.5, pill_y + pill_h - 7.5, dot_w, 3.0);
                    let dot_col = if is_running {
                        rgba(105, 175, 255, 0.98)
                    } else {
                        rgba(180, 195, 225, 0.72)
                    };
                    draw_rounded_fill(ctx, dot_r, 1.5, dot_col);
                }

                if self.hovered == HitTarget::DockApp(app_idx) {
                    tooltip = Some((cx, ir.top - 34.0, app.name.clone()));
                }
            }
        }

        // 3.5 动态展示当前正在运行但未固定在 Dock 上的应用（如 QQ 等，点击可直接唤起）
        if !self.dock_running_extra.is_empty() {
            if let Ok(sep_brush) = ctx.CreateSolidColorBrush(&rgba(255, 255, 255, 0.14), None) {
                ctx.DrawLine(
                    point_f(cursor_x + 6.0, pill_y + 18.0),
                    point_f(cursor_x + 6.0, pill_y + pill_h - 18.0),
                    &sep_brush,
                    1.0,
                    None,
                );
            }
            cursor_x += 14.0;
            let extra_list = self.dock_running_extra.clone();
            for (k, r_idx) in extra_list.into_iter().enumerate() {
                let slot_idx = fixed_count + k;
                let scale = *self.dock_scales.get(slot_idx).unwrap_or(&1.0);
                let slot_w = slot_base + (scale - 1.0) * push_scale;
                let sz = base_icon * scale;
                let cx = cursor_x + slot_w * 0.5;
                cursor_x += slot_w;

                let bottom_y = base_bottom - (scale - 1.0) * 6.0;
                let ir = rect_f(cx - sz * 0.5, bottom_y - sz, sz, sz);
                let hit_r = rect_f(cx - slot_w * 0.5, pill_y - 14.0, slot_w, pill_h + 18.0);
                self.hit_rects.push((hit_r, HitTarget::DockRunning(r_idx)));

                if let Some(rw) = self.running.get(r_idx) {
                    let bmp_opt = rw
                        .app_icon_idx
                        .and_then(|idx| self.apps.get(idx))
                        .and_then(|a| a.bitmap.as_ref())
                        .or(rw.custom_bitmap.as_ref());
                    if let Some(bmp) = bmp_opt {
                        d2d_ctx.DrawBitmap(
                            bmp,
                            Some(&ir),
                            1.0,
                            D2D1_INTERPOLATION_MODE_LINEAR,
                            None,
                            None,
                        );
                    } else {
                        draw_rounded_fill(ctx, ir, 12.0, rgba(70, 85, 120, 0.7));
                    }
                    if rw.unread_count > 0 {
                        draw_notification_badge(ctx, self.fmt_badge.as_ref(), ir.right - 6.0, ir.top + 4.0, rw.unread_count);
                    }
                    let dot_w = if self.hovered == HitTarget::DockRunning(r_idx) { 12.0 } else { 5.0 };
                    let dot_r = rect_f(cx - dot_w * 0.5, pill_y + pill_h - 7.5, dot_w, 3.0);
                    draw_rounded_fill(ctx, dot_r, 1.5, rgba(105, 175, 255, 0.98));

                    if self.hovered == HitTarget::DockRunning(r_idx) {
                        tooltip = Some((cx, ir.top - 34.0, rw.exe_name.clone()));
                    }
                }
            }
        }

        // 悬浮名称气泡 或 多子窗口切换弹窗
        let mut sub_menu_candidate: Option<(f32, usize)> = None;
        if let HitTarget::DockApp(app_idx) = self.hovered {
            if let Some(r_idx) = self.running.iter().position(|rw| rw.app_icon_idx == Some(app_idx)) {
                if self.running[r_idx].sub_windows.len() >= 2 {
                    if let Some((cx, _, _)) = tooltip {
                        sub_menu_candidate = Some((cx, r_idx));
                    }
                }
            }
        } else if let HitTarget::DockRunning(r_idx) = self.hovered {
            if self.running.get(r_idx).map_or(false, |rw| rw.sub_windows.len() >= 2) {
                if let Some((cx, _, _)) = tooltip {
                    sub_menu_candidate = Some((cx, r_idx));
                }
            }
        }

        if let Some((cx, r_idx)) = sub_menu_candidate.or(self.dock_hover_sub) {
            if self.context_menu.is_none() {
                if let Some(rw) = self.running.get(r_idx).cloned() {
                    if rw.sub_windows.len() >= 2 {
                        let shown_subs: Vec<_> = rw.sub_windows.iter().take(6).collect();
                        let sw_w = 250.0;
                        let item_h = 32.0;
                        let sw_h = 32.0 + shown_subs.len() as f32 * item_h;
                        let sx = cx.clamp(12.0 + sw_w * 0.5, (self.logical_w - sw_w * 0.5 - 12.0).max(12.0 + sw_w * 0.5)) - sw_w * 0.5;
                        let sy = (pill_y - sw_h - 14.0).max(4.0);
                        let sr = rect_f(sx, sy, sw_w, sw_h);
                        self.dock_sub_menu_rect = sr;
                        draw_glass_panel_gradient(
                            ctx,
                            sr,
                            14.0,
                            rgba(24, 29, 42, 0.96),
                            rgba(15, 18, 28, 0.98),
                            rgba(255, 255, 255, 0.18),
                        );

                        let hdr_r = rect_f(sx + 12.0, sy + 6.0, sw_w - 24.0, 20.0);
                        if let Some(ref fmt) = self.fmt_badge {
                            draw_text(
                                ctx,
                                fmt,
                                &format!("{} ({} 个窗口)", rw.exe_name, rw.sub_windows.len()),
                                hdr_r,
                                rgba(145, 185, 255, 0.9),
                            );
                        }

                        let mut iy = sy + 28.0;
                        for sw in shown_subs {
                            let row_r = rect_f(sx + 6.0, iy, sw_w - 12.0, item_h - 2.0);
                            let close_r = rect_f(row_r.right - 26.0, row_r.top + 3.0, 22.0, 24.0);
                            let row_hov = self.hovered == HitTarget::DockSubWin(sw.hwnd);
                            let close_hov = self.hovered == HitTarget::DockSubWinClose(sw.hwnd);

                            if row_hov || close_hov {
                                draw_rounded_fill(ctx, row_r, 8.0, rgba(91, 140, 255, 0.24));
                            }
                            let tr = rect_f(row_r.left + 8.0, row_r.top, close_r.left - row_r.left - 12.0, row_r.bottom - row_r.top);
                            if let Some(ref fmt) = self.fmt_small {
                                draw_text(ctx, fmt, &sw.title, tr, rgba(238, 242, 255, 0.94));
                            }

                            let cbg = if close_hov {
                                rgba(235, 80, 95, 0.45)
                            } else {
                                rgba(255, 255, 255, 0.06)
                            };
                            draw_rounded_fill(ctx, close_r, 6.0, cbg);
                            if let Some(ref fmt) = self.fmt_center_small {
                                draw_text(ctx, fmt, "✕", close_r, rgba(240, 205, 210, 0.9));
                            }

                            let click_r = rect_f(row_r.left, row_r.top, close_r.left - row_r.left - 2.0, row_r.bottom - row_r.top);
                            self.hit_rects.push((click_r, HitTarget::DockSubWin(sw.hwnd)));
                            self.hit_rects.push((close_r, HitTarget::DockSubWinClose(sw.hwnd)));
                            iy += item_h;
                        }
                        tooltip = None;
                    }
                }
            }
        } else {
            self.dock_sub_menu_rect = rect_f(0.0, 0.0, 0.0, 0.0);
        }

        if let Some((cx, ty, name)) = tooltip {
            let tw = (name.chars().count() as f32 * 9.5 + 26.0).clamp(68.0, 230.0);
            let tr = rect_f(cx - tw * 0.5, ty.max(4.0), tw, 26.0);
            draw_glass_panel(ctx, tr, 9.0, rgba(22, 26, 38, 0.94), rgba(255, 255, 255, 0.18));
            if let Some(ref fmt) = self.fmt_center_small {
                draw_text(ctx, fmt, &name, tr, rgba(242, 245, 255, 0.96));
            }
        }

        // 4. 右侧伴生系统状态岛（完全替代系统任务栏托盘与时钟，垂直居中随 Dock 高度自动对齐）
        let side_x = start_x + main_pill_w + gap;
        let side_rect = rect_f(side_x, pill_y, side_pill_w, pill_h);
        draw_glass_panel(
            ctx,
            side_rect,
            (pill_h * 0.32).clamp(16.0, 28.0),
            rgba(16, 19, 28, self.config.glass_opacity),
            rgba(255, 255, 255, 0.14),
        );
        let btn_y = pill_y + (pill_h - 34.0) * 0.5;

        // 快捷键胶囊（点击呼出 Hub）
        let badge_r = rect_f(side_x + 12.0, btn_y, 74.0, 34.0);
        let badge_bg = if self.hovered == HitTarget::DockHubBtn {
            rgba(91, 140, 255, 0.25)
        } else {
            rgba(255, 255, 255, 0.07)
        };
        draw_rounded_fill(ctx, badge_r, 11.0, badge_bg);
        if let Some(ref fmt) = self.fmt_badge {
            draw_text(ctx, fmt, "Alt+Space", badge_r, rgba(190, 208, 245, 0.92));
        }
        self.hit_rects.push((badge_r, HitTarget::DockHubBtn));

        // 网络状态按钮
        let net_r = rect_f(side_x + 92.0, btn_y, 30.0, 34.0);
        let net_bg = if self.hovered == HitTarget::SysNet {
            rgba(91, 140, 255, 0.25)
        } else {
            rgba(255, 255, 255, 0.05)
        };
        draw_rounded_fill(ctx, net_r, 10.0, net_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            draw_text(ctx, fmt, "🌐", net_r, rgba(160, 210, 255, 0.94));
        }
        self.hit_rects.push((net_r, HitTarget::SysNet));

        // 音量按钮（点击静音，滚轮调节）
        let vol_r = rect_f(side_x + 126.0, btn_y, 54.0, 34.0);
        self.sys_vol_rect = vol_r;
        let vol_bg = if self.hovered == HitTarget::SysVol {
            rgba(91, 140, 255, 0.25)
        } else {
            rgba(255, 255, 255, 0.05)
        };
        draw_rounded_fill(ctx, vol_r, 10.0, vol_bg);
        if let Some(ref fmt) = self.fmt_badge {
            let v_txt = if self.sys_status.volume_pct == 0 {
                "🔇 0".to_string()
            } else {
                format!("🔊 {}", self.sys_status.volume_pct)
            };
            let v_col = if self.sys_status.volume_pct == 0 {
                rgba(240, 150, 155, 0.9)
            } else {
                rgba(220, 232, 252, 0.92)
            };
            draw_text(ctx, fmt, &v_txt, vol_r, v_col);
        }
        self.hit_rects.push((vol_r, HitTarget::SysVol));

        // 输入法中/英徽标
        let ime_r = rect_f(side_x + 184.0, btn_y, 28.0, 34.0);
        let ime_bg = if self.hovered == HitTarget::SysIme {
            rgba(91, 140, 255, 0.25)
        } else {
            rgba(255, 255, 255, 0.05)
        };
        draw_rounded_fill(ctx, ime_r, 10.0, ime_bg);
        if let Some(ref fmt) = self.fmt_badge {
            let ime_txt = if self.sys_status.ime_cn { "中" } else { "英" };
            draw_text(ctx, fmt, ime_txt, ime_r, rgba(225, 235, 255, 0.94));
        }
        self.hit_rects.push((ime_r, HitTarget::SysIme));

        // 双行时间日期
        let clk_r = rect_f(side_x + 216.0, pill_y + (pill_h - 42.0) * 0.5, 56.0, 42.0);
        if self.hovered == HitTarget::SysClock {
            draw_rounded_fill(ctx, clk_r, 10.0, rgba(255, 255, 255, 0.09));
        }
        if let Some(ref fmt) = self.fmt_badge {
            let t_r = rect_f(clk_r.left, clk_r.top + 2.0, 56.0, 20.0);
            let d_r = rect_f(clk_r.left, clk_r.top + 20.0, 56.0, 18.0);
            draw_text(ctx, fmt, &self.sys_status.time_str, t_r, rgba(242, 246, 255, 0.96));
            draw_text(ctx, fmt, &self.sys_status.date_str, d_r, rgba(165, 178, 206, 0.82));
        }
        self.hit_rects.push((clk_r, HitTarget::SysClock));

        // 竖向微分割线
        if let Ok(sep_brush) = ctx.CreateSolidColorBrush(&rgba(255, 255, 255, 0.12), None) {
            ctx.DrawLine(
                point_f(side_x + 278.0, pill_y + 14.0),
                point_f(side_x + 278.0, pill_y + pill_h - 14.0),
                &sep_brush,
                1.0,
                None,
            );
        }

        // 设置按钮
        let set_r = rect_f(side_x + 286.0, btn_y, 34.0, 34.0);
        let set_bg = if self.hovered == HitTarget::DockSettings {
            rgba(91, 140, 255, 0.28)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, set_r, 11.0, set_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            draw_text(ctx, fmt, "⚙", set_r, rgba(210, 222, 245, 0.92));
        }
        self.hit_rects.push((set_r, HitTarget::DockSettings));

        // 刷新按钮
        let ref_r = rect_f(side_x + 324.0, btn_y, 34.0, 34.0);
        let ref_bg = if self.hovered == HitTarget::DockRefresh {
            rgba(255, 255, 255, 0.16)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, ref_r, 11.0, ref_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            draw_text(ctx, fmt, "↻", ref_r, rgba(210, 220, 240, 0.9));
        }
        self.hit_rects.push((ref_r, HitTarget::DockRefresh));

        // 电源按钮（锁定/睡眠/重启/关机）
        let pwr_r = rect_f(side_x + 362.0, btn_y, 34.0, 34.0);
        let pwr_bg = if self.power_menu_open || self.hovered == HitTarget::DockPower {
            rgba(255, 165, 85, 0.28)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, pwr_r, 11.0, pwr_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            draw_text(ctx, fmt, "⏻", pwr_r, rgba(255, 210, 155, 0.94));
        }
        self.hit_rects.push((pwr_r, HitTarget::DockPower));

        // 退出按钮
        let exit_r = rect_f(side_x + 400.0, btn_y, 34.0, 34.0);
        let exit_bg = if self.hovered == HitTarget::DockExit {
            rgba(235, 80, 95, 0.35)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, exit_r, 11.0, exit_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            draw_text(ctx, fmt, "✕", exit_r, rgba(240, 175, 180, 0.92));
        }
        self.hit_rects.push((exit_r, HitTarget::DockExit));

        // 4.5 电源弹窗菜单
        if self.power_menu_open {
            let full_r = rect_f(0.0, 0.0, self.logical_w, self.logical_h);
            self.hit_rects.push((full_r, HitTarget::CtxDismiss));

            let items = [
                ("锁定屏幕", HitTarget::PowerLock, false),
                ("睡眠", HitTarget::PowerSleep, false),
                ("重新启动", HitTarget::PowerRestart, true),
                ("关机", HitTarget::PowerShutdown, true),
            ];
            let mw = 148.0;
            let item_h = 34.0;
            let mh = 16.0 + items.len() as f32 * item_h;
            let mx = (pwr_r.right - mw + 16.0).clamp(12.0, (self.logical_w - mw - 12.0).max(12.0));
            let my = (pill_y - mh - 10.0).max(6.0);
            let mr = rect_f(mx, my, mw, mh);

            draw_glass_panel_gradient(
                ctx,
                mr,
                14.0,
                rgba(26, 31, 45, 0.96),
                rgba(17, 20, 30, 0.98),
                rgba(255, 255, 255, 0.18),
            );

            let mut iy = my + 8.0;
            for (lbl, target, is_danger) in items {
                let ir = rect_f(mx + 6.0, iy, mw - 12.0, item_h - 2.0);
                if self.hovered == target {
                    let bg = if is_danger {
                        rgba(235, 75, 90, 0.35)
                    } else {
                        rgba(91, 140, 255, 0.28)
                    };
                    draw_rounded_fill(ctx, ir, 8.0, bg);
                }
                if let Some(ref fmt) = self.fmt_small {
                    let tr = rect_f(ir.left + 12.0, ir.top, ir.right - ir.left - 18.0, ir.bottom - ir.top);
                    let col = if is_danger {
                        rgba(255, 195, 195, 0.95)
                    } else {
                        rgba(238, 242, 255, 0.94)
                    };
                    draw_text(ctx, fmt, lbl, tr, col);
                }
                self.hit_rects.push((ir, target));
                iy += item_h;
            }
        }

        // 5. Dock 图标右键菜单（Windows 风格 + 隐藏图标）
        if let Some(ref menu) = self.context_menu.clone() {
            let full_r = rect_f(0.0, 0.0, self.logical_w, self.logical_h);
            self.hit_rects.push((full_r, HitTarget::CtxDismiss));

            let has_running = menu.running_idx.is_some();
            let item_h = 34.0;
            let mut items: Vec<(&str, HitTarget, bool)> = vec![
                ("打开", HitTarget::CtxOpen, false),
                ("以管理员身份运行", HitTarget::CtxAdmin, false),
                ("打开文件所在位置", HitTarget::CtxLocation, false),
                ("从 Dock 隐藏此图标", HitTarget::CtxHide, false),
            ];
            if has_running {
                items.push(("关闭应用", HitTarget::CtxCloseApp, true));
            }
            let mw = 196.0;
            let mh = 16.0 + items.len() as f32 * item_h;
            let mx = menu.x.clamp(12.0, (self.logical_w - mw - 12.0).max(12.0));
            let my = (pill_y - mh - 10.0).max(6.0);
            let mr = rect_f(mx, my, mw, mh);

            draw_glass_panel_gradient(
                ctx,
                mr,
                14.0,
                rgba(26, 31, 45, 0.96),
                rgba(17, 20, 30, 0.98),
                rgba(255, 255, 255, 0.18),
            );

            let mut iy = my + 8.0;
            for (lbl, target, is_danger) in items {
                let ir = rect_f(mx + 6.0, iy, mw - 12.0, item_h - 2.0);
                let hov = self.hovered == target;
                if hov {
                    let bg = if is_danger {
                        rgba(235, 75, 90, 0.35)
                    } else {
                        rgba(91, 140, 255, 0.28)
                    };
                    draw_rounded_fill(ctx, ir, 8.0, bg);
                }
                if let Some(ref fmt) = self.fmt_small {
                    let tr = rect_f(ir.left + 12.0, ir.top, ir.right - ir.left - 18.0, ir.bottom - ir.top);
                    let col = if is_danger {
                        rgba(255, 185, 190, 0.96)
                    } else {
                        rgba(238, 242, 255, 0.94)
                    };
                    draw_text(ctx, fmt, lbl, tr, col);
                }
                self.hit_rects.push((ir, target));
                iy += item_h;
            }
        }
    }

    unsafe fn render_hub(&mut self, gpu: &GpuRenderer) {
        let ctx = &gpu.d2d_rt;
        let d2d_ctx = &gpu.d2d_ctx;
        let t = self.hub_anim.clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3);

        // 全屏半透明深色遮罩
        let full_r = rect_f(0.0, 0.0, self.logical_w, self.logical_h);
        if let Ok(bg_brush) = ctx.CreateSolidColorBrush(&rgba(6, 8, 14, 0.56 * ease), None) {
            ctx.FillRectangle(&full_r, &bg_brush);
        }

        // 从 Dock Hub 按钮坐标缩放展开到全屏工作台（收起时反向缩回图标）
        let pad = 20.0;
        let target_w = (self.logical_w - pad * 2.0).max(800.0);
        let target_h = (self.logical_h - pad * 2.0).max(560.0);
        let pw = 56.0 + (target_w - 56.0) * ease;
        let ph = 56.0 + (target_h - 56.0) * ease;
        let cx = self.hub_origin_x + (self.logical_w * 0.5 - self.hub_origin_x) * ease;
        let cy = self.hub_origin_y + (self.logical_h * 0.5 - self.hub_origin_y) * ease;
        let px = cx - pw * 0.5;
        let py = cy - ph * 0.5;
        let panel_r = rect_f(px, py, pw, ph);

        draw_glass_panel_gradient(
            ctx,
            panel_r,
            24.0,
            rgba(16, 20, 31, (self.config.glass_opacity + 0.04).min(0.96) * ease),
            rgba(10, 13, 21, (self.config.glass_opacity + 0.06).min(0.98) * ease),
            rgba(255, 255, 255, 0.14 * ease),
        );

        if ease < 0.72 {
            return;
        }

        // 1. 顶部聚光灯栏
        let top_y = py + 24.0;
        let title_r = rect_f(px + 28.0, top_y, 175.0, 42.0);
        if let Some(ref fmt) = self.fmt_title {
            let hdr = if self.hub_settings { "xDock 设置" } else { "xDock Hub" };
            draw_text(ctx, fmt, hdr, title_r, rgba(240, 244, 255, 0.96));
        }

        // 右上角 设置 / 返回启动台 切换按钮
        let hub_set_r = rect_f(px + pw - 116.0, top_y + 4.0, 90.0, 34.0);
        let hub_set_bg = if self.hub_settings {
            rgba(91, 140, 255, 0.32)
        } else if self.hovered == HitTarget::HubSettingsBtn {
            rgba(255, 255, 255, 0.14)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, hub_set_r, 11.0, hub_set_bg);
        draw_rounded_stroke(ctx, hub_set_r, 11.0, rgba(255, 255, 255, 0.14), 1.0);
        if let Some(ref fmt) = self.fmt_center_small {
            let lbl = if self.hub_settings { "← 返回" } else { "⚙ 设置" };
            draw_text(ctx, fmt, lbl, hub_set_r, rgba(235, 242, 255, 0.95));
        }
        self.hit_rects.push((hub_set_r, HitTarget::HubSettingsBtn));

        if self.hub_settings {
            self.render_settings_panel(ctx, px, py, pw, ph);
            return;
        }

        // 居中大号搜索框
        let search_w = (pw * 0.30).clamp(210.0, 360.0);
        let search_x = px + 210.0;
        let search_r = rect_f(search_x, top_y, search_w, 42.0);
        let search_stroke = if self.search_focused {
            rgba(105, 155, 255, 0.75)
        } else {
            rgba(255, 255, 255, 0.14)
        };
        draw_rounded_fill(ctx, search_r, 14.0, rgba(255, 255, 255, 0.06));
        draw_rounded_stroke(ctx, search_r, 14.0, search_stroke, 1.2);
        let search_text = if self.search_query.is_empty() {
            "键入应用名称快速搜索...".to_string()
        } else {
            format!("{}|", self.search_query)
        };
        let search_col = if self.search_query.is_empty() {
            rgba(150, 160, 185, 0.72)
        } else {
            rgba(245, 248, 255, 0.98)
        };
        if let Some(ref fmt) = self.fmt_body {
            let text_r = rect_f(search_r.left + 16.0, search_r.top, search_w - 32.0, 42.0);
            draw_text(ctx, fmt, &search_text, text_r, search_col);
        }
        self.hit_rects.push((search_r, HitTarget::HubSearchBox));

        // 分类胶囊标签
        let mut tab_x = search_r.right + 14.0;
        for cat in AppCategory::TABS {
            let tab_w = 70.0;
            if tab_x + tab_w > hub_set_r.left - 12.0 {
                break;
            }
            let tr = rect_f(tab_x, top_y + 4.0, tab_w, 34.0);
            let active = self.category == cat;
            let hovered = self.hovered == HitTarget::HubCategory(cat);
            let bg = if active {
                rgba(91, 140, 255, 0.32)
            } else if hovered {
                rgba(255, 255, 255, 0.11)
            } else {
                rgba(255, 255, 255, 0.04)
            };
            draw_rounded_fill(ctx, tr, 11.0, bg);
            if active {
                draw_rounded_stroke(ctx, tr, 11.0, rgba(130, 175, 255, 0.6), 1.0);
            }
            if let Some(ref fmt) = self.fmt_center_small {
                let col = if active {
                    rgba(245, 248, 255, 0.98)
                } else {
                    rgba(185, 195, 218, 0.85)
                };
                draw_text(ctx, fmt, cat.label(), tr, col);
            }
            self.hit_rects.push((tr, HitTarget::HubCategory(cat)));
            tab_x += tab_w + 6.0;
        }

        // 2. 主体三栏区（左1：正在运行 全高 | 左2：快速直达 全高 | 右：应用网格）
        let body_y = top_y + 62.0;
        let body_h = ph - 134.0;
        let col_w = 236.0;

        // 左1栏：正在运行的应用（全高 + 滚轮滚动 + 确认关闭）
        let run_r = rect_f(px + 24.0, body_y, col_w, body_h);
        self.running_panel_rect = run_r;
        draw_rounded_fill(ctx, run_r, 18.0, rgba(255, 255, 255, 0.035));
        draw_rounded_stroke(ctx, run_r, 18.0, rgba(255, 255, 255, 0.08), 1.0);

        let max_vis = (((body_h - 52.0) / 42.0).floor() as usize).max(1);
        if self.running.len() <= max_vis {
            self.running_scroll = 0;
        } else if self.running_scroll + max_vis > self.running.len() {
            self.running_scroll = self.running.len() - max_vis;
        }

        let run_hdr_text = if self.running.len() > max_vis {
            format!("正在运行 ({}) · 滚轮", self.running.len())
        } else {
            format!("正在运行 ({})", self.running.len())
        };
        let run_hdr = rect_f(run_r.left + 14.0, run_r.top + 12.0, col_w - 28.0, 22.0);
        if let Some(ref fmt) = self.fmt_small {
            draw_text(ctx, fmt, &run_hdr_text, run_hdr, rgba(130, 190, 255, 0.92));
        }

        let mut ry = run_r.top + 42.0;
        let running_snapshot = self.running.clone();
        for (r_idx, rw) in running_snapshot
            .iter()
            .enumerate()
            .skip(self.running_scroll)
            .take(max_vis)
        {
            let row_r = rect_f(run_r.left + 8.0, ry, col_w - 16.0, 38.0);
            let close_r = rect_f(row_r.right - 28.0, row_r.top + 5.0, 24.0, 28.0);
            let row_hov = self.hovered == HitTarget::HubRunningApp(r_idx);
            let close_hov = self.hovered == HitTarget::HubRunningClose(r_idx);

            if row_hov || close_hov {
                draw_rounded_fill(ctx, row_r, 10.0, rgba(255, 255, 255, 0.07));
            }

            let ir = rect_f(row_r.left + 8.0, row_r.top + 7.0, 24.0, 24.0);
            let run_bmp = rw
                .app_icon_idx
                .and_then(|idx| self.apps.get(idx))
                .and_then(|a| a.bitmap.as_ref())
                .or(rw.custom_bitmap.as_ref());
            if let Some(bmp) = run_bmp {
                d2d_ctx.DrawBitmap(
                    bmp,
                    Some(&ir),
                    1.0,
                    D2D1_INTERPOLATION_MODE_LINEAR,
                    None,
                    None,
                );
            } else {
                draw_rounded_fill(ctx, ir, 7.0, rgba(91, 140, 255, 0.25));
            }

            let nr = rect_f(ir.right + 8.0, row_r.top, close_r.left - ir.right - 12.0, 38.0);
            if let Some(ref fmt) = self.fmt_small {
                draw_text(ctx, fmt, &rw.exe_name, nr, rgba(235, 240, 252, 0.92));
            }

            let close_bg = if close_hov {
                rgba(235, 80, 95, 0.42)
            } else {
                rgba(255, 255, 255, 0.06)
            };
            draw_rounded_fill(ctx, close_r, 8.0, close_bg);
            if let Some(ref fmt) = self.fmt_center_small {
                let col = if close_hov {
                    rgba(255, 220, 225, 0.98)
                } else {
                    rgba(185, 195, 215, 0.78)
                };
                draw_text(ctx, fmt, "✕", close_r, col);
            }

            let click_row_r = rect_f(row_r.left, row_r.top, close_r.left - row_r.left - 2.0, row_r.bottom - row_r.top);
            self.hit_rects.push((click_row_r, HitTarget::HubRunningApp(r_idx)));
            self.hit_rects.push((close_r, HitTarget::HubRunningClose(r_idx)));
            ry += 42.0;
        }

        // 左2栏：快速直达（独立全高竖栏 + 滚轮滚动 + 编辑开关）
        let side_r = rect_f(run_r.right + 16.0, body_y, col_w, body_h);
        self.quick_panel_rect = side_r;
        draw_rounded_fill(ctx, side_r, 18.0, rgba(255, 255, 255, 0.035));
        draw_rounded_stroke(ctx, side_r, 18.0, rgba(255, 255, 255, 0.08), 1.0);

        let max_q_vis = (((body_h - 52.0) / 42.0).floor() as usize).max(1);
        if self.quick_indices.len() <= max_q_vis {
            self.quick_scroll = 0;
        } else if self.quick_scroll + max_q_vis > self.quick_indices.len() {
            self.quick_scroll = self.quick_indices.len() - max_q_vis;
        }

        let quick_hdr_txt = if self.quick_indices.len() > max_q_vis {
            format!("快速直达 ({}) · 滚轮", self.quick_indices.len())
        } else {
            "快速直达".to_string()
        };
        let side_hdr = rect_f(side_r.left + 14.0, side_r.top + 12.0, col_w - 86.0, 22.0);
        if let Some(ref fmt) = self.fmt_small {
            draw_text(ctx, fmt, &quick_hdr_txt, side_hdr, rgba(155, 170, 200, 0.85));
        }

        let edit_btn_r = rect_f(side_r.right - 66.0, side_r.top + 10.0, 56.0, 24.0);
        let edit_bg = if self.quick_edit_mode {
            rgba(91, 140, 255, 0.36)
        } else if self.hovered == HitTarget::ToggleQuickEdit {
            rgba(255, 255, 255, 0.14)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, edit_btn_r, 8.0, edit_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            let lbl = if self.quick_edit_mode { "✓ 完成" } else { "✎ 编辑" };
            draw_text(ctx, fmt, lbl, edit_btn_r, rgba(225, 234, 255, 0.92));
        }
        self.hit_rects.push((edit_btn_r, HitTarget::ToggleQuickEdit));

        let mut qy = side_r.top + 42.0;
        let quick_list: Vec<usize> = self
            .quick_indices
            .iter()
            .skip(self.quick_scroll)
            .take(max_q_vis)
            .copied()
            .collect();
        for app_idx in quick_list {
            let qr = rect_f(side_r.left + 8.0, qy, col_w - 16.0, 38.0);
            let hovered = self.hovered == HitTarget::HubQuickApp(app_idx)
                || self.hovered == HitTarget::QuickRemove(app_idx);
            if hovered {
                draw_rounded_fill(ctx, qr, 10.0, rgba(91, 140, 255, 0.20));
            }
            if let Some(app) = self.apps.get(app_idx) {
                let ir = rect_f(qr.left + 8.0, qr.top + 7.0, 24.0, 24.0);
                if let Some(ref bmp) = app.bitmap {
                    d2d_ctx.DrawBitmap(
                        bmp,
                        Some(&ir),
                        1.0,
                        D2D1_INTERPOLATION_MODE_LINEAR,
                        None,
                        None,
                    );
                } else {
                    draw_rounded_fill(ctx, ir, 7.0, rgba(80, 95, 130, 0.6));
                }
                let text_right = if self.quick_edit_mode { qr.right - 34.0 } else { qr.right - 8.0 };
                let nr = rect_f(ir.right + 8.0, qr.top, text_right - ir.right - 8.0, 38.0);
                if let Some(ref fmt) = self.fmt_small {
                    draw_text(ctx, fmt, &app.name, nr, rgba(235, 240, 252, 0.94));
                }
            }
            if self.quick_edit_mode {
                let rm_r = rect_f(qr.right - 28.0, qr.top + 5.0, 24.0, 28.0);
                let rm_bg = if self.hovered == HitTarget::QuickRemove(app_idx) {
                    rgba(235, 80, 95, 0.42)
                } else {
                    rgba(255, 255, 255, 0.08)
                };
                draw_rounded_fill(ctx, rm_r, 8.0, rm_bg);
                if let Some(ref fmt) = self.fmt_center_small {
                    draw_text(ctx, fmt, "✕", rm_r, rgba(245, 210, 215, 0.92));
                }
                self.hit_rects.push((rm_r, HitTarget::QuickRemove(app_idx)));
            } else {
                self.hit_rects.push((qr, HitTarget::HubQuickApp(app_idx)));
            }
            qy += 42.0;
        }

        // 右侧主网格区 (config.hub_cols x 5)
        let grid_x = side_r.right + 20.0;
        let grid_w = (px + pw - 24.0) - grid_x;
        let cols = self.config.hub_cols;
        let rows = 5usize;
        let cell_gap = 12.0;
        let cell_w = (grid_w - cell_gap * (cols as f32 - 1.0)) / cols as f32;
        let cell_h = (body_h - cell_gap * (rows as f32 - 1.0)) / rows as f32;

        let per_page = cols * rows;
        let start_idx = self.page * per_page;
        let page_items: Vec<usize> = self
            .filtered_indices
            .iter()
            .skip(start_idx)
            .take(per_page)
            .copied()
            .collect();

        for (slot, app_idx) in page_items.iter().enumerate() {
            let r = slot / cols;
            let c = slot % cols;
            let hover_val = *self.card_hovers.get(*app_idx).unwrap_or(&0.0);
            let is_selected = self.selected_grid_idx == Some(slot);

            let slide_x = (1.0 - self.page_anim) * 28.0 * self.page_dir;
            let cx = grid_x + c as f32 * (cell_w + cell_gap) + slide_x;
            let cy = body_y + r as f32 * (cell_h + cell_gap) - hover_val * 4.0;
            let card_r = rect_f(cx, cy, cell_w, cell_h);

            let card_bg = if hover_val > 0.01 {
                rgba(255, 255, 255, 0.04 + 0.10 * hover_val)
            } else if is_selected {
                rgba(91, 140, 255, 0.14)
            } else {
                rgba(255, 255, 255, 0.03)
            };
            draw_rounded_fill(ctx, card_r, 16.0, card_bg);

            if hover_val > 0.05 || is_selected {
                let border_a = if is_selected { 0.45 } else { 0.28 * hover_val };
                draw_rounded_stroke(ctx, card_r, 16.0, rgba(135, 175, 255, border_a), 1.1);
            }

            if let Some(app) = self.apps.get(*app_idx) {
                let icon_sz = (cell_h * 0.44).clamp(38.0, 52.0) * (1.0 + 0.14 * hover_val);
                let ix = cx + (cell_w - icon_sz) * 0.5;
                let iy = cy + (cell_h * 0.58 - icon_sz) * 0.5 + 4.0;
                let ir = rect_f(ix, iy, icon_sz, icon_sz);

                if let Some(ref bmp) = app.bitmap {
                    d2d_ctx.DrawBitmap(
                        bmp,
                        Some(&ir),
                        self.page_anim.clamp(0.2, 1.0),
                        D2D1_INTERPOLATION_MODE_LINEAR,
                        None,
                        None,
                    );
                } else {
                    draw_rounded_fill(ctx, ir, 12.0, rgba(80, 95, 130, 0.65));
                }

                let label_r = rect_f(cx + 6.0, cy + cell_h * 0.64, cell_w - 12.0, cell_h * 0.32);
                if let Some(ref fmt) = self.fmt_center_small {
                    draw_text(ctx, fmt, &app.name, label_r, rgba(232, 238, 252, 0.92));
                }

                // 主页图标消息提示角标
                if app.badge_count > 0 && !self.quick_edit_mode {
                    draw_notification_badge(ctx, self.fmt_badge.as_ref(), ir.right - 2.0, ir.top + 2.0, app.badge_count);
                }

                if self.quick_edit_mode {
                    let is_pinned = self.quick_indices.contains(app_idx);
                    let pin_r = rect_f(card_r.right - 28.0, card_r.top + 6.0, 22.0, 22.0);
                    let pin_bg = if is_pinned {
                        rgba(91, 140, 255, 0.78)
                    } else {
                        rgba(255, 255, 255, 0.12)
                    };
                    draw_rounded_fill(ctx, pin_r, 7.0, pin_bg);
                    if let Some(ref fmt) = self.fmt_center_small {
                        draw_text(
                            ctx,
                            fmt,
                            if is_pinned { "★" } else { "+" },
                            pin_r,
                            rgba(245, 248, 255, 0.96),
                        );
                    }
                }
            }

            self.hit_rects.push((card_r, HitTarget::HubGridApp(*app_idx)));
        }

        // 3. 底部翻页指示器 & 操作提示
        let bot_y = py + ph - 36.0;
        let dot_spacing = 18.0;
        let dots_total_w = self.total_pages as f32 * dot_spacing;
        let mut dx = px + (pw - dots_total_w) * 0.5;
        for p in 0..self.total_pages {
            let active = p == self.page;
            let dw = if active { 20.0 } else { 8.0 };
            let dr = rect_f(dx, bot_y + 4.0, dw, 8.0);
            let col = if active {
                rgba(105, 160, 255, 0.95)
            } else {
                rgba(255, 255, 255, 0.24)
            };
            draw_rounded_fill(ctx, dr, 4.0, col);
            self.hit_rects.push((rect_f(dx - 4.0, bot_y, dw + 8.0, 18.0), HitTarget::HubPageDot(p)));
            dx += dw + 8.0;
        }

        let hint_r = rect_f(px + pw - 270.0, bot_y - 2.0, 240.0, 22.0);
        if let Some(ref fmt) = self.fmt_small {
            draw_text(
                ctx,
                fmt,
                &format!("共 {} 项 · 滚轮翻页 · Esc 收起", self.filtered_indices.len()),
                hint_r,
                rgba(155, 165, 190, 0.72),
            );
        }

        // 4. 关闭应用二次确认弹窗
        if let Some(r_idx) = self.confirm_close {
            if let Some(rw) = self.running.get(r_idx).cloned() {
                if let Ok(mask_brush) = ctx.CreateSolidColorBrush(&rgba(4, 6, 12, 0.62), None) {
                    ctx.FillRectangle(&panel_r, &mask_brush);
                }
                self.hit_rects.push((full_r, HitTarget::ConfirmCloseCancel));

                let dw = 360.0;
                let dh = 168.0;
                let dx = (self.logical_w - dw) * 0.5;
                let dy = (self.logical_h - dh) * 0.5;
                let dlg_r = rect_f(dx, dy, dw, dh);
                draw_glass_panel_gradient(
                    ctx,
                    dlg_r,
                    20.0,
                    rgba(28, 33, 48, 0.97),
                    rgba(18, 21, 32, 0.98),
                    rgba(255, 255, 255, 0.22),
                );

                let t_r = rect_f(dx + 24.0, dy + 22.0, dw - 48.0, 28.0);
                if let Some(ref fmt) = self.fmt_title {
                    draw_text(ctx, fmt, "确认关闭应用", t_r, rgba(242, 245, 255, 0.98));
                }

                let m_r = rect_f(dx + 24.0, dy + 56.0, dw - 48.0, 40.0);
                if let Some(ref fmt) = self.fmt_body {
                    draw_text(
                        ctx,
                        fmt,
                        &format!("确定要关闭 \"{}\" 吗？", rw.exe_name),
                        m_r,
                        rgba(195, 205, 225, 0.90),
                    );
                }

                let cancel_r = rect_f(dx + dw - 210.0, dy + dh - 54.0, 88.0, 36.0);
                let cancel_bg = if self.hovered == HitTarget::ConfirmCloseCancel {
                    rgba(255, 255, 255, 0.16)
                } else {
                    rgba(255, 255, 255, 0.08)
                };
                draw_rounded_fill(ctx, cancel_r, 10.0, cancel_bg);
                if let Some(ref fmt) = self.fmt_center_small {
                    draw_text(ctx, fmt, "取消", cancel_r, rgba(220, 228, 245, 0.92));
                }
                self.hit_rects.push((cancel_r, HitTarget::ConfirmCloseCancel));

                let yes_r = rect_f(dx + dw - 112.0, dy + dh - 54.0, 88.0, 36.0);
                let yes_bg = if self.hovered == HitTarget::ConfirmCloseYes {
                    rgba(235, 75, 90, 0.85)
                } else {
                    rgba(220, 65, 80, 0.65)
                };
                draw_rounded_fill(ctx, yes_r, 10.0, yes_bg);
                if let Some(ref fmt) = self.fmt_center_small {
                    draw_text(ctx, fmt, "确认关闭", yes_r, rgba(255, 245, 245, 0.98));
                }
                self.hit_rects.push((yes_r, HitTarget::ConfirmCloseYes));
            }
        }
    }

    unsafe fn render_settings_panel(
        &mut self,
        ctx: &ID2D1RenderTarget,
        px: f32,
        py: f32,
        pw: f32,
        ph: f32,
    ) {
        let card_x = px + 36.0;
        let card_y = py + 86.0;
        let card_w = pw - 72.0;
        let card_h = ph - 120.0;
        let card_r = rect_f(card_x, card_y, card_w, card_h);
        draw_rounded_fill(ctx, card_r, 20.0, rgba(255, 255, 255, 0.03));
        draw_rounded_stroke(ctx, card_r, 20.0, rgba(255, 255, 255, 0.08), 1.0);

        let mut ry = card_y + 24.0;
        let row_h = 54.0;

        // 辅助绘制分段选项行
        let draw_row = |ui: &mut Self,
                        y: f32,
                        title: &str,
                        desc: &str,
                        opts: &[(&str, bool, HitTarget)]| {
            let lr = rect_f(card_x + 24.0, y + 4.0, 320.0, 24.0);
            let dr = rect_f(card_x + 24.0, y + 26.0, 360.0, 20.0);
            if let Some(ref fmt) = ui.fmt_body {
                draw_text(ctx, fmt, title, lr, rgba(238, 242, 255, 0.96));
            }
            if let Some(ref fmt) = ui.fmt_small {
                draw_text(ctx, fmt, desc, dr, rgba(155, 168, 196, 0.78));
            }
            let mut ox = card_x + card_w - 24.0 - opts.iter().map(|&(_, _, t)| if t == HitTarget::DockHeightInput { 126.0 } else { 96.0 }).sum::<f32>();
            for &(lbl, active, target) in opts {
                let is_input = target == HitTarget::DockHeightInput;
                let bw = if is_input { 118.0 } else { 88.0 };
                let br = rect_f(ox, y + 9.0, bw, 36.0);
                let hov = ui.hovered == target;
                let bg = if is_input {
                    if active {
                        rgba(56, 189, 248, 0.24)
                    } else if hov {
                        rgba(56, 189, 248, 0.14)
                    } else {
                        rgba(12, 18, 28, 0.85)
                    }
                } else if active {
                    rgba(91, 140, 255, 0.34)
                } else if hov {
                    rgba(255, 255, 255, 0.13)
                } else {
                    rgba(255, 255, 255, 0.05)
                };
                draw_rounded_fill(ctx, br, 10.0, bg);
                if is_input {
                    let stroke_col = if active {
                        rgba(56, 189, 248, 0.95)
                    } else {
                        rgba(56, 189, 248, 0.55)
                    };
                    draw_rounded_stroke(ctx, br, 10.0, stroke_col, 1.3);
                } else if active {
                    draw_rounded_stroke(ctx, br, 10.0, rgba(130, 175, 255, 0.65), 1.0);
                }
                if let Some(ref fmt) = ui.fmt_center_small {
                    let col = if is_input {
                        rgba(145, 225, 255, 0.98)
                    } else if active {
                        rgba(248, 250, 255, 0.98)
                    } else {
                        rgba(195, 205, 225, 0.86)
                    };
                    draw_text(ctx, fmt, lbl, br, col);
                }
                ui.hit_rects.push((br, target));
                ox += bw + 8.0;
            }
        };

        let z = self.config.dock_zoom;
        draw_row(
            self,
            ry,
            "Dock 悬停放大倍数",
            "鼠标经过底部 Dock 图标时的波浪放大强度",
            &[
                ("1.25x", (z - 1.25).abs() < 0.05, HitTarget::SetZoom(0)),
                ("1.42x", (z - 1.42).abs() < 0.05, HitTarget::SetZoom(1)),
                ("1.60x", (z - 1.60).abs() < 0.05, HitTarget::SetZoom(2)),
            ],
        );
        ry += row_h + 6.0;

        let dh = self.config.dock_height;
        let dh_input_lbl = if self.height_input_focused {
            format!("✎ {}| px", self.height_input_buf)
        } else {
            format!("✎ 自定义 {}px", dh as u16)
        };
        draw_row(
            self,
            ry,
            "主 Dock 栏高度",
            "点击预设或最右侧青色输入框键入高度 (30~120 px)",
            &[
                ("56 px", (dh - 56.0).abs() < 2.0, HitTarget::SetDockHeight(56)),
                ("64 px", (dh - 64.0).abs() < 2.0, HitTarget::SetDockHeight(64)),
                ("76 px", (dh - 76.0).abs() < 2.0, HitTarget::SetDockHeight(76)),
                (&dh_input_lbl, self.height_input_focused, HitTarget::DockHeightInput),
            ],
        );
        ry += row_h + 6.0;

        let g = self.config.dock_bottom_gap;
        draw_row(
            self,
            ry,
            "Dock 底部避让距离",
            "贴底 (2px) 可避免遮挡上方应用输入框（即时生效）",
            &[
                ("2 px", g <= 6, HitTarget::SetBottomGap(0)),
                ("16 px", g > 6 && g <= 26, HitTarget::SetBottomGap(1)),
                ("36 px", g > 26, HitTarget::SetBottomGap(2)),
            ],
        );
        ry += row_h + 6.0;

        let c = self.config.hub_cols;
        draw_row(
            self,
            ry,
            "Hub 启动台网格列数",
            "调整右侧应用卡片每行显示数量（5 / 6 / 7 列）",
            &[
                ("5 列", c == 5, HitTarget::SetCols(5)),
                ("6 列", c == 6, HitTarget::SetCols(6)),
                ("7 列", c == 7, HitTarget::SetCols(7)),
            ],
        );
        ry += row_h + 6.0;

        let ap = self.config.anim_preset;
        draw_row(
            self,
            ry,
            "物理弹簧动画手感",
            "基于真实时间 dt 计算，兼容 60Hz / 120Hz 显示器",
            &[
                ("柔和", ap == 0, HitTarget::SetAnimPreset(0)),
                ("清脆", ap == 1, HitTarget::SetAnimPreset(1)),
                ("极速", ap == 2, HitTarget::SetAnimPreset(2)),
            ],
        );
        ry += row_h + 6.0;

        let op = self.config.glass_opacity;
        draw_row(
            self,
            ry,
            "曜石玻璃背景不透明度",
            "调节 Dock 与 Hub 面板的半透明深邃质感",
            &[
                ("通透", op < 0.83, HitTarget::SetGlassOpacity(0)),
                ("标准", (0.83..=0.92).contains(&op), HitTarget::SetGlassOpacity(1)),
                ("深邃", op > 0.92, HitTarget::SetGlassOpacity(2)),
            ],
        );
        ry += row_h + 6.0;

        let auto_close = self.config.close_hub_on_launch;
        draw_row(
            self,
            ry,
            "启动应用后自动收起 Hub",
            "在 Hub 中点击应用后自动回到底部 Dock 状态",
            &[(
                if auto_close { "已开启" } else { "已关闭" },
                auto_close,
                HitTarget::ToggleCloseOnLaunch,
            )],
        );
        ry += row_h + 6.0;

        let auto_start = self.auto_start;
        draw_row(
            self,
            ry,
            "开机自动启动",
            "开启后随系统登录自启；关闭时立即清除自启项，无残留文件",
            &[(
                if auto_start { "已开启" } else { "已关闭" },
                auto_start,
                HitTarget::ToggleAutoStart,
            )],
        );
        ry += row_h + 6.0;

        let hidden_cnt = self.config.hidden_dock_apps.len();
        draw_row(
            self,
            ry,
            "已隐藏的 Dock 图标",
            &format!("当前已隐藏 {} 个主 Dock 图标（右键图标可隐藏）", hidden_cnt),
            &[(
                if hidden_cnt > 0 { "恢复全部" } else { "无隐藏" },
                hidden_cnt > 0,
                HitTarget::RestoreHiddenDock,
            )],
        );

        // 底部操作按钮
        let reset_r = rect_f(card_x + 24.0, card_y + card_h - 52.0, 128.0, 36.0);
        let reset_bg = if self.hovered == HitTarget::ResetConfig {
            rgba(235, 95, 105, 0.28)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        draw_rounded_fill(ctx, reset_r, 10.0, reset_bg);
        if let Some(ref fmt) = self.fmt_center_small {
            draw_text(ctx, fmt, "恢复默认设置", reset_r, rgba(235, 195, 200, 0.92));
        }
        self.hit_rects.push((reset_r, HitTarget::ResetConfig));

        let note_r = rect_f(reset_r.right + 16.0, card_y + card_h - 52.0, 420.0, 36.0);
        if let Some(ref fmt) = self.fmt_small {
            draw_text(
                ctx,
                fmt,
                "配置自动保存至 %APPDATA%\\xDock\\config.ini",
                note_r,
                rgba(150, 162, 190, 0.72),
            );
        }
    }
}

unsafe fn draw_notification_badge(
    ctx: &ID2D1RenderTarget,
    fmt: Option<&IDWriteTextFormat>,
    cx: f32,
    cy: f32,
    count: u32,
) {
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let bw = if count >= 10 { 22.0 } else { 18.0 };
    let bh = 18.0;
    let br = rect_f(cx - bw * 0.5, cy - bh * 0.5, bw, bh);
    draw_rounded_fill(ctx, br, 9.0, rgba(242, 68, 78, 0.98));
    draw_rounded_stroke(ctx, br, 9.0, rgba(255, 255, 255, 0.35), 1.0);
    if let Some(f) = fmt {
        draw_text(ctx, f, &label, br, rgba(255, 255, 255, 0.98));
    }
}

unsafe fn draw_rounded_fill(
    ctx: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    color: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
) {
    if let Ok(brush) = ctx.CreateSolidColorBrush(&color, None) {
        let rr = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        ctx.FillRoundedRectangle(&rr, &brush);
    }
}

unsafe fn draw_rounded_stroke(
    ctx: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    color: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    width: f32,
) {
    if let Ok(brush) = ctx.CreateSolidColorBrush(&color, None) {
        let rr = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        ctx.DrawRoundedRectangle(&rr, &brush, width, None);
    }
}

unsafe fn draw_glass_panel(
    ctx: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    fill: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    stroke: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
) {
    draw_rounded_fill(ctx, rect, radius, fill);
    draw_rounded_stroke(ctx, rect, radius, stroke, 1.1);
}

unsafe fn draw_glass_panel_gradient(
    ctx: &ID2D1RenderTarget,
    rect: D2D_RECT_F,
    radius: f32,
    top_col: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    bot_col: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    stroke: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
) {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: top_col,
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: bot_col,
        },
    ];
    if let Ok(stop_col) = ctx.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) {
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: point_f(rect.left, rect.top),
            endPoint: point_f(rect.left, rect.bottom),
        };
        if let Ok(brush) = ctx.CreateLinearGradientBrush(&props, None, &stop_col) {
            let rr = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            ctx.FillRoundedRectangle(&rr, &brush);
        }
    } else {
        draw_rounded_fill(ctx, rect, radius, top_col);
    }
    draw_rounded_stroke(ctx, rect, radius, stroke, 1.2);
}

unsafe fn draw_text(
    ctx: &ID2D1RenderTarget,
    fmt: &IDWriteTextFormat,
    text: &str,
    rect: D2D_RECT_F,
    color: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
) {
    if let Ok(brush) = ctx.CreateSolidColorBrush(&color, None) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        ctx.DrawText(
            &wide,
            fmt,
            &rect,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );
    }
}

fn launch_path(path: &Path) {
    let wide = to_wide(&path.to_string_lossy());
    let verb = to_wide("open");
    unsafe {
        let _ = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}
