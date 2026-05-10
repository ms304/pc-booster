use sysinfo::System;

pub struct SystemInfo {
    sys: System,
}

impl SystemInfo {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self { sys }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_memory();
    }

    pub fn total_memory(&self) -> u64 {
        self.sys.total_memory()
    }

    pub fn used_memory(&self) -> u64 {
        self.sys.used_memory()
    }

    pub fn available_memory(&self) -> u64 {
        self.sys.available_memory()
    }

    pub fn free_memory(&self) -> u64 {
        self.sys.free_memory()
    }

    pub fn memory_usage_percent(&self) -> f32 {
        let total = self.total_memory();
        if total == 0 {
            0.0
        } else {
            (self.used_memory() as f32 / total as f32) * 100.0
        }
    }

    pub fn format_memory(bytes: u64) -> String {
        const GB: u64 = 1024 * 1024 * 1024;
        const MB: u64 = 1024 * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else {
            format!("{} KB", bytes / 1024)
        }
    }
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self::new()
    }
}
