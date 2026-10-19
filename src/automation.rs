//! Автоматизация — правила «когда → что сделать», как раздел Actions в Legion
//! Toolkit. Правила хранятся в `~/.config/linux-legion/automation.conf`,
//! срабатывают в службе (`--daemon`); правила с триггером «вручную» — это
//! быстрые действия: их запускают кнопкой в окне или `linux_legion run ИМЯ`.
//!
//! ```text
//! [On battery]
//! enabled=1
//! trigger=ac-disconnected
//! step=power-mode quiet
//! step=spectrum-brightness 2
//! ```

use crate::actions::Action;
use crate::hw::battery;
use crate::hw::charger::{self, Charger};
use crate::hw::power::{self, PowerMode};
use crate::hw::session;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Быстрое действие: только вручную.
    Manual,
    /// Запуск службы — вход в систему.
    Startup,
    Resume,
    AcConnected,
    AcDisconnected,
    LowWattageCharger,
    /// Заряд опустился до N % на батарее.
    BatteryBelow(u8),
    LidOpened,
    LidClosed,
    /// Режим сменился на этот (в том числе по Fn+Q).
    PowerModeChanged(PowerMode),
    /// Каждый день в ЧЧ:ММ.
    Time(u8, u8),
    /// Запустилась хотя бы одна из программ (игры и т. п.).
    ProcessesStarted(Vec<String>),
    /// Не осталось ни одной из программ.
    ProcessesStopped(Vec<String>),
    DisplayConnected,
    DisplayDisconnected,
}

/// Вид триггера — для редактора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    Manual,
    Startup,
    Resume,
    AcConnected,
    AcDisconnected,
    LowWattageCharger,
    BatteryBelow,
    LidOpened,
    LidClosed,
    PowerModeChanged,
    Time,
    ProcessesStarted,
    ProcessesStopped,
    DisplayConnected,
    DisplayDisconnected,
}

/// Параметр триггера в редакторе.
pub enum Param {
    None,
    Choice(Vec<(String, String)>),
    Text(&'static str),
}

impl TriggerKind {
    pub const ALL: [TriggerKind; 15] = [
        TriggerKind::Manual,
        TriggerKind::AcConnected,
        TriggerKind::AcDisconnected,
        TriggerKind::LowWattageCharger,
        TriggerKind::BatteryBelow,
        TriggerKind::PowerModeChanged,
        TriggerKind::ProcessesStarted,
        TriggerKind::ProcessesStopped,
        TriggerKind::LidOpened,
        TriggerKind::LidClosed,
        TriggerKind::DisplayConnected,
        TriggerKind::DisplayDisconnected,
        TriggerKind::Time,
        TriggerKind::Startup,
        TriggerKind::Resume,
    ];

    pub fn key(self) -> &'static str {
        match self {
            TriggerKind::Manual => "manual",
            TriggerKind::Startup => "startup",
            TriggerKind::Resume => "resume",
            TriggerKind::AcConnected => "ac-connected",
            TriggerKind::AcDisconnected => "ac-disconnected",
            TriggerKind::LowWattageCharger => "low-wattage-charger",
            TriggerKind::BatteryBelow => "battery-below",
            TriggerKind::LidOpened => "lid-opened",
            TriggerKind::LidClosed => "lid-closed",
            TriggerKind::PowerModeChanged => "power-mode",
            TriggerKind::Time => "time",
            TriggerKind::ProcessesStarted => "processes-started",
            TriggerKind::ProcessesStopped => "processes-stopped",
            TriggerKind::DisplayConnected => "display-connected",
            TriggerKind::DisplayDisconnected => "display-disconnected",
        }
    }

    /// Подписи — как у триггеров Legion Toolkit.
    pub fn label(self) -> &'static str {
        match self {
            TriggerKind::Manual => "Quick action (run manually)",
            TriggerKind::Startup => "When the service starts (at login)",
            TriggerKind::Resume => "When resumed from sleep",
            TriggerKind::AcConnected => "When AC adapter is connected",
            TriggerKind::AcDisconnected => "When AC adapter is disconnected",
            TriggerKind::LowWattageCharger => "When low wattage charger is connected",
            TriggerKind::BatteryBelow => "When battery level drops to",
            TriggerKind::LidOpened => "When lid is opened",
            TriggerKind::LidClosed => "When lid is closed",
            TriggerKind::PowerModeChanged => "When power mode changes to",
            TriggerKind::Time => "At a specific time",
            TriggerKind::ProcessesStarted => "When apps start",
            TriggerKind::ProcessesStopped => "When apps stop",
            TriggerKind::DisplayConnected => "When external display is connected",
            TriggerKind::DisplayDisconnected => "When external display is disconnected",
        }
    }

    pub fn from_key(s: &str) -> Option<TriggerKind> {
        TriggerKind::ALL.into_iter().find(|k| k.key() == s)
    }

    pub fn param(self) -> Param {
        match self {
            TriggerKind::BatteryBelow => {
                Param::Choice([5, 10, 15, 20, 25, 30, 40, 50].map(|n| (n.to_string(), format!("{n} %"))).to_vec())
            }
            TriggerKind::PowerModeChanged => {
                Param::Choice(PowerMode::ALL.iter().map(|m| (m.key().to_string(), m.label().to_string())).collect())
            }
            TriggerKind::Time => Param::Text("HH:MM"),
            TriggerKind::ProcessesStarted | TriggerKind::ProcessesStopped => Param::Text("steam, cs2, game.exe"),
            _ => Param::None,
        }
    }
}

fn names(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

impl Trigger {
    pub fn kind(&self) -> TriggerKind {
        match self {
            Trigger::Manual => TriggerKind::Manual,
            Trigger::Startup => TriggerKind::Startup,
            Trigger::Resume => TriggerKind::Resume,
            Trigger::AcConnected => TriggerKind::AcConnected,
            Trigger::AcDisconnected => TriggerKind::AcDisconnected,
            Trigger::LowWattageCharger => TriggerKind::LowWattageCharger,
            Trigger::BatteryBelow(_) => TriggerKind::BatteryBelow,
            Trigger::LidOpened => TriggerKind::LidOpened,
            Trigger::LidClosed => TriggerKind::LidClosed,
            Trigger::PowerModeChanged(_) => TriggerKind::PowerModeChanged,
            Trigger::Time(..) => TriggerKind::Time,
            Trigger::ProcessesStarted(_) => TriggerKind::ProcessesStarted,
            Trigger::ProcessesStopped(_) => TriggerKind::ProcessesStopped,
            Trigger::DisplayConnected => TriggerKind::DisplayConnected,
            Trigger::DisplayDisconnected => TriggerKind::DisplayDisconnected,
        }
    }

    pub fn value(&self) -> String {
        match self {
            Trigger::BatteryBelow(n) => n.to_string(),
            Trigger::PowerModeChanged(m) => m.key().into(),
            Trigger::Time(h, m) => format!("{h:02}:{m:02}"),
            Trigger::ProcessesStarted(n) | Trigger::ProcessesStopped(n) => n.join(", "),
            _ => String::new(),
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

    pub fn parse(line: &str) -> Result<Trigger, String> {
        let line = line.trim();
        let (k, v) = line.split_once(char::is_whitespace).map_or((line, ""), |(k, v)| (k, v.trim()));
        let kind = TriggerKind::from_key(k).ok_or_else(|| format!("unknown trigger “{k}”"))?;
        Trigger::from_parts(kind, v)
    }

    pub fn from_parts(kind: TriggerKind, v: &str) -> Result<Trigger, String> {
        let v = v.trim();
        let bad = || format!("{}: invalid value “{v}”", kind.label());
        Ok(match kind {
            TriggerKind::Manual => Trigger::Manual,
            TriggerKind::Startup => Trigger::Startup,
            TriggerKind::Resume => Trigger::Resume,
            TriggerKind::AcConnected => Trigger::AcConnected,
            TriggerKind::AcDisconnected => Trigger::AcDisconnected,
            TriggerKind::LowWattageCharger => Trigger::LowWattageCharger,
            TriggerKind::BatteryBelow => {
                Trigger::BatteryBelow(v.trim_end_matches('%').trim().parse().ok().filter(|n| (1..=99).contains(n)).ok_or_else(bad)?)
            }
            TriggerKind::LidOpened => Trigger::LidOpened,
            TriggerKind::LidClosed => Trigger::LidClosed,
            TriggerKind::PowerModeChanged => Trigger::PowerModeChanged(
                PowerMode::from_key(&v.to_ascii_lowercase())
                    .or_else(|| PowerMode::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(v)))
                    .ok_or_else(bad)?,
            ),
            TriggerKind::Time => {
                let (h, m) = v.split_once(':').ok_or_else(bad)?;
                let (h, m): (u8, u8) = (h.trim().parse().map_err(|_| bad())?, m.trim().parse().map_err(|_| bad())?);
                if h > 23 || m > 59 {
                    return Err(bad());
                }
                Trigger::Time(h, m)
            }
            TriggerKind::ProcessesStarted | TriggerKind::ProcessesStopped => {
                let n = names(v);
                if n.is_empty() {
                    return Err(format!("{}: list at least one app", kind.label()));
                }
                if kind == TriggerKind::ProcessesStarted {
                    Trigger::ProcessesStarted(n)
                } else {
                    Trigger::ProcessesStopped(n)
                }
            }
            TriggerKind::DisplayConnected => Trigger::DisplayConnected,
            TriggerKind::DisplayDisconnected => Trigger::DisplayDisconnected,
        })
    }

    pub fn describe(&self) -> String {
        match self {
            Trigger::BatteryBelow(n) => format!("{} {n} %", self.kind().label()),
            Trigger::PowerModeChanged(m) => format!("{} {}", self.kind().label(), m.label()),
            Trigger::Time(h, m) => format!("Every day at {h:02}:{m:02}"),
            Trigger::ProcessesStarted(n) => format!("When {} starts", n.join(", ")),
            Trigger::ProcessesStopped(n) => format!("When {} stops", n.join(", ")),
            t => t.kind().label().into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub steps: Vec<Action>,
}

/// Разобрать automation.conf. Ошибочные строки пропускаются, но попадают в
/// список предупреждений — чтобы правку руками было видно в журнале службы.
pub fn parse(text: &str) -> (Vec<Rule>, Vec<String>) {
    let mut rules: Vec<Rule> = Vec::new();
    let mut warnings = Vec::new();
    // Правило с ошибкой не должно срабатывать (неразобранный триггер иначе
    // молча превратился бы в «вручную») — оно выключается.
    let mut broken: Vec<usize> = Vec::new();
    for (no, line) in text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            rules.push(Rule { name: name.trim().to_string(), enabled: true, trigger: Trigger::Manual, steps: Vec::new() });
            continue;
        }
        let (Some(r), Some((k, v))) = (rules.last_mut(), line.split_once('=')) else {
            warnings.push(format!("line {no}: ignored"));
            continue;
        };
        let res = match k.trim() {
            "enabled" => {
                r.enabled = v.trim() != "0";
                Ok(())
            }
            "trigger" => Trigger::parse(v).map(|t| r.trigger = t),
            "step" => Action::parse(v).map(|a| r.steps.push(a)),
            other => Err(format!("unknown key “{other}”")),
        };
        if let Err(e) = res {
            warnings.push(format!("line {no}: {e}"));
            broken.push(rules.len() - 1);
        }
    }
    for i in broken {
        rules[i].enabled = false;
    }
    (rules, warnings)
}

pub fn render(rules: &[Rule]) -> String {
    let mut s = String::from("# linux-legion: actions (see `linux_legion help`)\n");
    for r in rules {
        s.push_str(&format!("\n[{}]\nenabled={}\ntrigger={}\n", r.name, r.enabled as u8, r.trigger.to_line()));
        for a in &r.steps {
            s.push_str(&format!("step={}\n", a.to_line()));
        }
    }
    s
}

fn path() -> std::path::PathBuf {
    crate::autoapply::config_dir().join("automation.conf")
}

pub fn load() -> Vec<Rule> {
    std::fs::read_to_string(path()).map(|t| parse(&t).0).unwrap_or_default()
}

pub fn load_with_warnings() -> (Vec<Rule>, Vec<String>) {
    std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default()
}

pub fn store(rules: &[Rule]) -> Result<(), String> {
    crate::autoapply::write_config(&path(), &render(rules)).map_err(|e| e.to_string())
}

pub fn mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

/// Имя правила: как у пресетов — без скобок, `=` и `#`.
pub fn valid_name(name: &str) -> bool {
    crate::presets::valid_name(name)
}

// ---------------------------------------------------------------------------
// Срабатывание

/// Снимок того, на что реагируют триггеры.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub charger: Charger,
    pub battery: Option<u8>,
    pub lid_closed: Option<bool>,
    pub mode: Option<PowerMode>,
    /// (часы, минуты) по местному времени.
    pub clock: (u8, u8),
    pub displays: usize,
    /// Для каждого правила с программами — запущена ли хоть одна.
    pub apps: Vec<bool>,
}

/// Местное время без зависимостей: `localtime_r` из libc.
pub fn local_clock() -> (u8, u8) {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_hour as u8, tm.tm_min as u8)
    }
}

/// Прочитать состояние. Программы ищутся, только если есть правила на них.
pub fn snapshot(rules: &[Rule]) -> Snapshot {
    let app_rules: Vec<&Vec<String>> = rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| match &r.trigger {
            Trigger::ProcessesStarted(n) | Trigger::ProcessesStopped(n) => Some(n),
            _ => None,
        })
        .collect();
    let apps = if app_rules.is_empty() {
        Vec::new()
    } else {
        let running = session::process_names();
        app_rules.iter().map(|n| session::any_running(&running, n)).collect()
    };
    Snapshot {
        charger: charger::read(),
        battery: battery::read_battery().map(|b| b.percent),
        lid_closed: session::lid_closed(),
        mode: power::read_state().mode,
        clock: local_clock(),
        displays: session::external_displays(),
        apps,
    }
}

/// Какие правила сработали между двумя снимками. Триггеры — по фронту: «AC
/// подключён» срабатывает в момент подключения, а не всё время, пока он
/// подключён. `startup`/`resume` — события самой службы.
pub fn fired<'a>(rules: &'a [Rule], prev: &Snapshot, now: &Snapshot, startup: bool, resumed: bool) -> Vec<&'a Rule> {
    let mut app_i = 0;
    let mut out = Vec::new();
    for r in rules.iter().filter(|r| r.enabled) {
        let hit = match &r.trigger {
            Trigger::Manual => false,
            Trigger::Startup => startup,
            Trigger::Resume => resumed,
            Trigger::AcConnected => !startup && !prev.charger.connected() && now.charger.connected(),
            Trigger::AcDisconnected => !startup && prev.charger.connected() && !now.charger.connected(),
            Trigger::LowWattageCharger => !prev.charger.low_wattage() && now.charger.low_wattage(),
            Trigger::BatteryBelow(n) => {
                !now.charger.connected() && now.battery.is_some_and(|b| b <= *n) && prev.battery.is_some_and(|b| b > *n)
            }
            Trigger::LidOpened => prev.lid_closed == Some(true) && now.lid_closed == Some(false),
            Trigger::LidClosed => prev.lid_closed == Some(false) && now.lid_closed == Some(true),
            Trigger::PowerModeChanged(m) => !startup && prev.mode != now.mode && now.mode == Some(*m),
            Trigger::Time(h, m) => prev.clock != now.clock && now.clock == (*h, *m),
            Trigger::ProcessesStarted(_) | Trigger::ProcessesStopped(_) => {
                let (was, is) = (prev.apps.get(app_i).copied(), now.apps.get(app_i).copied());
                app_i += 1;
                match (&r.trigger, was, is) {
                    // При старте службы уже запущенная игра тоже считается.
                    (Trigger::ProcessesStarted(_), _, Some(true)) if startup => true,
                    (Trigger::ProcessesStarted(_), Some(false), Some(true)) => true,
                    (Trigger::ProcessesStopped(_), Some(true), Some(false)) => true,
                    _ => false,
                }
            }
            Trigger::DisplayConnected => !startup && now.displays > prev.displays,
            Trigger::DisplayDisconnected => !startup && now.displays < prev.displays,
        };
        if hit {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(t: &str) -> Rule {
        Rule { name: t.into(), enabled: true, trigger: Trigger::parse(t).unwrap(), steps: Vec::new() }
    }

    #[test]
    fn config_roundtrip() {
        let rules = vec![
            Rule {
                name: "On battery".into(),
                enabled: true,
                trigger: Trigger::AcDisconnected,
                steps: vec![Action::PowerMode(PowerMode::Quiet), Action::SpectrumBrightness(2)],
            },
            Rule {
                name: "Games".into(),
                enabled: false,
                trigger: Trigger::ProcessesStarted(vec!["steam".into(), "cs2.exe".into()]),
                steps: vec![Action::Preset("Gaming".into())],
            },
            Rule { name: "Night".into(), enabled: true, trigger: Trigger::Time(22, 5), steps: vec![Action::Delay(3)] },
        ];
        let (back, warn) = parse(&render(&rules));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(back, rules);
        let (r, warn) = parse("orphan=1\n[A]\ntrigger=time 25:00\nstep=nope\nenabled=1\n[B]\ntrigger=startup\n");
        assert_eq!(warn.len(), 3);
        assert!(!r[0].enabled, "a rule with errors is disabled");
        assert_eq!(r[0].trigger, Trigger::Manual);
        assert!(r[1].enabled);
    }

    #[test]
    fn edges() {
        let rules = vec![
            rule("ac-connected"),
            rule("ac-disconnected"),
            rule("battery-below 20"),
            rule("power-mode extreme"),
            rule("processes-started steam"),
            rule("processes-stopped steam"),
            rule("time 07:30"),
            rule("startup"),
            rule("manual"),
        ];
        let base = Snapshot { charger: Charger::Barrel, battery: Some(25), mode: Some(PowerMode::Balanced), apps: vec![false, false], ..Default::default() };
        let names = |v: Vec<&Rule>| v.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert!(fired(&rules, &base, &base, false, false).is_empty());
        let unplug = Snapshot { charger: Charger::None, ..base.clone() };
        assert_eq!(names(fired(&rules, &base, &unplug, false, false)), ["ac-disconnected"]);
        let low = Snapshot { battery: Some(20), ..unplug.clone() };
        assert_eq!(names(fired(&rules, &unplug, &low, false, false)), ["battery-below 20"]);
        let extreme = Snapshot { mode: Some(PowerMode::Extreme), apps: vec![true, true], clock: (7, 30), ..base.clone() };
        assert_eq!(
            names(fired(&rules, &base, &extreme, false, false)),
            ["power-mode extreme", "processes-started steam", "time 07:30"]
        );
        assert_eq!(names(fired(&rules, &extreme, &base, false, false)), ["processes-stopped steam"]);
        // При старте служба сравнивает снимок сам с собой: срабатывают
        // «startup» и уже запущенные игры, но не время и не «AC подключён».
        assert_eq!(names(fired(&rules, &extreme, &extreme, true, false)), ["processes-started steam", "startup"]);
    }
}
