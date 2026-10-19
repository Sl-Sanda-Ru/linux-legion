//! Режимы питания (Fn+Q) и лимиты мощности режима «Пользовательский».
//!
//! * режим — `platform-profile` драйвера `lenovo-wmi-gamezone`;
//! * лимиты — firmware attributes драйвера `lenovo-wmi-other`
//!   (`/sys/class/firmware-attributes/lenovo-wmi-other-0/attributes/*`).

use super::sysfs::{self, Result};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerMode {
    Quiet,
    Balanced,
    Performance,
    Extreme,
    Custom,
}

impl PowerMode {
    pub const ALL: [PowerMode; 5] =
        [PowerMode::Quiet, PowerMode::Balanced, PowerMode::Performance, PowerMode::Extreme, PowerMode::Custom];

    pub fn sysfs(self) -> &'static str {
        match self {
            PowerMode::Quiet => "low-power",
            PowerMode::Balanced => "balanced",
            PowerMode::Performance => "performance",
            PowerMode::Extreme => "max-power",
            PowerMode::Custom => "custom",
        }
    }

    pub fn from_sysfs(s: &str) -> Option<Self> {
        Some(match s {
            "low-power" | "quiet" | "cool" => PowerMode::Quiet,
            "balanced" => PowerMode::Balanced,
            "performance" | "balanced-performance" => PowerMode::Performance,
            "max-power" => PowerMode::Extreme,
            "custom" => PowerMode::Custom,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            PowerMode::Quiet => "Quiet",
            PowerMode::Balanced => "Balance",
            PowerMode::Performance => "Performance",
            PowerMode::Extreme => "Extreme",
            PowerMode::Custom => "Custom",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            PowerMode::Quiet => "Quiet and a cool chassis, at the cost of performance",
            PowerMode::Balanced => "Automatic balance between noise and power",
            PowerMode::Performance => "Maximum power for games and work",
            PowerMode::Extreme => "Power beyond stock, fans at their limit",
            PowerMode::Custom => "Your own power limits and fan speeds",
        }
    }

    /// Лимиты CPU (PL1/PL2, Вт), которые Lenovo закладывает в режим — те же,
    /// что Legion Space/DPTF применяют в Windows (capdata01 Legion Pro 7 Gen 10).
    /// Для «Свой» лимиты берутся из атрибутов прошивки (`read_tunables`).
    pub fn cpu_limits(self) -> Option<(u32, u32)> {
        Some(match self {
            PowerMode::Quiet => (55, 65),
            PowerMode::Balanced => (90, 125),
            PowerMode::Performance => (145, 190),
            PowerMode::Extreme => (160, 205),
            PowerMode::Custom => return None,
        })
    }

    /// EPP (energy_performance_preference) под режим.
    pub fn epp(self) -> &'static str {
        match self {
            PowerMode::Quiet => "balance_power",
            PowerMode::Balanced => "balance_performance",
            PowerMode::Performance | PowerMode::Extreme | PowerMode::Custom => "performance",
        }
    }

    /// Ключ в custom.conf.
    pub fn key(self) -> &'static str {
        match self {
            PowerMode::Quiet => "quiet",
            PowerMode::Balanced => "balanced",
            PowerMode::Performance => "performance",
            PowerMode::Extreme => "extreme",
            PowerMode::Custom => "custom",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        PowerMode::ALL.iter().copied().find(|m| m.key() == s)
    }

    /// Цвет индикатора на кнопке питания, как в Legion.
    pub fn tone(self) -> &'static str {
        match self {
            PowerMode::Quiet => "quiet",
            PowerMode::Balanced => "balanced",
            PowerMode::Performance => "perf",
            PowerMode::Extreme => "extreme",
            PowerMode::Custom => "custom",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct PowerState {
    pub mode: Option<PowerMode>,
    pub choices: Vec<PowerMode>,
    pub writable: bool,
}

fn profile_dir() -> Option<PathBuf> {
    let by_name = |want: Option<&str>| {
        sysfs::find_dir("/sys/class/platform-profile", |p| {
            let name = sysfs::read(&p.join("name")).unwrap_or_default();
            want.is_none_or(|w| name == w)
        })
    };
    by_name(Some("lenovo-wmi-gamezone")).or_else(|| by_name(None))
}

/// Файлы профиля: (текущий, варианты).
fn profile_files() -> Option<(PathBuf, PathBuf)> {
    if let Some(d) = profile_dir() {
        return Some((d.join("profile"), d.join("choices")));
    }
    let legacy = sysfs::sys("/sys/firmware/acpi/platform_profile");
    legacy.exists().then(|| (legacy.clone(), legacy.with_file_name("platform_profile_choices")))
}

pub fn read_state() -> PowerState {
    let Some((cur, choices)) = profile_files() else { return PowerState::default() };
    let mode = sysfs::read(&cur).and_then(|s| PowerMode::from_sysfs(&s));
    let mut list: Vec<PowerMode> = sysfs::read(&choices)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(PowerMode::from_sysfs)
        .collect();
    list.sort_by_key(|m| PowerMode::ALL.iter().position(|x| x == m));
    list.dedup();
    PowerState { mode, choices: list, writable: sysfs::writable(&cur) }
}

pub fn set_mode(mode: PowerMode) -> Result<()> {
    let (cur, _) = profile_files().ok_or_else(|| sysfs::Error::NotFound("platform_profile".into()))?;
    sysfs::write(&cur, mode.sysfs())
}

// ---------------------------------------------------------------------------
// Лимиты мощности

#[derive(Clone, Debug, PartialEq)]
pub struct Tunable {
    pub name: String,
    pub label: String,
    pub hint: String,
    pub unit: &'static str,
    pub value: i64,
    pub min: i64,
    pub max: i64,
    pub step: i64,
    pub default: Option<i64>,
    pub writable: bool,
}

fn attrs_dir() -> Option<PathBuf> {
    sysfs::find_dir("/sys/class/firmware-attributes", |p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("lenovo-wmi-other"))
    })
    .map(|d| d.join("attributes"))
}

/// Порядок атрибутов на странице: сначала CPU, потом GPU — как в
/// Lenovo Legion Toolkit. Неизвестные уходят в конец по алфавиту.
const ORDER: [&str; 12] = [
    "ppt_pl1_spl",
    "ppt_pl2_sppt",
    "ppt_pl3_fppt",
    "ppt_cpu_cl",
    "ppt_pl1_tau",
    "ppt_pl1_apu_spl",
    "cpu_temp",
    "gpu_nv_ctgp",
    "gpu_nv_ppab",
    "gpu_nv_cpu_boost",
    "gpu_nv_ac_offset",
    "gpu_temp",
];

/// Подписи известных атрибутов: (название, пояснение, единица). Названия и
/// пояснения взяты из Lenovo Legion Toolkit (окно Custom Mode), чтобы на
/// обеих системах они читались одинаково; имена атрибутов и единицы сверены
/// с `display_name` драйвера `lenovo-wmi-other` на Legion Pro 7 16IAX10H.
fn describe(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match name {
        "ppt_pl1_spl" => (
            "Long Term Power Limit",
            "The maximum power the CPU is allowed to sustain indefinitely under long workloads.",
            "W",
        ),
        "ppt_pl2_sppt" => (
            "Short Term Power Limit",
            "The maximum power the CPU is allowed to draw for brief bursts of high activity.",
            "W",
        ),
        "ppt_pl3_fppt" => (
            "Peak Power Limit",
            "The absolute limit for instantaneous power spikes to protect the voltage regulator.",
            "W",
        ),
        "ppt_cpu_cl" => (
            "Long Term Power Limit (Cross Loading)",
            "The sustained power limit applied to the CPU when the GPU is also under heavy load.",
            "W",
        ),
        "ppt_pl1_tau" => (
            "Short Term Power Limit Duration",
            "How long the CPU may stay above the long term limit before dropping back to it.",
            "s",
        ),
        "ppt_pl1_apu_spl" => (
            "APU sPPT Power Limit",
            "The slow package power tracking limit of an AMD APU package.",
            "W",
        ),
        "cpu_temp" => (
            "CPU Temperature Limit",
            "The target temperature threshold at which the CPU will begin thermal throttling.",
            "°C",
        ),
        "gpu_nv_ctgp" => (
            "Configurable TGP",
            "The base sustained power limit allocated to the GPU, excluding dynamic boost headroom.",
            "W",
        ),
        "gpu_nv_ppab" => (
            "Dynamic Boost",
            "The extra power handed to the GPU from the CPU's budget during high graphics demand.",
            "W",
        ),
        "gpu_nv_cpu_boost" => (
            "GPU to CPU Dynamic Boost",
            "The extra power the CPU may borrow from the GPU's budget.",
            "W",
        ),
        "gpu_nv_ac_offset" => (
            "Total Processing Power Target on AC",
            "The CPU power threshold that triggers dynamic power adjustments for the GPU on AC power.",
            "W",
        ),
        "gpu_temp" => (
            "GPU Temperature Limit",
            "The target temperature threshold at which the GPU will begin thermal throttling.",
            "°C",
        ),
        _ => return None,
    })
}

pub fn read_tunables() -> Vec<Tunable> {
    let Some(dir) = attrs_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Tunable> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if sysfs::read(&p.join("type")).as_deref() != Some("integer") {
                return None;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let num = |f: &str| sysfs::read_num::<i64>(&p.join(f));
            let (label, hint, unit) = match describe(&name) {
                Some((l, h, u)) => (l.to_string(), h.to_string(), u),
                None => (sysfs::read(&p.join("display_name")).unwrap_or_else(|| name.clone()), String::new(), ""),
            };
            Some(Tunable {
                value: num("current_value")?,
                min: num("min_value")?,
                max: num("max_value")?,
                step: num("scalar_increment").unwrap_or(1).max(1),
                default: num("default_value"),
                writable: sysfs::writable(&p.join("current_value")),
                name,
                label,
                hint,
                unit,
            })
        })
        .collect();
    let rank = |n: &str| ORDER.iter().position(|x| *x == n).unwrap_or(ORDER.len());
    out.sort_by(|a, b| rank(&a.name).cmp(&rank(&b.name)).then_with(|| a.name.cmp(&b.name)));
    out
}

pub fn set_tunable(name: &str, value: i64) -> Result<()> {
    let dir = attrs_dir().ok_or_else(|| sysfs::Error::NotFound("firmware-attributes".into()))?;
    sysfs::write(&dir.join(name).join("current_value"), &value.to_string())
}
