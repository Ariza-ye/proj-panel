//! Lightweight runtime localization: a process-wide language switch plus inline `tr!` macros.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// Languages supported by the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh")]
    Zh,
    #[serde(rename = "en")]
    En,
}

impl Language {
    /// All selectable languages in display order.
    pub const ALL: [Language; 2] = [Language::Zh, Language::En];

    /// Name of the language written in the language itself.
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::Zh => "中文",
            Self::En => "English",
        }
    }

    /// Position of the language inside [`Language::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    /// Resolves a language from its position inside [`Language::ALL`].
    pub fn from_index(idx: usize) -> Self {
        Self::ALL.get(idx).copied().unwrap_or_default()
    }

    const fn to_u8(self) -> u8 {
        match self {
            Self::Zh => 0,
            Self::En => 1,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Returns the language currently used by the interface.
pub fn current() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::En,
        _ => Language::Zh,
    }
}

/// Switches the language used by the interface.
pub fn set_current(lang: Language) {
    CURRENT.store(lang.to_u8(), Ordering::Relaxed);
}

/// Returns whether the interface currently uses English.
pub fn is_en() -> bool {
    current() == Language::En
}

/// Selects between a Chinese and an English static string.
macro_rules! tr {
    ($zh:expr, $en:expr) => {
        if $crate::i18n::is_en() { $en } else { $zh }
    };
}

/// Selects between Chinese and English format strings sharing the same arguments.
macro_rules! tr_fmt {
    ($zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_en() {
            format!($en $(, $arg)*)
        } else {
            format!($zh $(, $arg)*)
        }
    };
}

/// Localized abbreviated month name for a 1-based month number.
pub fn month_label(month: u32) -> String {
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if is_en() {
        EN.get(month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("")
            .to_string()
    } else {
        format!("{month}月")
    }
}
