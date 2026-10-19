//! Контекст приложения и оболочка окна: шапка, навигация, страница, строка состояния.

use super::{icons, pages, widgets};
use crate::hw::battery::Battery;
use crate::hw::device::{SystemInfo, Toggles};
use crate::hw::power::{PowerMode, PowerState};
use crate::hw::sensors::Sensors;
use crate::actions::Kind as StepKind;
use crate::automation::TriggerKind;
use crate::spectrum::protocol::Effect;
use crate::worker::{self, Job, NoticeKind, RgbLink, RgbState, Sink};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use syngui::prelude::*;
use syngui::widgets::GestureDetector;

const BRAND_PNG: &[u8] = include_bytes!("../../assets/icon/linux-legion-128.png");

/// Разделы навигации.
pub const PAGES: [(&str, &str); 6] = [
    ("Home", icons::DASHBOARD),
    ("Performance", icons::SPEED),
    ("Lighting", icons::PALETTE),
    ("Battery", icons::BATTERY_FULL),
    ("Device", icons::LAPTOP),
    ("Actions", icons::ACTIONS),
];

/// Черновик правила на странице действий: то, от чего зависит вид редактора.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// Номер правила в списке; `None` — новое.
    pub index: Option<usize>,
    pub trigger: TriggerKind,
    pub steps: Vec<StepKind>,
    pub error: Option<String>,
}

/// Значения полей черновика. Редактор читает их без подписки, чтобы
/// текстовое поле не пересоздавалось (и не теряло фокус) на каждой букве.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftVals {
    pub name: String,
    pub trigger: String,
    pub steps: Vec<String>,
}

#[derive(Clone)]
pub struct AppCtx {
    pub sink: Sink,
    pub page: RwSignal<usize>,
    /// Черновик лимитов мощности: имя атрибута → значение.
    pub tun_edit: RwSignal<BTreeMap<String, i64>>,
    /// Черновик оборотов: номер вентилятора → об/мин (0 — авто).
    pub fan_edit: RwSignal<BTreeMap<u8, u32>>,
    /// Редактируемые слои активного профиля подсветки.
    pub rgb_edit: RwSignal<Vec<Effect>>,
    pub rgb_layer: RwSignal<Option<usize>>,
    /// Выделенные зоны (коды), по возрастанию.
    pub rgb_sel: RwSignal<Vec<u16>>,
    /// Путь для экспорта/импорта профиля подсветки.
    pub rgb_file: RwSignal<String>,
    /// Имя, под которым сохранить пресет «Своего».
    pub preset_name: RwSignal<String>,
    pub preset_delete_open: RwSignal<bool>,
    pub gpu_confirm_open: RwSignal<bool>,
    pub act_draft: RwSignal<Option<Draft>>,
    pub act_vals: RwSignal<DraftVals>,
    pub act_delete: RwSignal<Option<usize>>,
    pub act_delete_open: RwSignal<bool>,
    jobs: Sender<Job>,
    pending: Arc<Mutex<Option<Receiver<Job>>>>,
    page_flag: Arc<AtomicU8>,
    simulate: bool,
}

impl AppCtx {
    /// Создаёт сигналы. Вызывать один раз, до `App::run`.
    pub fn new(simulate: bool) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            sink: Sink {
                sensors: use_signal(Sensors::default()),
                power: use_signal(PowerState::default()),
                tunables: use_signal(Vec::new()),
                fans: use_signal(Vec::new()),
                battery: use_signal(None::<Battery>),
                toggles: use_signal(Toggles::default()),
                info: use_signal(SystemInfo::default()),
                rgb: use_signal(RgbState::default()),
                rgb_frame: use_signal(Vec::new()),
                rgb_effects: use_signal(None),
                busy: use_signal(false),
                notice: use_signal(None),
                autoapply: use_signal(Default::default()),
                cpu: use_signal(Default::default()),
                presets: use_signal(Default::default()),
                rules: use_signal(Vec::new()),
                power_source: use_signal(Default::default()),
                gpu_apps: use_signal(Vec::new()),
            },
            page: use_signal(0usize),
            tun_edit: use_signal(BTreeMap::new()),
            fan_edit: use_signal(BTreeMap::new()),
            rgb_edit: use_signal(Vec::new()),
            rgb_layer: use_signal(None),
            rgb_sel: use_signal(Vec::new()),
            rgb_file: use_signal(String::from("~/legion-profile.json")),
            preset_name: use_signal(String::new()),
            preset_delete_open: use_signal(false),
            gpu_confirm_open: use_signal(false),
            act_draft: use_signal(None),
            act_vals: use_signal(DraftVals::default()),
            act_delete: use_signal(None),
            act_delete_open: use_signal(false),
            jobs: tx,
            pending: Arc::new(Mutex::new(Some(rx))),
            page_flag: Arc::new(AtomicU8::new(0)),
            simulate,
        }
    }

    /// Запускает фоновый поток и эффекты уровня приложения (после
    /// инициализации рантайма окна).
    pub fn start(&self) {
        if let Some(rx) = self.pending.lock().unwrap().take() {
            worker::spawn(self.sink, self.simulate, self.page_flag.clone(), rx);
        }
        self.install_effects();
    }

    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// Задания, накопившиеся без фонового потока (для тестов).
    #[cfg(test)]
    pub fn take_jobs(&self) -> Vec<Job> {
        let guard = self.pending.lock().unwrap();
        guard.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default()
    }

    pub fn go(&self, page: usize) {
        self.page.set(page);
        self.page_flag.store(page as u8, Ordering::Relaxed);
    }

    /// Эффекты живут всё время работы, поэтому ставятся вне страниц.
    fn install_effects(&self) {
        // Сменился профиль подсветки (с клавиатуры или из UI) — прочитать его слои.
        let c = self.clone();
        create_effect(move || {
            let st = c.sink.rgb.get();
            if st.link != RgbLink::Ready || st.profile == 0 {
                return;
            }
            if c.sink.rgb_effects.get_untracked().map(|e| e.0) != Some(st.profile) {
                c.send(Job::RgbLoad(st.profile));
            }
        });
        // Прочитанные слои становятся черновиком.
        let c = self.clone();
        create_effect(move || {
            let Some((_, effects)) = c.sink.rgb_effects.get() else { return };
            let first = (!effects.is_empty()).then_some(0);
            c.rgb_edit.set(effects);
            c.rgb_layer.set(first);
        });
    }

    /// Черновик слоёв отличается от записанного в контроллер.
    pub fn rgb_dirty(&self) -> bool {
        let edit = self.rgb_edit.get();
        self.sink.rgb_effects.get().is_some_and(|(_, e)| e != edit)
    }

    pub fn edit_rgb(&self, f: impl FnOnce(&mut Vec<Effect>)) {
        let mut v = self.rgb_edit.get_untracked();
        f(&mut v);
        self.rgb_edit.set(v);
    }

    pub fn power_mode(&self) -> Option<PowerMode> {
        self.sink.power.get().mode
    }

    /// Открыть редактор действия: существующего (`Some(i)`) или нового.
    pub fn open_draft(&self, index: Option<usize>) {
        use super::pages::actions::{default_step_value, default_trigger_value};
        let rules = self.sink.rules.get_untracked();
        let (draft, vals) = match index.and_then(|i| rules.get(i)) {
            Some(r) => (
                Draft { index, trigger: r.trigger.kind(), steps: r.steps.iter().map(|a| a.kind()).collect(), error: None },
                DraftVals {
                    name: r.name.clone(),
                    trigger: r.trigger.value(),
                    steps: r.steps.iter().map(|a| a.value()).collect(),
                },
            ),
            None => (
                Draft {
                    index: None,
                    trigger: TriggerKind::AcDisconnected,
                    steps: vec![StepKind::PowerMode],
                    error: None,
                },
                DraftVals {
                    name: String::new(),
                    trigger: default_trigger_value(TriggerKind::AcDisconnected),
                    steps: vec![default_step_value(StepKind::PowerMode)],
                },
            ),
        };
        self.act_vals.set(vals);
        self.act_draft.set(Some(draft));
    }

    pub fn move_step(&self, from: usize, to: usize) {
        self.act_vals.update(|v| {
            if from < v.steps.len() && to < v.steps.len() {
                v.steps.swap(from, to);
            }
        });
        self.act_draft.update(|d| {
            if let Some(d) = d.as_mut().filter(|d| from < d.steps.len() && to < d.steps.len()) {
                d.steps.swap(from, to);
            }
        });
    }

    pub fn remove_step(&self, i: usize) {
        self.act_vals.update(|v| {
            if i < v.steps.len() {
                v.steps.remove(i);
            }
        });
        self.act_draft.update(|d| {
            if let Some(d) = d.as_mut().filter(|d| i < d.steps.len()) {
                d.steps.remove(i);
            }
        });
    }

    pub fn save_draft(&self) {
        let Some(d) = self.act_draft.get_untracked() else { return };
        let mut rules = self.sink.rules.get_untracked();
        match super::pages::actions::build_rule(&d, &self.act_vals.get_untracked(), &rules) {
            Ok(rule) => {
                match d.index.filter(|&i| i < rules.len()) {
                    Some(i) => rules[i] = rule,
                    None => rules.push(rule),
                }
                self.send(Job::SaveRules(rules));
                self.act_draft.set(None);
            }
            Err(e) => self.act_draft.set(Some(Draft { error: Some(e), ..d })),
        }
    }
}

pub fn build_root(ctx: AppCtx) -> impl Widget {
    Column::new()
        .gap(0.0)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .child(header(ctx.clone()))
        .child(
            Row::new()
                .gap(0.0)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .child(nav(ctx.clone()))
                .child(content(ctx.clone()))
                .class("grow"),
        )
        .child(footer(ctx))
        .class("root")
}

fn header(ctx: AppCtx) -> impl Widget {
    let (c1, c2, c3) = (ctx.clone(), ctx.clone(), ctx.clone());
    Row::new()
        .gap(14.0)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .child(Image::from_bytes("brand-icon", BRAND_PNG.to_vec()).fit(ImageFit::Contain).class("brand-icon"))
        .child(
            Column::new()
                .gap(1.0)
                .child(Text::new("LEGION").class("brand-kicker"))
                .child(move || {
                    let model = c1.sink.info.get().model;
                    Text::new(if model.is_empty() { "Lenovo Legion".to_string() } else { model }).class("brand-title")
                }),
        )
        .child(DecoratedBox::new().class("grow"))
        .child(move || mode_pill(&c2))
        .child(widgets::reactive_box(move || temp_pill(c3.sink.sensors.get())))
        .child(widgets::reactive_box(move || battery_pill(ctx.sink.battery.get())))
        .class("header")
}

fn pill(content: impl Widget + 'static) -> impl Widget {
    DecoratedBox::new().child(content).class("pill")
}

fn mode_pill(ctx: &AppCtx) -> impl Widget {
    let (tone, text) = match ctx.power_mode() {
        Some(m) => (m.tone(), m.label()),
        None => ("idle", "Mode unknown"),
    };
    pill(
        Row::new()
            .gap(8.0)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .child(DecoratedBox::new().class(&format!("dot dot-{tone}")))
            .child(Text::new(text).class("pill-text")),
    )
}

fn temp_pill(s: Sensors) -> Box<dyn Widget> {
    let Some(t) = s.cpu_temp else { return widgets::nothing() };
    Box::new(pill(
        Row::new()
            .gap(6.0)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .child(Icon::new(icons::THERMOSTAT).class("pill-icon"))
            .child(Text::new(format!("CPU {t:.0}°")).class("pill-text")),
    ))
}

fn battery_pill(b: Option<Battery>) -> Box<dyn Widget> {
    let Some(b) = b else { return widgets::nothing() };
    let icon = if b.ac_online { icons::BATTERY_CHARGING } else { icons::BATTERY_FULL };
    let tone = if !b.ac_online && b.percent <= 15 { "bad" } else { "ok" };
    Box::new(pill(
        Row::new()
            .gap(6.0)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .child(Icon::new(icon).class(&format!("pill-icon tone-{tone}")))
            .child(Text::new(format!("{}%", b.percent)).class("pill-text")),
    ))
}

fn nav(ctx: AppCtx) -> impl Widget {
    let mut col = Column::new().gap(6.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
    col = col.child(Text::new("CONTROL CENTER").class("nav-caption"));
    for (i, (label, icon)) in PAGES.into_iter().enumerate() {
        let (c, sel) = (ctx.clone(), ctx.page);
        col = col.child(
            GestureDetector::new()
                .on_click(move || c.go(i))
                .cursor(syngui::CursorIcon::Pointer)
                .child(move || {
                    let on = sel.get() == i;
                    DecoratedBox::new()
                        .child(
                            Row::new()
                                .gap(12.0)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .child(Icon::new(icon).class(if on { "nav-icon nav-icon-on" } else { "nav-icon" }))
                                .child(Text::new(label).class(if on { "nav-text nav-text-on" } else { "nav-text" })),
                        )
                        .class(if on { "nav-item nav-item-on" } else { "nav-item" })
                }),
        );
    }
    let info = ctx.sink.info;
    col.child(DecoratedBox::new().class("grow"))
        .child(move || {
            let i = info.get();
            Column::new()
                .gap(3.0)
                .child(Text::new(i.machine_type.clone()).class("nav-foot-strong"))
                .child(Text::new(format!("BIOS {}", i.bios)).class("nav-foot"))
                .class("nav-footer")
        })
        .class("nav")
}

fn content(ctx: AppCtx) -> impl Widget {
    ScrollView::new()
        .vertical()
        .child(Padding::all(24.0).child(widgets::reactive_box(move || match ctx.page.get() {
            0 => Box::new(pages::home::view(ctx.clone())),
            1 => Box::new(pages::power::view(ctx.clone())),
            2 => Box::new(pages::lighting::view(ctx.clone())),
            3 => Box::new(pages::battery::view(ctx.clone())),
            4 => Box::new(pages::device::view(ctx.clone())),
            _ => Box::new(pages::actions::view(ctx.clone())),
        })))
        .class("content")
        .class("grow")
}

fn footer(ctx: AppCtx) -> impl Widget {
    let c = ctx.clone();
    Row::new()
        .gap(10.0)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .child(widgets::reactive_box(move || status_line(&c)))
        .child(DecoratedBox::new().class("grow"))
        .child(move || {
            let text = if ctx.simulate { "demo mode · simulated data" } else { concat!("linux-legion ", env!("CARGO_PKG_VERSION")) };
            Text::new(text).class("footer-note")
        })
        .class("footer")
}

fn status_line(ctx: &AppCtx) -> Box<dyn Widget> {
    if ctx.sink.busy.get() {
        return Box::new(
            Row::new()
                .gap(10.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .child(CircularProgress::new().indeterminate().size(16.0).stroke_width(2.0))
                .child(Text::new("Writing to controller…").class("status-text")),
        );
    }
    match ctx.sink.notice.get() {
        Some(n) => {
            let (icon, tone) = match n.kind {
                NoticeKind::Success => (icons::CHECK, "ok"),
                NoticeKind::Error => (icons::ERROR, "bad"),
            };
            Box::new(
                Row::new()
                    .gap(8.0)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .child(Icon::new(icon).class(&format!("status-icon tone-{tone}")))
                    .child(Text::new(n.text).class("status-text")),
            )
        }
        None => Box::new(Text::new("Ready").class("status-text")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::automation::{Rule, Trigger};
    use syngui::testing::*;

    const STYLES: &str = include_str!("../../styles/app.mss");

    /// Каждая страница собирается и раскладывается без паники — в том числе
    /// действия с открытым редактором и пресеты.
    #[test]
    fn every_page_builds() {
        let _ = TestHarness::new(Box::new(DecoratedBox::new()));
        let ctx = AppCtx::new(true);
        ctx.sink.rules.set(vec![
            Rule { name: "Quick".into(), enabled: true, trigger: Trigger::Manual, steps: vec![Action::Delay(1)] },
            Rule {
                name: "Unplugged".into(),
                enabled: true,
                trigger: Trigger::AcDisconnected,
                steps: vec![Action::PowerMode(PowerMode::Quiet), Action::SpectrumBrightness(2)],
            },
        ]);
        ctx.sink.presets.set(crate::presets::Presets {
            active: Some("Gaming".into()),
            list: vec![crate::presets::Preset { name: "Gaming".into(), ..Default::default() }],
        });
        ctx.open_draft(Some(1));
        for page in 0..PAGES.len() {
            ctx.go(page);
            let mut h = TestHarness::new(Box::new(build_root(ctx.clone())));
            h.apply_mss(STYLES);
            for _ in 0..3 {
                h.rebuild();
                h.apply_mss_dirty(STYLES);
                h.layout(1360.0, 900.0);
            }
            let _ = h.paint();
            assert!(h.element_count() > 20, "page {page} is nearly empty");
        }
    }

    #[test]
    fn draft_validation_and_save() {
        let _ = TestHarness::new(Box::new(DecoratedBox::new()));
        let ctx = AppCtx::new(true);
        ctx.sink.rules.set(vec![Rule { name: "Taken".into(), enabled: false, trigger: Trigger::Startup, steps: vec![] }]);
        ctx.open_draft(None);
        ctx.save_draft();
        assert!(ctx.act_draft.get_untracked().unwrap().error.unwrap().contains("name"));
        ctx.act_vals.update(|v| v.name = "taken".into());
        ctx.save_draft();
        assert!(ctx.act_draft.get_untracked().unwrap().error.unwrap().contains("already exists"));
        ctx.act_vals.update(|v| v.name = "On battery".into());
        // Шаг: режим по умолчанию — первый в списке (Quiet); добавим второй и удалим первый.
        ctx.act_draft.update(|d| d.as_mut().unwrap().steps.push(crate::actions::Kind::Delay));
        ctx.act_vals.update(|v| v.steps.push("3".into()));
        ctx.move_step(0, 1);
        ctx.remove_step(1);
        ctx.save_draft();
        assert!(ctx.act_draft.get_untracked().is_none(), "editor closes on save");
        let jobs = ctx.take_jobs();
        let Some(Job::SaveRules(rules)) = jobs.last() else { panic!("no SaveRules job") };
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].name, "On battery");
        assert_eq!(rules[1].trigger, Trigger::AcDisconnected);
        assert_eq!(rules[1].steps, vec![Action::Delay(3)]);
    }
}
