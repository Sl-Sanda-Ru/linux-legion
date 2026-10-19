//! EPP — energy_performance_preference драйвера intel_pstate/amd-pstate:
//! насколько охотно процессор поднимает частоту. Значения:
//! `performance`, `balance_performance`, `balance_power`, `power`.
//! Governor при этом остаётся `powersave` (у intel_pstate это штатный
//! governor с HWP; `performance` просто прибивает EPP к performance).

use super::sysfs::{self, Result};
use std::path::PathBuf;

fn policies() -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(sysfs::sys("/sys/devices/system/cpu/cpufreq")) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("policy")))
        .collect();
    v.sort();
    v
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Epp {
    pub current: Option<String>,
    pub choices: Vec<String>,
    /// scaling_governor; при `performance` EPP прибит к performance и не
    /// пишется (EBUSY) — так делает, например, tuned-ppd в режиме Производительность.
    pub governor: Option<String>,
    pub writable: bool,
}

impl Epp {
    pub fn pinned(&self) -> bool {
        self.governor.as_deref() == Some("performance")
    }
}

pub fn read() -> Epp {
    let Some(p0) = policies().into_iter().next() else { return Epp::default() };
    let f = p0.join("energy_performance_preference");
    Epp {
        current: sysfs::read(&f),
        choices: sysfs::read(&p0.join("energy_performance_available_preferences"))
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        governor: sysfs::read(&p0.join("scaling_governor")),
        writable: sysfs::writable(&f),
    }
}

/// Записать EPP во все policy. Возвращает true, если что-то поменялось.
/// Policy с governor `performance` пропускаются — там EPP и так performance.
pub fn set(pref: &str) -> Result<bool> {
    let mut changed = false;
    for p in policies() {
        if sysfs::read(&p.join("scaling_governor")).as_deref() == Some("performance") {
            continue;
        }
        let f = p.join("energy_performance_preference");
        if sysfs::read(&f).as_deref() == Some(pref) {
            continue;
        }
        sysfs::write(&f, pref)?;
        changed = true;
    }
    Ok(changed)
}

/// Турбо-частоты (Intel Turbo Boost, у AMD — Core Performance Boost): может ли
/// процессор подниматься выше базовой частоты.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boost {
    pub on: bool,
    pub writable: bool,
}

/// Файл переключателя: у intel_pstate — `no_turbo` (1 — турбо выключено),
/// у остальных драйверов — общий `cpufreq/boost` (1 — включено).
fn boost_file() -> Option<(PathBuf, bool)> {
    let intel = sysfs::sys("/sys/devices/system/cpu/intel_pstate/no_turbo");
    if intel.exists() {
        return Some((intel, true));
    }
    let generic = sysfs::sys("/sys/devices/system/cpu/cpufreq/boost");
    generic.exists().then_some((generic, false))
}

pub fn boost() -> Option<Boost> {
    let (f, inverted) = boost_file()?;
    let v = sysfs::read_bool(&f)?;
    Some(Boost { on: v != inverted, writable: sysfs::writable(&f) })
}

/// Включить/выключить турбо. Возвращает true, если что-то поменялось.
pub fn set_boost(on: bool) -> Result<bool> {
    let (f, inverted) = boost_file().ok_or_else(|| sysfs::Error::NotFound("cpu boost".into()))?;
    if sysfs::read_bool(&f).map(|v| v != inverted) == Some(on) {
        return Ok(false);
    }
    sysfs::write(&f, if on != inverted { "1" } else { "0" })?;
    Ok(true)
}
