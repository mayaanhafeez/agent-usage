use std::{fmt, str::FromStr};

use chrono::{DateTime, Local};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    Claude,
    Codex,
    OpenCode,
    Gemini,
}

impl Provider {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::OpenCode, Self::Gemini];

    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Gemini => "Gemini CLI",
        }
    }

    pub fn process_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude"],
            Self::Codex => &["codex"],
            Self::OpenCode => &["opencode"],
            Self::Gemini => &["gemini"],
        }
    }

    pub fn mark(self) -> &'static str {
        match self {
            Self::Claude => "*",
            Self::Codex => ">_",
            Self::OpenCode => "OC",
            Self::Gemini => "+",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "opencode" | "open-code" => Ok(Self::OpenCode),
            "gemini" | "gemini-cli" => Ok(Self::Gemini),
            _ => Err(format!("unsupported agent: {value}")),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Limit {
    pub label: String,
    pub percent: u16,
    pub resets: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct UsageRow {
    pub label: String,
    pub tokens: u64,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub provider: Provider,
    pub plan: Option<String>,
    pub limits: Vec<Limit>,
    pub days: Vec<UsageRow>,
    pub models: Vec<UsageRow>,
    pub note: Option<String>,
    pub updated_at: DateTime<Local>,
}

impl Snapshot {
    pub fn empty(provider: Provider) -> Self {
        Self {
            provider,
            plan: None,
            limits: Vec::new(),
            days: Vec::new(),
            models: Vec::new(),
            note: None,
            updated_at: Local::now(),
        }
    }
}
