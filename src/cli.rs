//! Командная строка — как `llt` из Legion Toolkit: удобно вешать на сочетания
//! клавиш рабочего стола и звать из скриптов.
//!
//! ```text
//! linux_legion list                         все функции и их значения
//! linux_legion get power-mode
//! linux_legion set power-mode performance
//! linux_legion values battery-mode          допустимые значения
//! linux_legion run "Gaming"                 быстрое действие (правило «вручную»)
//! linux_legion actions                      правила автоматизации
//! linux_legion spectrum export 3 wave.json  профиль подсветки → файл Legion Toolkit
//! linux_legion spectrum import 3 wave.json
//! ```

use crate::actions::{self, Action, Ctx, Kind, Value};
use crate::automation::{self, Trigger};
use crate::autoapply;
use crate::hw::battery;
use crate::hw::device::{self, Toggle};
use crate::hw::power;
use crate::presets;
use crate::spectrum::device::Keyboard;
use crate::spectrum::profile_file;
use crate::spectrum::protocol as proto;

pub const HELP: &str = "\
Usage: linux_legion [COMMAND]

Without a command the control center window opens.

Commands:
  list                          Show every feature and its current value
  get <feature>                 Print the current value of a feature
  set <feature> [value]         Change a feature (same names as the action steps)
  values <feature>              List the values a feature accepts
  run <name>                    Run a quick action (an action with the manual trigger)
  actions                       List the actions from automation.conf
  spectrum info                 Show the lighting controller, its zones and keyboard layout
  spectrum export <profile> <file.json>
                                Save a lighting profile in Lenovo Legion Toolkit format
  spectrum import <profile> <file.json>
                                Load a Legion Toolkit lighting profile into the keyboard
  --apply                       Apply the saved Custom Mode values once
  --daemon                      Run the service (linux-legion-autoapply.service)
  --simulate                    Use a simulated Legion Pro 7 (also with commands)
  --page N                      Open the window on page N (0 home … 5 actions)
  help                          Show this help

Features: power-mode, battery-mode, preset, cycle-preset, fan-max, cpu-boost, fn-lock, camera,
always-on-usb, spectrum-profile, spectrum-brightness, panel-logo";

/// Функции, у которых есть текущее значение.
const GETTABLE: [Kind; 11] = [
    Kind::PowerMode,
    Kind::BatteryMode,
    Kind::Preset,
    Kind::FanMax,
    Kind::CpuBoost,
    Kind::FnLock,
    Kind::Camera,
    Kind::AlwaysOnUsb,
    Kind::SpectrumProfile,
    Kind::SpectrumBrightness,
    Kind::PanelLogo,
];

fn on_off(b: bool) -> String {
    (if b { "on" } else { "off" }).into()
}

fn get(kind: Kind, simulate: bool, kb: &mut Option<Result<Keyboard, String>>) -> Result<String, String> {
    let toggle = |t: Toggle| {
        device::read_toggles()
            .items
            .into_iter()
            .find(|(x, ..)| *x == t)
            .map(|(_, on, _)| on_off(on))
            .ok_or_else(|| "not available (ideapad_acpi)".to_string())
    };
    let mut keyboard = |f: &dyn Fn(&mut Keyboard) -> crate::spectrum::device::Result<String>| {
        let kb = kb.get_or_insert_with(|| Keyboard::open(simulate).map_err(|e| e.to_string()));
        match kb {
            Ok(kb) => f(kb).map_err(|e| e.to_string()),
            Err(e) => Err(e.clone()),
        }
    };
    match kind {
        Kind::PowerMode => power::read_state().mode.map(|m| m.key().to_string()).ok_or_else(|| "not available".into()),
        Kind::BatteryMode => battery::read_battery()
            .and_then(|b| b.charge_type)
            .map(|t| Action::BatteryMode(t).value())
            .ok_or_else(|| "not available".into()),
        Kind::Preset => Ok(presets::load().active.unwrap_or_else(|| "none".into())),
        Kind::FanMax => Ok(on_off(autoapply::load().fan_max)),
        Kind::CpuBoost => crate::hw::cpufreq::boost().map(|b| on_off(b.on)).ok_or_else(|| "not available".into()),
        Kind::FnLock => toggle(Toggle::FnLock),
        Kind::Camera => toggle(Toggle::Camera),
        Kind::AlwaysOnUsb => toggle(Toggle::UsbCharging),
        Kind::SpectrumProfile => keyboard(&|kb| kb.profile().map(|p| p.to_string())),
        Kind::SpectrumBrightness => keyboard(&|kb| kb.brightness().map(|p| p.to_string())),
        Kind::PanelLogo => keyboard(&|kb| kb.logo().map(on_off)),
        k => Err(format!("“{}” has no current value", k.key())),
    }
}

fn kind_arg(s: Option<&String>) -> Result<Kind, String> {
    let s = s.ok_or("a feature name is required (see `linux_legion help`)")?;
    Kind::from_key(s).ok_or_else(|| format!("unknown feature “{s}” (see `linux_legion help`)"))
}

fn profile_arg(s: Option<&String>) -> Result<u8, String> {
    s.and_then(|s| s.parse().ok()).filter(|n| proto::PROFILES.contains(n)).ok_or_else(|| "profile must be 1…6".into())
}

/// Выполнить команду. `None` — команды нет, открыть окно.
pub fn run(args: &[String], simulate: bool) -> Option<Result<(), String>> {
    let cmd = args.first()?.as_str();
    let rest = &args[1..];
    let res = match cmd {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(())
        }
        "list" => {
            let mut kb = None;
            for k in GETTABLE {
                let v = get(k, simulate, &mut kb).unwrap_or_else(|e| format!("— ({e})"));
                println!("{:<21} {v}", k.key());
            }
            Ok(())
        }
        "get" => kind_arg(rest.first()).and_then(|k| get(k, simulate, &mut None)).map(|v| println!("{v}")),
        "values" => kind_arg(rest.first()).map(|k| match k.value() {
            Value::Choice(list) => list.into_iter().for_each(|(v, label)| println!("{v:<14} {label}")),
            Value::Text(hint) => println!("<{hint}>"),
            Value::None => println!("(no value)"),
        }),
        "set" => kind_arg(rest.first())
            .and_then(|k| Action::from_parts(k, &rest[1..].join(" ")))
            .and_then(|a| actions::execute(&a, &mut Ctx { kb: None, simulate }))
            .map(|done| println!("{done}")),
        "actions" => {
            let (rules, warnings) = automation::load_with_warnings();
            for w in warnings {
                eprintln!("automation.conf: {w}");
            }
            if rules.is_empty() {
                println!("no actions (create them on the Actions page)");
            }
            for r in rules {
                let state = if r.enabled { "" } else { " (disabled)" };
                println!("{}{state}\n  {}", r.name, r.trigger.describe());
                for s in &r.steps {
                    println!("  → {}", s.describe());
                }
            }
            Ok(())
        }
        "run" => {
            let name = rest.join(" ");
            let rules = automation::load();
            match rules.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                None => Err(format!("no action named “{name}”")),
                Some(r) if r.trigger != Trigger::Manual && !r.enabled => Err(format!("“{}” is disabled", r.name)),
                Some(r) => actions::run_all(&r.steps, &mut Ctx { kb: None, simulate }).map(|done| {
                    for d in done {
                        println!("{d}");
                    }
                }),
            }
        }
        "spectrum" => {
            let sub = rest.first().map(String::as_str);
            let (profile, file) = (profile_arg(rest.get(1)), rest.get(2));
            match (sub, profile, file) {
                (Some("info"), ..) => Keyboard::open(simulate)
                    .and_then(|mut kb| {
                        let m = kb.key_matrix()?;
                        Ok((kb.name(), m, kb.profile()?, kb.brightness()?))
                    })
                    .map_err(|e| e.to_string())
                    .map(|(name, m, profile, brightness)| {
                        let codes = m.codes();
                        let layout = crate::spectrum::layout::KeyboardLayout::detect(&codes);
                        println!("controller   {name}");
                        println!("matrix       {} × {}, {} zones", m.width, m.height, codes.len());
                        println!("layout       {}", layout.label());
                        println!("profile      {profile}");
                        println!("brightness   {brightness}");
                    }),
                (Some("export"), Ok(p), Some(f)) => Keyboard::open(simulate)
                    .and_then(|mut kb| {
                        // Чтение слоёв делает профиль активным — вернуть прежний.
                        let active = kb.profile()?;
                        let effects = kb.effects(p)?;
                        if active != p {
                            kb.set_profile(active)?;
                        }
                        Ok(effects)
                    })
                    .map_err(|e| e.to_string())
                    .and_then(|e| std::fs::write(f, profile_file::export(&e)).map_err(|e| format!("{f}: {e}")))
                    .map(|_| println!("profile {p} saved to {f}")),
                (Some("import"), Ok(p), Some(f)) => std::fs::read_to_string(f)
                    .map_err(|e| format!("{f}: {e}"))
                    .and_then(|t| profile_file::import(&t))
                    .and_then(|e| {
                        let mut kb = Keyboard::open(simulate).map_err(|e| e.to_string())?;
                        kb.set_effects(p, &e).map_err(|e| e.to_string())?;
                        Ok(e.len())
                    })
                    .map(|n| println!("{n} effects written to profile {p}")),
                (Some("export" | "import"), Err(e), _) => Err(e),
                _ => Err("usage: linux_legion spectrum info | export|import <profile 1…6> <file.json>".into()),
            }
        }
        _ => return None,
    };
    Some(res)
}
