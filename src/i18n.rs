use serde::{Deserialize, Serialize};

/// Supported languages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Language {
    #[default]
    En, // English
    Zh, // Chinese (Simplified)
}

impl Language {
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "zh" | "chinese" | "中文" => Language::Zh,
            _ => Language::En,
        }
    }
}

/// Text resources for internationalization
#[derive(Debug, Clone)]
pub struct I18n {
    pub language: Language,
}

impl I18n {
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    // Application Title
    pub fn app_title(&self) -> &'static str {
        match self.language {
            Language::En => "⚡ Claude Zephyr - Auto Endpoint Switching",
            Language::Zh => "⚡ Claude Zephyr - 自动端点切换",
        }
    }

    pub fn proxy_label(&self) -> &'static str {
        match self.language {
            Language::En => "🔗 Proxy:",
            Language::Zh => "🔗 代理:",
        }
    }

    // Status indicators
    pub fn status_monitoring(&self) -> &'static str {
        match self.language {
            Language::En => "🟢 Monitoring",
            Language::Zh => "🟢 正在监控",
        }
    }

    pub fn status_paused(&self) -> &'static str {
        match self.language {
            Language::En => "⏸️  Health checks paused",
            Language::Zh => "⏸️  健康检查已暂停",
        }
    }

    // Button labels
    pub fn btn_quit(&self) -> &'static str {
        match self.language {
            Language::En => "[Q] Quit",
            Language::Zh => "[Q] 退出",
        }
    }

    pub fn btn_manual_check(&self) -> &'static str {
        match self.language {
            Language::En => "[R] Manual Check",
            Language::Zh => "[R] 手动检查",
        }
    }

    pub fn btn_pause(&self) -> &'static str {
        match self.language {
            Language::En => "[P] Pause",
            Language::Zh => "[P] 暂停",
        }
    }

    pub fn btn_resume(&self) -> &'static str {
        match self.language {
            Language::En => "[P] Resume",
            Language::Zh => "[P] 恢复",
        }
    }

    pub fn btn_to_manual(&self) -> &'static str {
        match self.language {
            Language::En => "[M] Manual Mode",
            Language::Zh => "[M] 手动模式",
        }
    }

    pub fn btn_to_auto(&self) -> &'static str {
        match self.language {
            Language::En => "[M] Auto Mode",
            Language::Zh => "[M] 自动模式",
        }
    }

    pub fn btn_browse_endpoints(&self) -> &'static str {
        match self.language {
            Language::En => " │ [↑↓] Browse Endpoints",
            Language::Zh => " │ [↑↓] 浏览端点",
        }
    }

    pub fn btn_select_confirm(&self) -> &'static str {
        match self.language {
            Language::En => " │ [↑↓] Select [Enter] Confirm",
            Language::Zh => " │ [↑↓] 选择 [Enter] 确认",
        }
    }

    // Mode indicators
    pub fn mode_auto(&self) -> &'static str {
        match self.language {
            Language::En => "🤖Auto",
            Language::Zh => "🤖自动",
        }
    }

    pub fn mode_manual(&self) -> &'static str {
        match self.language {
            Language::En => "🎯Manual",
            Language::Zh => "🎯手动",
        }
    }

    pub fn mode_manual_indexed(&self, index: usize) -> String {
        match self.language {
            Language::En => format!("🎯Manual[{}]", index + 1),
            Language::Zh => format!("🎯手动[{}]", index + 1),
        }
    }

    // Status text
    pub fn status_checking(&self) -> &'static str {
        match self.language {
            Language::En => "⟳ Checking...",
            Language::Zh => "⟳ 检查中...",
        }
    }

    pub fn status_available(&self) -> &'static str {
        match self.language {
            Language::En => "✓",
            Language::Zh => "✓",
        }
    }

    pub fn status_error(&self) -> &'static str {
        match self.language {
            Language::En => "✗",
            Language::Zh => "✗",
        }
    }

    pub fn error_timeout(&self) -> &'static str {
        match self.language {
            Language::En => "Timeout",
            Language::Zh => "超时",
        }
    }

    pub fn error_generic(&self) -> &'static str {
        match self.language {
            Language::En => "Error",
            Language::Zh => "错误",
        }
    }

    // Health check status
    pub fn health_checking_with_time(&self, seconds: u64) -> String {
        match self.language {
            Language::En => format!("Checking... ({seconds}s left)"),
            Language::Zh => format!("检查中... ({seconds}s剩余)"),
        }
    }

    pub fn health_ready(&self) -> &'static str {
        match self.language {
            Language::En => "Ready",
            Language::Zh => "就绪",
        }
    }

    pub fn health_next(&self, seconds: u64) -> String {
        match self.language {
            Language::En => format!("Next: {seconds}s"),
            Language::Zh => format!("下次: {seconds}s"),
        }
    }

    // Load levels
    pub fn load_high(&self, count: u32) -> String {
        match self.language {
            Language::En => format!("High Load: {count}"),
            Language::Zh => format!("高负载: {count}"),
        }
    }

    pub fn load_medium(&self, count: u32) -> String {
        match self.language {
            Language::En => format!("Med Load: {count}"),
            Language::Zh => format!("中负载: {count}"),
        }
    }

    pub fn load_low(&self, count: u32) -> String {
        match self.language {
            Language::En => format!("Low Load: {count}"),
            Language::Zh => format!("低负载: {count}"),
        }
    }

    pub fn load_idle(&self) -> &'static str {
        match self.language {
            Language::En => "Idle",
            Language::Zh => "空闲",
        }
    }

    // Switch info
    pub fn switch_new_connection(&self) -> &'static str {
        match self.language {
            Language::En => "New Connection",
            Language::Zh => "新连接",
        }
    }

    // Paused subtitle
    pub fn paused_subtitle(&self) -> &'static str {
        match self.language {
            Language::En => {
                "⏸️  Health checks paused - Connection monitoring continues, auto switching stopped"
            }
            Language::Zh => "⏸️  健康检查已暂停 - 连接监控继续运行，自动切换已停止",
        }
    }
}
