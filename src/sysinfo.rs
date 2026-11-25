use crate::gpu::to_wide;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Media::Audio::{waveOutGetVolume, waveOutSetVolume, HWAVEOUT};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SW_SHOWNORMAL,
};

#[derive(Clone, Debug)]
pub struct SystemStatus {
    pub time_str: String,
    pub date_str: String,
    pub ime_cn: bool,
    pub volume_pct: u8,
    pub saved_vol_raw: u32,
}

impl Default for SystemStatus {
    fn default() -> Self {
        Self {
            time_str: "00:00".to_string(),
            date_str: "01-01".to_string(),
            ime_cn: true,
            volume_pct: 50,
            saved_vol_raw: 0x8000_8000,
        }
    }
}

impl SystemStatus {
    pub fn refresh(&mut self, self_hwnd: HWND) {
        unsafe {
            let st = GetLocalTime();
            self.time_str = format!("{:02}:{:02}", st.wHour, st.wMinute);
            self.date_str = format!("{:02}-{:02}", st.wMonth, st.wDay);

            // 检查前台非 xDock 窗口的键盘布局（判断中/英）
            let fg = GetForegroundWindow();
            if !fg.0.is_null() && fg != self_hwnd {
                let tid = GetWindowThreadProcessId(fg, None);
                let hkl = GetKeyboardLayout(tid);
                let lang_id = (hkl.0 as usize) & 0xFFFF;
                self.ime_cn = lang_id == 0x0804 || lang_id == 0x0404;
            }

            let mut vol = 0u32;
            if waveOutGetVolume(HWAVEOUT(std::ptr::null_mut()), &mut vol) == 0 {
                let left = (vol & 0xFFFF) as u32;
                self.volume_pct = ((left * 100 + 32767) / 65535).min(100) as u8;
                if vol != 0 {
                    self.saved_vol_raw = vol;
                }
            }
        }
    }

    pub fn toggle_mute(&mut self) {
        unsafe {
            if self.volume_pct == 0 {
                let target = if self.saved_vol_raw == 0 {
                    0x8000_8000
                } else {
                    self.saved_vol_raw
                };
                let _ = waveOutSetVolume(HWAVEOUT(std::ptr::null_mut()), target);
                let left = (target & 0xFFFF) as u32;
                self.volume_pct = ((left * 100 + 32767) / 65535).min(100) as u8;
            } else {
                let mut cur = 0u32;
                if waveOutGetVolume(HWAVEOUT(std::ptr::null_mut()), &mut cur) == 0 && cur != 0 {
                    self.saved_vol_raw = cur;
                }
                let _ = waveOutSetVolume(HWAVEOUT(std::ptr::null_mut()), 0);
                self.volume_pct = 0;
            }
        }
    }

    pub fn adjust_volume(&mut self, delta_steps: i32) {
        let next = (self.volume_pct as i32 + delta_steps * 5).clamp(0, 100) as u8;
        let ch = ((next as u32 * 65535) / 100) & 0xFFFF;
        let raw = (ch << 16) | ch;
        unsafe {
            let _ = waveOutSetVolume(HWAVEOUT(std::ptr::null_mut()), raw);
        }
        self.volume_pct = next;
        if raw != 0 {
            self.saved_vol_raw = raw;
        }
    }
}

pub fn open_ms_settings(uri: &str) {
    let w_uri = to_wide(uri);
    let w_open = to_wide("open");
    unsafe {
        let _ = ShellExecuteW(
            None,
            PCWSTR(w_open.as_ptr()),
            PCWSTR(w_uri.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}
