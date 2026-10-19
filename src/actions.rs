//! Действия — шаги автоматизации Legion Toolkit, которые имеют смысл в Linux.
//! Одни и те же действия выполняют правила службы, быстрые действия в окне и
//! командная строка (`linux_legion set …`, `linux_legion run …`).
//!
//! Текстовая форма — «вид значение», она же хранится в automation.conf:
//!
//! ```text
//! power-mode performance      battery-mode conservation   preset Gaming
//! cycle-preset                fn-lock on                  camera off
//! always-on-usb on            fan-max on                  cpu-boost off
//! spectrum-profile 3
//! spectrum-brightness 5       panel-logo off              spectrum-import ~/wave.json
//! notify Text                 run command --args          delay 5
//! ```

use crate::autoapply::{self, Flag};
use crate::hw::battery::{self, ChargeType};
use crate::hw::cpufreq;
use crate::hw::device::{self, Toggle};
use crate::hw::power::{self, PowerMode};
use crate::notify;
use crate::presets;
use crate::spectrum::device::Keyboard;
use crate::spectrum::profile_file;
use crate::spectrum::protocol::{self as proto};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    PowerMode(PowerMode),
    BatteryMode(ChargeType),
    Preset(String),
    CyclePreset,
    Toggle(Toggle, bool),
    FanMax(bool),
    CpuBoost(bool),
    SpectrumProfile(u8),
    SpectrumBrightness(u8),
    PanelLogo(bool),
    /// Загрузить файл профиля (формат Legion Toolkit) в текущий профиль.
    SpectrumImport(String),
    Notify(String),
    Run(String),
    Delay(u32),
}

/// Вид действия — для редактора: подпись и какие значения допустимы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    PowerMode,
    BatteryMode,
    Preset,
    CyclePreset,
    FnLock,
    Camera,
    AlwaysOnUsb,
    FanMax,
    CpuBoost,
    SpectrumProfile,
    SpectrumBrightness,
    PanelLogo,
    SpectrumImport,
    Notify,
    Run,
    Delay,
}

/// Чем задаётся значение действия в редакторе.
pub enum Value {
    None,
    /// Выбор из списка: (значение в конфиге, подпись).
    Choice(Vec<(String, String)>),
    Text(&'static str),
}

const ON_OFF: [(&str, &str); 2] = [("on", "On"), ("off", "Off")];

impl Kind {
    pub const ALL: [Kind; 16] = [
        Kind::PowerMode,
        Kind::BatteryMode,
        Kind::Preset,
        Kind::CyclePreset,
        Kind::FanMax,
        Kind::CpuBoost,
        Kind::FnLock,
        Kind::Camera,
        Kind::AlwaysOnUsb,
        Kind::SpectrumProfile,
        Kind::SpectrumBrightness,
        Kind::PanelLogo,
        Kind::SpectrumImport,
        Kind::Notify,
        Kind::Run,
        Kind::Delay,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Kind::PowerMode => "power-mode",
            Kind::BatteryMode => "battery-mode",
            Kind::Preset => "preset",
            Kind::CyclePreset => "cycle-preset",
            Kind::FnLock => "fn-lock",
            Kind::Camera => "camera",
            Kind::AlwaysOnUsb => "always-on-usb",
            Kind::FanMax => "fan-max",
            Kind::CpuBoost => "cpu-boost",
            Kind::SpectrumProfile => "spectrum-profile",
            Kind::SpectrumBrightness => "spectrum-brightness",
            Kind::PanelLogo => "panel-logo",
            Kind::SpectrumImport => "spectrum-import",
            Kind::Notify => "notify",
            Kind::Run => "run",
            Kind::Delay => "delay",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::PowerMode => "Power Mode",
            Kind::BatteryMode => "Battery Mode",
            Kind::Preset => "Custom Mode preset",
            Kind::CyclePreset => "Cycle Custom Mode presets",
            Kind::FnLock => "Fn Lock",
            Kind::Camera => "Camera",
            Kind::AlwaysOnUsb => "Always on USB",
            Kind::FanMax => "Maximum fan speed",
            Kind::CpuBoost => "CPU Boost",
            Kind::SpectrumProfile => "Keyboard backlight profile",
            Kind::SpectrumBrightness => "Keyboard backlight brightness",
            Kind::PanelLogo => "Panel logo backlight",
            Kind::SpectrumImport => "Import keyboard backlight profile",
            Kind::Notify => "Notification",
            Kind::Run => "Run",
            Kind::Delay => "Delay",
        }
    }

    pub fn from_key(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == s)
    }

    pub fn value(self) -> Value {
        let choice = |v: Vec<(String, String)>| Value::Choice(v);
        let on_off = || choice(ON_OFF.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect());
        match self {
            Kind::PowerMode => choice(PowerMode::ALL.iter().map(|m| (m.key().into(), m.label().into())).collect()),
            Kind::BatteryMode => choice(ChargeType::ALL.iter().map(|t| (charge_key(*t).into(), t.label().into())).collect()),
            Kind::Preset => choice(presets::load().list.into_iter().map(|p| (p.name.clone(), p.name)).collect()),
            Kind::FnLock | Kind::Camera | Kind::AlwaysOnUsb | Kind::FanMax | Kind::CpuBoost | Kind::PanelLogo => on_off(),
            Kind::SpectrumProfile => choice(proto::PROFILES.map(|n| (n.to_string(), format!("Profile {n}"))).collect()),
            Kind::SpectrumBrightness => choice(
                (0..=proto::MAX_BRIGHTNESS).map(|n| (n.to_string(), if n == 0 { "Off".into() } else { n.to_string() })).collect(),
            ),
            Kind::SpectrumImport => Value::Text("/path/to/profile.json"),
            Kind::Notify => Value::Text("Message"),
            Kind::Run => Value::Text("command and arguments"),
            Kind::Delay => Value::Text("seconds"),
            Kind::CyclePreset => Value::None,
        }
    }
}

fn charge_key(t: ChargeType) -> &'static str {
    match t {
        ChargeType::Standard => "normal",
        ChargeType::Fast => "rapid-charge",
        ChargeType::LongLife => "conservation",
    }
}

fn on_off(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn on_off_str(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// `~/файл` → путь в домашнем каталоге.
pub fn expand_home(p: &str) -> PathBuf {
    let p = p.trim();
    match p.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest),
        None => PathBuf::from(p),
    }
}

impl Action {
    pub fn kind(&self) -> Kind {
        match self {
            Action::PowerMode(_) => Kind::PowerMode,
            Action::BatteryMode(_) => Kind::BatteryMode,
            Action::Preset(_) => Kind::Preset,
            Action::CyclePreset => Kind::CyclePreset,
            Action::Toggle(Toggle::FnLock, _) => Kind::FnLock,
            Action::Toggle(Toggle::Camera, _) => Kind::Camera,
            Action::Toggle(Toggle::UsbCharging, _) => Kind::AlwaysOnUsb,
            Action::FanMax(_) => Kind::FanMax,
            Action::CpuBoost(_) => Kind::CpuBoost,
            Action::SpectrumProfile(_) => Kind::SpectrumProfile,
            Action::SpectrumBrightness(_) => Kind::SpectrumBrightness,
            Action::PanelLogo(_) => Kind::PanelLogo,
            Action::SpectrumImport(_) => Kind::SpectrumImport,
            Action::Notify(_) => Kind::Notify,
            Action::Run(_) => Kind::Run,
            Action::Delay(_) => Kind::Delay,
        }
    }

    /// Значение в текстовой форме (вторая часть строки конфига).
    pub fn value(&self) -> String {
        match self {
            Action::PowerMode(m) => m.key().into(),
            Action::BatteryMode(t) => charge_key(*t).into(),
            Action::Preset(n) | Action::SpectrumImport(n) | Action::Notify(n) | Action::Run(n) => n.clone(),
            Action::CyclePreset => String::new(),
            Action::Toggle(_, b) | Action::FanMax(b) | Action::CpuBoost(b) | Action::PanelLogo(b) => {
                on_off_str(*b).into()
            }
            Action::SpectrumProfile(n) | Action::SpectrumBrightness(n) => n.to_string(),
            Action::Delay(s) => s.to_string(),
        }
    }

    pub fn to_line(&self) -> String {
        let v = self.value();
        if v.is_empty() {
            self.kind().key().into()
        } else {
            format!("{} {v}", self.kind().key())
        }
    }

    pub fn parse(line: &str) -> Result<Action, String> {
        let line = line.trim();
        let (k, v) = line.split_once(char::is_whitespace).map_or((line, ""), |(k, v)| (k, v.trim()));
        let kind = Kind::from_key(k).ok_or_else(|| format!("unknown action “{k}”"))?;
        Action::from_parts(kind, v)
    }

    pub fn from_parts(kind: Kind, v: &str) -> Result<Action, String> {
        let v = v.trim();
        let bad = || format!("{}: invalid value “{v}”", kind.label());
        let flag = || on_off(v).ok_or_else(bad);
        let text = || if v.is_empty() { Err(format!("{}: a value is required", kind.label())) } else { Ok(v.to_string()) };
        Ok(match kind {
            Kind::PowerMode => Action::PowerMode(
                PowerMode::from_key(&v.to_ascii_lowercase())
                    .or_else(|| PowerMode::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(v)))
                    .ok_or_else(bad)?,
            ),
            Kind::BatteryMode => Action::BatteryMode(
                ChargeType::ALL
                    .into_iter()
                    .find(|t| charge_key(*t).eq_ignore_ascii_case(v) || t.label().eq_ignore_ascii_case(v))
                    .ok_or_else(bad)?,
            ),
            Kind::Preset => Action::Preset(text()?),
            Kind::CyclePreset => Action::CyclePreset,
            Kind::FnLock => Action::Toggle(Toggle::FnLock, flag()?),
            Kind::Camera => Action::Toggle(Toggle::Camera, flag()?),
            Kind::AlwaysOnUsb => Action::Toggle(Toggle::UsbCharging, flag()?),
            Kind::FanMax => Action::FanMax(flag()?),
            Kind::CpuBoost => Action::CpuBoost(flag()?),
            Kind::PanelLogo => Action::PanelLogo(flag()?),
            Kind::SpectrumProfile => {
                Action::SpectrumProfile(v.parse().ok().filter(|n| proto::PROFILES.contains(n)).ok_or_else(bad)?)
            }
            Kind::SpectrumBrightness => {
                Action::SpectrumBrightness(v.parse().ok().filter(|n| *n <= proto::MAX_BRIGHTNESS).ok_or_else(bad)?)
            }
            Kind::SpectrumImport => Action::SpectrumImport(text()?),
            Kind::Notify => Action::Notify(text()?),
            Kind::Run => Action::Run(text()?),
            Kind::Delay => Action::Delay(v.parse().ok().filter(|s| *s <= 3600).ok_or_else(bad)?),
        })
    }

    /// Подпись для списков: «Power Mode: Performance».
    pub fn describe(&self) -> String {
        let label = self.kind().label();
        let v = match self {
            Action::CyclePreset => return label.into(),
            Action::PowerMode(m) => m.label().to_string(),
            Action::BatteryMode(t) => t.label().to_string(),
            Action::Toggle(_, b) | Action::FanMax(b) | Action::CpuBoost(b) | Action::PanelLogo(b) => {
                (if *b { "On" } else { "Off" }).into()
            }
            Action::SpectrumProfile(n) => format!("Profile {n}"),
            Action::SpectrumBrightness(0) => "Off".into(),
            Action::Delay(s) => format!("{s} s"),
            a => a.value(),
        };
        format!("{label}: {v}")
    }
}

/// Сменить режим питания со всем, что к этому прилагается (то же, что кнопка
/// в окне): вернуть вентиляторы EC при выходе из «Свой» без программного авто
/// и выставить лимиты CPU/EPP режима, даже если служба не запущена.
pub fn set_power_mode(m: PowerMode) -> crate::hw::sysfs::Result<()> {
    let was = power::read_state().mode;
    power::set_mode(m)?;
    let saved = autoapply::load();
    if was == Some(PowerMode::Custom) && m != PowerMode::Custom && !saved.fan_curve && !saved.fan_max {
        autoapply::release_fans();
    }
    let _ = autoapply::apply_cpu(m);
    if m == PowerMode::Custom {
        let _ = autoapply::apply_custom();
    }
    Ok(())
}

/// Включить/выключить максимальные обороты. Вернуться из ручных оборотов
/// EC этой модели сам не может, поэтому вместе с ними включается
/// программное авто — после выключения вентиляторы снова идут по кривой.
pub fn set_fan_max(on: bool) -> Result<Vec<String>, String> {
    autoapply::remember_flag(Flag::FanMax, on);
    if on {
        autoapply::remember_flag(Flag::FanCurve, true);
        Ok(autoapply::apply_fan_max())
    } else {
        // Сразу на кривую — даже если служба не запущена (иначе обороты
        // так и остались бы максимальными). Дальше их ведёт служба.
        Ok(autoapply::apply_curve_once())
    }
}

/// Включить/выключить турбо-частоты CPU и запомнить выбор: после
/// перезагрузки турбо снова включено, служба вернёт сохранённое.
pub fn set_cpu_boost(on: bool) -> crate::hw::sysfs::Result<()> {
    cpufreq::set_boost(on)?;
    autoapply::remember_cpu_boost(on);
    Ok(())
}

/// Где взять клавиатуру: в окне — у фонового потока, иначе открыть свою.
pub struct Ctx<'a> {
    pub kb: Option<&'a mut Keyboard>,
    pub simulate: bool,
}

impl Ctx<'_> {
    fn keyboard<T>(&mut self, f: impl FnOnce(&mut Keyboard) -> crate::spectrum::device::Result<T>) -> Result<T, String> {
        match self.kb.as_deref_mut() {
            Some(kb) => f(kb).map_err(|e| e.to_string()),
            None => {
                let mut kb = Keyboard::open(self.simulate).map_err(|e| e.to_string())?;
                f(&mut kb).map_err(|e| e.to_string())
            }
        }
    }
}

/// Выполнить действие. Ok — короткое описание сделанного.
pub fn execute(a: &Action, ctx: &mut Ctx) -> Result<String, String> {
    let sys = |r: crate::hw::sysfs::Result<()>| r.map_err(|e| e.to_string());
    match a {
        Action::PowerMode(m) => sys(set_power_mode(*m))?,
        Action::BatteryMode(t) => sys(battery::set_charge_type(*t))?,
        Action::Preset(n) => {
            presets::activate(n)?;
        }
        Action::CyclePreset => {
            let n = presets::cycle()?;
            return Ok(format!("Custom Mode preset: {n}"));
        }
        Action::Toggle(t, on) => sys(device::set_toggle(*t, *on))?,
        Action::FanMax(on) => {
            set_fan_max(*on)?;
        }
        Action::CpuBoost(on) => sys(set_cpu_boost(*on))?,
        Action::SpectrumProfile(n) => ctx.keyboard(|kb| kb.set_profile(*n))?,
        Action::SpectrumBrightness(v) => ctx.keyboard(|kb| kb.set_brightness(*v))?,
        Action::PanelLogo(on) => ctx.keyboard(|kb| kb.set_logo(*on))?,
        Action::SpectrumImport(path) => {
            let text = std::fs::read_to_string(expand_home(path)).map_err(|e| format!("{path}: {e}"))?;
            let effects = profile_file::import(&text)?;
            ctx.keyboard(|kb| {
                let p = kb.profile()?;
                kb.set_effects(p, &effects)
            })?;
        }
        Action::Notify(text) => notify::send(notify::Kind::Automation, "Legion Control", text),
        Action::Run(cmd) => {
            std::process::Command::new("sh")
                .args(["-c", cmd])
                .stdin(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("run: {e}"))?;
        }
        Action::Delay(s) => std::thread::sleep(Duration::from_secs(*s as u64)),
    }
    Ok(a.describe())
}

/// Выполнить шаги по порядку; первая ошибка останавливает цепочку, как в
/// Legion Toolkit.
pub fn run_all(steps: &[Action], ctx: &mut Ctx) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for s in steps {
        done.push(execute(s, ctx).map_err(|e| format!("{}: {e}", s.kind().label()))?);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_roundtrip() {
        let all = [
            Action::PowerMode(PowerMode::Performance),
            Action::BatteryMode(ChargeType::LongLife),
            Action::Preset("Night mode".into()),
            Action::CyclePreset,
            Action::Toggle(Toggle::FnLock, true),
            Action::Toggle(Toggle::Camera, false),
            Action::Toggle(Toggle::UsbCharging, true),
            Action::FanMax(false),
            Action::CpuBoost(true),
            Action::SpectrumProfile(3),
            Action::SpectrumBrightness(0),
            Action::PanelLogo(true),
            Action::SpectrumImport("~/wave.json".into()),
            Action::Notify("Plugged in".into()),
            Action::Run("notify-send hi".into()),
            Action::Delay(5),
        ];
        for a in all {
            assert_eq!(Action::parse(&a.to_line()), Ok(a.clone()), "{}", a.to_line());
        }
        assert_eq!(Action::parse("power-mode Balance"), Ok(Action::PowerMode(PowerMode::Balanced)));
        assert_eq!(Action::parse("battery-mode Rapid Charge"), Ok(Action::BatteryMode(ChargeType::Fast)));
        assert!(Action::parse("spectrum-profile 7").is_err());
        assert!(Action::parse("spectrum-brightness 10").is_err());
        assert!(Action::parse("fn-lock maybe").is_err());
        assert!(Action::parse("run").is_err());
        assert!(Action::parse("explode now").is_err());
    }
}
