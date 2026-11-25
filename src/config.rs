use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub dock_zoom: f32,          // 1.25, 1.42, 1.60
    pub dock_height: f32,        // 56.0, 68.0, 82.0, 96.0
    pub dock_bottom_gap: i32,    // 12, 36, 64
    pub hub_cols: usize,         // 5, 6, 7
    pub anim_preset: u8,         // 0=柔和, 1=清脆, 2=极速
    pub glass_opacity: f32,      // 0.78, 0.88, 0.95
    pub close_hub_on_launch: bool,
    pub pinned_apps: Vec<String>,
    pub hidden_dock_apps: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            dock_zoom: 1.42,
            dock_height: 58.0,
            dock_bottom_gap: 2,
            hub_cols: 6,
            anim_preset: 1,
            glass_opacity: 0.90,
            close_hub_on_launch: true,
            pinned_apps: Vec::new(),
            hidden_dock_apps: Vec::new(),
        }
    }
}

impl AppConfig {
    fn config_path() -> Option<PathBuf> {
        std::env::var("APPDATA")
            .ok()
            .map(|ad| PathBuf::from(ad).join("xDock").join("config.ini"))
    }

    pub fn load() -> Self {
        let mut cfg = Self::default();
        let Some(path) = Self::config_path() else {
            return cfg;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            return cfg;
        };
        for line in content.lines() {
            let line = line.trim();
            if let Some((k, v)) = line.split_once('=') {
                match k.trim() {
                    "dock_zoom" => {
                        if let Ok(val) = v.trim().parse::<f32>() {
                            cfg.dock_zoom = val.clamp(1.15, 1.75);
                        }
                    }
                    "dock_height" => {
                        if let Ok(val) = v.trim().parse::<f32>() {
                            cfg.dock_height = val.clamp(30.0, 120.0);
                        }
                    }
                    "dock_bottom_gap" => {
                        if let Ok(val) = v.trim().parse::<i32>() {
                            cfg.dock_bottom_gap = val.clamp(0, 120);
                        }
                    }
                    "hub_cols" => {
                        if let Ok(val) = v.trim().parse::<usize>() {
                            cfg.hub_cols = val.clamp(4, 8);
                        }
                    }
                    "anim_preset" => {
                        if let Ok(val) = v.trim().parse::<u8>() {
                            cfg.anim_preset = val.min(2);
                        }
                    }
                    "glass_opacity" => {
                        if let Ok(val) = v.trim().parse::<f32>() {
                            cfg.glass_opacity = val.clamp(0.65, 0.98);
                        }
                    }
                    "close_hub_on_launch" => {
                        cfg.close_hub_on_launch = v.trim() == "1" || v.trim() == "true";
                    }
                    "pinned_apps" => {
                        cfg.pinned_apps = v
                            .split('|')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "hidden_dock_apps" => {
                        cfg.hidden_dock_apps = v
                            .split('|')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    _ => {}
                }
            }
        }
        cfg
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let pinned = self.pinned_apps.join("|");
        let hidden = self.hidden_dock_apps.join("|");
        let body = format!(
            "dock_zoom={:.2}\ndock_height={:.0}\ndock_bottom_gap={}\nhub_cols={}\nanim_preset={}\nglass_opacity={:.2}\nclose_hub_on_launch={}\npinned_apps={}\nhidden_dock_apps={}\n",
            self.dock_zoom,
            self.dock_height,
            self.dock_bottom_gap,
            self.hub_cols,
            self.anim_preset,
            self.glass_opacity,
            if self.close_hub_on_launch { 1 } else { 0 },
            pinned,
            hidden
        );
        let _ = std::fs::write(path, body);
    }

    pub fn spring_params(&self) -> (f32, f32) {
        match self.anim_preset {
            0 => (260.0, 24.0), // 柔和
            2 => (480.0, 30.0), // 极速
            _ => (360.0, 26.0), // 清脆
        }
    }
}
