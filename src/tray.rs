//! Значок в трее (как у Legion Toolkit): режим питания, пресеты «Своего»,
//! CPU Boost, максимальные обороты, режим батареи, профили подсветки и
//! быстрые действия — в одном меню. Значок держит служба, поэтому он есть с
//! момента входа, даже когда окно закрыто.
//!
//! Протокол — StatusNotifierItem (D-Bus). В GNOME его показывает расширение
//! AppIndicator, в KDE и большинстве панелей — сама панель.

use crate::actions::{self, Action, Ctx};
use crate::automation::{self, Trigger};
use crate::autoapply;
use crate::hw::battery::{self, ChargeType};
use crate::hw::cpufreq;
use crate::hw::power::{self, PowerMode};
use crate::notify;
use crate::presets;
use crate::spectrum::protocol as proto;
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::MenuItem;

/// Всё, что показывает меню. Служба перечитывает это раз в секунду и
/// обновляет меню, только если что-то поменялось.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub mode: Option<PowerMode>,
    pub modes: Vec<PowerMode>,
    pub presets: Vec<String>,
    pub active_preset: Option<String>,
    pub boost: Option<bool>,
    pub fan_max: bool,
    pub fans: bool,
    pub charge: Option<ChargeType>,
    pub charge_types: Vec<ChargeType>,
    pub quick: Vec<String>,
}

pub fn read_state() -> State {
    let p = power::read_state();
    let all = presets::load();
    let saved = autoapply::load();
    let bat = battery::read_battery();
    State {
        mode: p.mode,
        modes: if p.choices.is_empty() { PowerMode::ALL.to_vec() } else { p.choices },
        presets: all.list.into_iter().map(|p| p.name).collect(),
        active_preset: all.active,
        boost: cpufreq::boost().map(|b| b.on),
        fan_max: saved.fan_max,
        fans: crate::hw::fans::read_fans().iter().any(|f| f.writable),
        charge: bat.as_ref().and_then(|b| b.charge_type),
        charge_types: bat.map(|b| b.charge_types).unwrap_or_default(),
        quick: automation::load().into_iter().filter(|r| r.trigger == Trigger::Manual).map(|r| r.name).collect(),
    }
}

pub struct LegionTray {
    pub state: State,
}

/// Выполнить действие из меню. Ошибку показать уведомлением: меню само
/// ничего сообщить не может.
fn run(a: Action) {
    std::thread::spawn(move || {
        if let Err(e) = actions::execute(&a, &mut Ctx { kb: None, simulate: false }) {
            eprintln!("linux-legion: tray: {}: {e}", a.describe());
            notify::send(notify::Kind::Automation, &a.kind().label().to_string(), &e);
        }
    });
}

fn run_quick(name: String) {
    std::thread::spawn(move || {
        let Some(rule) = automation::load().into_iter().find(|r| r.name == name) else { return };
        if let Err(e) = actions::run_all(&rule.steps, &mut Ctx { kb: None, simulate: false }) {
            eprintln!("linux-legion: tray: action “{name}”: {e}");
            notify::send(notify::Kind::Automation, &format!("Action “{name}” failed"), &e);
        }
    });
}

/// Открыть окно. Второй экземпляр не запускается: окно одно (single-instance),
/// повторный запуск просто выводит его на передний план.
pub fn open_window() {
    let exe = std::env::current_exe().unwrap_or_else(|_| "linux-legion".into());
    match std::process::Command::new(exe).stdin(std::process::Stdio::null()).spawn() {
        Ok(mut c) => {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
        Err(e) => eprintln!("linux-legion: tray: cannot open the window: {e}"),
    }
}

impl ksni::Tray for LegionTray {
    fn id(&self) -> String {
        "linux-legion".into()
    }

    fn title(&self) -> String {
        "Legion Control".into()
    }

    fn icon_name(&self) -> String {
        "linux-legion".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let mode = self.state.mode.map_or("Mode unknown", |m| m.label());
        ksni::ToolTip { title: "Legion Control".into(), description: format!("Power Mode: {mode}"), ..Default::default() }
    }

    /// Щелчок по значку открывает окно, меню — по правой кнопке.
    fn activate(&mut self, _x: i32, _y: i32) {
        open_window();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let st = &self.state;
        let mut items: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: "Open Legion Control".into(),
                icon_name: "linux-legion".into(),
                activate: Box::new(|_: &mut Self| open_window()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];

        // Режим питания — переключатель, как Fn+Q.
        let modes = st.modes.clone();
        items.push(
            RadioGroup {
                selected: st.mode.and_then(|m| modes.iter().position(|x| *x == m)).unwrap_or(usize::MAX),
                select: Box::new(move |this: &mut Self, i| {
                    if let Some(&m) = modes.get(i) {
                        this.state.mode = Some(m);
                        run(Action::PowerMode(m));
                    }
                }),
                options: st.modes.iter().map(|m| RadioItem { label: m.label().into(), ..Default::default() }).collect(),
            }
            .into(),
        );

        if !st.presets.is_empty() {
            let names = st.presets.clone();
            items.push(
                SubMenu {
                    label: "Custom Mode preset".into(),
                    submenu: vec![RadioGroup {
                        selected: st
                            .active_preset
                            .as_ref()
                            .and_then(|a| names.iter().position(|n| n == a))
                            .unwrap_or(usize::MAX),
                        select: Box::new(move |this: &mut Self, i| {
                            if let Some(n) = names.get(i) {
                                this.state.active_preset = Some(n.clone());
                                run(Action::Preset(n.clone()));
                            }
                        }),
                        options: st.presets.iter().map(|n| RadioItem { label: n.clone(), ..Default::default() }).collect(),
                    }
                    .into()],
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);

        if let Some(on) = st.boost {
            items.push(
                CheckmarkItem {
                    label: "CPU Boost".into(),
                    checked: on,
                    activate: Box::new(move |this: &mut Self| {
                        this.state.boost = Some(!on);
                        run(Action::CpuBoost(!on));
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        if st.fans {
            let on = st.fan_max;
            items.push(
                CheckmarkItem {
                    label: "Maximum fan speed".into(),
                    checked: on,
                    activate: Box::new(move |this: &mut Self| {
                        this.state.fan_max = !on;
                        run(Action::FanMax(!on));
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        if !st.charge_types.is_empty() {
            let types: Vec<ChargeType> = ChargeType::ALL.into_iter().filter(|t| st.charge_types.contains(t)).collect();
            let sel = st.charge.and_then(|c| types.iter().position(|t| *t == c)).unwrap_or(usize::MAX);
            let labels: Vec<RadioItem> = types.iter().map(|t| RadioItem { label: t.label().into(), ..Default::default() }).collect();
            items.push(
                SubMenu {
                    label: "Battery Mode".into(),
                    submenu: vec![RadioGroup {
                        selected: sel,
                        select: Box::new(move |this: &mut Self, i| {
                            if let Some(&t) = types.get(i) {
                                this.state.charge = Some(t);
                                run(Action::BatteryMode(t));
                            }
                        }),
                        options: labels,
                    }
                    .into()],
                    ..Default::default()
                }
                .into(),
            );
        }

        // Подсветка: служба не опрашивает контроллер (им занято окно), поэтому
        // здесь только команды, без отметки текущего профиля.
        let mut light: Vec<MenuItem<Self>> = proto::PROFILES
            .map(|n| {
                StandardItem {
                    label: format!("Profile {n}"),
                    activate: Box::new(move |_: &mut Self| run(Action::SpectrumProfile(n))),
                    ..Default::default()
                }
                .into()
            })
            .collect();
        light.push(MenuItem::Separator);
        light.push(
            StandardItem {
                label: "Backlight off".into(),
                activate: Box::new(|_: &mut Self| run(Action::SpectrumBrightness(0))),
                ..Default::default()
            }
            .into(),
        );
        light.push(
            StandardItem {
                label: "Full brightness".into(),
                activate: Box::new(|_: &mut Self| run(Action::SpectrumBrightness(proto::MAX_BRIGHTNESS))),
                ..Default::default()
            }
            .into(),
        );
        items.push(SubMenu { label: "Keyboard backlight".into(), submenu: light, ..Default::default() }.into());

        if !st.quick.is_empty() {
            items.push(
                SubMenu {
                    label: "Quick actions".into(),
                    submenu: st
                        .quick
                        .iter()
                        .map(|name| {
                            let n = name.clone();
                            StandardItem {
                                label: name.clone(),
                                activate: Box::new(move |_: &mut Self| run_quick(n.clone())),
                                ..Default::default()
                            }
                            .into()
                        })
                        .collect(),
                    ..Default::default()
                }
                .into(),
            );
        }
        items
    }
}

/// Значок, которым управляет служба: показать, обновить, убрать.
#[derive(Default)]
pub struct TrayIcon {
    handle: Option<ksni::blocking::Handle<LegionTray>>,
    /// Не повторять ошибку в журнал каждую секунду.
    failed: bool,
}

impl TrayIcon {
    /// Привести значок в соответствие настройке и состоянию ноутбука.
    pub fn sync(&mut self, enabled: bool) {
        if !enabled {
            if let Some(h) = self.handle.take() {
                h.shutdown().wait();
            }
            self.failed = false;
            return;
        }
        let state = read_state();
        match &self.handle {
            Some(h) if !h.is_closed() => {
                h.update(|t: &mut LegionTray| {
                    if t.state != state {
                        t.state = state;
                    }
                });
            }
            _ if self.failed => {}
            _ => match (LegionTray { state }).spawn() {
                Ok(h) => {
                    eprintln!("linux-legion: tray icon shown");
                    self.handle = Some(h);
                }
                Err(e) => {
                    // Нет сессионной шины или панели с треем — служба работает и без значка.
                    eprintln!("linux-legion: tray icon unavailable: {e}");
                    self.failed = true;
                }
            },
        }
    }
}
