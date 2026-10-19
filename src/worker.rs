//! Фоновый поток: опрашивает датчики и sysfs, общается с контроллером
//! подсветки и выполняет команды UI. Результаты уходят в сигналы
//! (`set()` потокобезопасен); читать сигналы отсюда нельзя.

use crate::actions;
use crate::automation::{self, Rule};
use crate::autoapply::{self, Flag, Saved, ServiceState};
use crate::hw::battery::{self, Battery, ChargeType, Status};
use crate::hw::charger::{self, Charger};
use crate::hw::gpu::{self as gpu_apps, GpuProcess};
use crate::hw::cpufreq::{self, Boost, Epp};
use crate::hw::device::{self, SystemInfo, Toggle, Toggles};
use crate::hw::fans::{self, Fan};
use crate::hw::power::{self, PowerMode, PowerState, Tunable};
use crate::hw::rapl::{self, Rapl};
use crate::hw::sensors::{Gpu, Sampler, Sensors};
use crate::hw::sysfs;
use crate::notify;
use crate::presets::{self, Presets};
use crate::spectrum::device::{Error as KbError, KeyMatrix, Keyboard};
use crate::spectrum::profile_file;
use crate::spectrum::protocol::{Effect, Rgb};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use syngui::prelude::*;

/// Разделы, от которых зависит частота опроса.
pub mod page {
    pub const HOME: u8 = 0;
    pub const LIGHTING: u8 = 2;
}

const SYSFS_POLL: Duration = Duration::from_millis(1000);
/// Видеокарту опрашиваем реже, на батарее — ещё реже.
const GPU_POLL_AC: Duration = Duration::from_secs(2);
const GPU_POLL_BATTERY: Duration = Duration::from_secs(10);
const RGB_FRAME: Duration = Duration::from_millis(100);
const RGB_STATUS: Duration = Duration::from_millis(1500);
const RGB_RETRY: Duration = Duration::from_secs(3);

pub enum Job {
    SetPowerMode(PowerMode),
    SetTunable(String, i64),
    SetFanTarget(u8, u32),
    SetChargeType(ChargeType),
    SetToggle(Toggle, bool),
    RgbProfile(u8),
    RgbBrightness(u8),
    RgbLogo(bool),
    /// Перечитать слои профиля в `rgb_effects`.
    RgbLoad(u8),
    RgbWrite(u8, Vec<Effect>),
    RgbReset(u8),
    /// Включить/выключить службу автоприменения.
    SetAutoApply(bool),
    /// Переключатель службы (кривая вентиляторов, RAPL, EPP, максимум оборотов).
    SetFlag(Flag, bool),
    SetNotify(notify::Kind, bool),
    SetCpuBoost(bool),
    ActivatePreset(String),
    SavePreset(String),
    DeletePreset(String),
    /// Записать правила автоматизации целиком.
    SaveRules(Vec<Rule>),
    /// Выполнить шаги правила (быстрое действие или «Run now»).
    RunRule(String),
    /// Профиль подсветки → файл Legion Toolkit и обратно.
    RgbExport(u8, String),
    RgbImport(u8, String),
    /// Завершить программы, которые держат дискретную видеокарту.
    DeactivateGpu,
}

/// Источник питания и разряд за время работы от батареи (как на странице
/// батареи в Legion Toolkit).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PowerSource {
    pub charger: Charger,
    pub on_battery_since: Option<std::time::SystemTime>,
    /// Наименьший и наибольший разряд с момента отключения, Вт.
    pub min_discharge_w: Option<f32>,
    pub max_discharge_w: Option<f32>,
}

/// Лимиты CPU (RAPL) и EPP — то, чем управляет служба по режиму.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CpuState {
    pub rapl: Rapl,
    pub epp: Epp,
    /// Турбо-частоты; `None` — драйвер их не показывает.
    pub boost: Option<Boost>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum RgbLink {
    #[default]
    Searching,
    NotFound,
    NoAccess(String),
    Error(String),
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RgbState {
    pub link: RgbLink,
    pub name: String,
    pub profile: u8,
    pub brightness: u8,
    pub logo: bool,
    pub matrix: KeyMatrix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Success,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
    /// Растёт с каждым уведомлением — одинаковый текст всё равно покажется.
    pub seq: u64,
}

/// Сигналы, в которые пишет поток.
#[derive(Clone, Copy)]
pub struct Sink {
    pub sensors: RwSignal<Sensors>,
    pub power: RwSignal<PowerState>,
    pub tunables: RwSignal<Vec<Tunable>>,
    pub fans: RwSignal<Vec<Fan>>,
    pub battery: RwSignal<Option<Battery>>,
    pub toggles: RwSignal<Toggles>,
    pub info: RwSignal<SystemInfo>,
    pub rgb: RwSignal<RgbState>,
    pub rgb_frame: RwSignal<Vec<(u16, Rgb)>>,
    /// Слои профиля, как они записаны в контроллере: (профиль, слои).
    pub rgb_effects: RwSignal<Option<(u8, Vec<Effect>)>>,
    pub busy: RwSignal<bool>,
    pub notice: RwSignal<Option<Notice>>,
    /// Служба автоприменения и сохранённые значения режима «Свой».
    pub autoapply: RwSignal<(ServiceState, Saved)>,
    pub cpu: RwSignal<CpuState>,
    pub presets: RwSignal<Presets>,
    pub rules: RwSignal<Vec<Rule>>,
    pub power_source: RwSignal<PowerSource>,
    /// Программы, которые держат видеокарту (опрашиваются на главной).
    pub gpu_apps: RwSignal<Vec<GpuProcess>>,
}

/// Номер уведомления — общий для потока и запущенных из него действий.
static SEQ: AtomicU64 = AtomicU64::new(0);

fn post(sink: &Sink, kind: NoticeKind, text: impl Into<String>) {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    sink.notice.set(Some(Notice { kind, text: text.into(), seq }));
}

pub fn spawn(sink: Sink, simulate: bool, page: Arc<AtomicU8>, rx: Receiver<Job>) {
    std::thread::Builder::new()
        .name("legion-worker".into())
        .spawn(move || {
            let mut w = Worker {
                sink,
                simulate,
                page,
                sampler: Sampler::default(),
                kb: None,
                matrix: KeyMatrix::default(),
                last_gpu: Gpu::Absent,
                last_rgb_try: None,
                source: PowerSource::default(),
            };
            w.run(rx)
        })
        .expect("failed to start the background thread");
}

struct Worker {
    sink: Sink,
    simulate: bool,
    page: Arc<AtomicU8>,
    sampler: Sampler,
    kb: Option<Keyboard>,
    /// Матрица зон не меняется, пока контроллер подключён.
    matrix: KeyMatrix,
    last_gpu: Gpu,
    last_rgb_try: Option<Instant>,
    source: PowerSource,
}

impl Worker {
    fn run(&mut self, rx: Receiver<Job>) {
        self.sink.info.set(device::read_info());
        self.refresh_sysfs();
        self.refresh_autoapply();
        self.refresh_config();
        let mut next_sysfs = Instant::now();
        let mut next_gpu = Instant::now();
        let mut next_status = Instant::now();
        let mut next_frame = Instant::now();
        loop {
            let now = Instant::now();
            let page = self.page.load(Ordering::Relaxed);

            if now >= next_sysfs {
                let gpu = page == page::HOME && now >= next_gpu;
                let on_ac = battery::read_battery().is_none_or(|b| b.ac_online);
                if gpu {
                    next_gpu = now + if on_ac { GPU_POLL_AC } else { GPU_POLL_BATTERY };
                }
                let mut s = self.sampler.read(gpu);
                if !gpu {
                    // Сохраняем прошлое состояние видеокарты до следующего опроса.
                    s.gpu = self.last_gpu.clone();
                }
                self.last_gpu = s.gpu.clone();
                if gpu {
                    // Кто держит карту — только пока она не спит (иначе и так никто).
                    let apps = if matches!(s.gpu, Gpu::Active(_)) { gpu_apps::processes() } else { Vec::new() };
                    self.sink.gpu_apps.set(apps);
                }
                self.sink.sensors.set(s);
                self.refresh_sysfs();
                next_sysfs = now + SYSFS_POLL;
            }

            if self.kb.is_none() && self.last_rgb_try.is_none_or(|t| t.elapsed() >= RGB_RETRY) {
                self.connect_rgb();
            }
            if self.kb.is_some() && now >= next_status {
                self.rgb_status();
                next_status = now + RGB_STATUS;
            }
            if self.kb.is_some() && page == page::LIGHTING && now >= next_frame {
                self.rgb_frame();
                next_frame = now + RGB_FRAME;
            }

            let wait = if page == page::LIGHTING && self.kb.is_some() { RGB_FRAME } else { Duration::from_millis(250) };
            match rx.recv_timeout(wait) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn notify(&mut self, kind: NoticeKind, text: impl Into<String>) {
        post(&self.sink, kind, text);
    }

    fn refresh_config(&mut self) {
        self.sink.presets.set(presets::load());
        self.sink.rules.set(automation::load());
    }

    /// Источник питания; разряд копится, пока ноутбук от батареи.
    fn refresh_source(&mut self, b: Option<&Battery>) {
        let charger = charger::read();
        let src = &mut self.source;
        if charger.connected() {
            *src = PowerSource { charger, ..Default::default() };
        } else {
            if src.charger.connected() {
                // Только что отключили: разряд считается заново.
                src.min_discharge_w = None;
                src.max_discharge_w = None;
                src.on_battery_since = Some(std::time::SystemTime::now());
            }
            if let Some(t) = autoapply::on_battery_since() {
                // Отметка службы точнее: она видит отключение, даже когда окна нет.
                src.on_battery_since = Some(t);
            }
            src.charger = charger;
            if let Some(w) = b.filter(|b| b.status == Status::Discharging && b.power_w > 0.5).map(|b| b.power_w) {
                src.min_discharge_w = Some(src.min_discharge_w.map_or(w, |m| m.min(w)));
                src.max_discharge_w = Some(src.max_discharge_w.map_or(w, |m| m.max(w)));
            }
        }
        self.sink.power_source.set(self.source.clone());
    }

    fn refresh_sysfs(&mut self) {
        self.sink.power.set(power::read_state());
        self.sink.tunables.set(power::read_tunables());
        self.sink.fans.set(fans::read_fans());
        self.sink.cpu.set(CpuState { rapl: rapl::read(), epp: cpufreq::read(), boost: cpufreq::boost() });
        let b = battery::read_battery();
        self.refresh_source(b.as_ref());
        self.sink.battery.set(b);
        self.sink.toggles.set(device::read_toggles());
    }

    fn refresh_autoapply(&mut self) {
        let service = if self.simulate { ServiceState::default() } else { autoapply::service_state() };
        self.sink.autoapply.set((service, autoapply::load()));
    }

    fn sysfs_result(&mut self, r: sysfs::Result<()>, ok: String) {
        match r {
            Ok(()) => self.notify(NoticeKind::Success, ok),
            Err(sysfs::Error::Rejected(..)) if power::read_state().mode != Some(PowerMode::Custom) => {
                self.notify(NoticeKind::Error, "The driver accepts this value only in Custom mode")
            }
            Err(e) => self.notify(NoticeKind::Error, e.to_string()),
        }
        self.refresh_sysfs();
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::SetPowerMode(m) => {
                // Вместе с режимом — вентиляторы EC и лимиты CPU/EPP (см. actions).
                let r = actions::set_power_mode(m);
                self.sysfs_result(r, format!("Power mode: {}", m.label()));
            }
            Job::SetTunable(name, v) => {
                let label = self.label_of(&name);
                let r = power::set_tunable(&name, v);
                if r.is_ok() {
                    autoapply::remember_tunable(&name, v);
                    self.refresh_autoapply();
                }
                self.sysfs_result(r, format!("{label}: {v}"));
            }
            Job::SetFanTarget(i, rpm) => {
                let r = fans::set_target(i, rpm);
                if r.is_ok() {
                    autoapply::remember_fan(i, rpm);
                    self.refresh_autoapply();
                }
                let text = if rpm == 0 { "auto".to_string() } else { format!("{rpm} rpm") };
                self.sysfs_result(r, format!("Fan {i}: {text}"));
            }
            Job::SetChargeType(t) => {
                let r = battery::set_charge_type(t);
                self.sysfs_result(r, format!("Battery mode: {}", t.label()));
            }
            Job::SetToggle(t, on) => {
                let r = device::set_toggle(t, on);
                let state = if on { "on" } else { "off" };
                self.sysfs_result(r, format!("{}: {state}", t.label()));
            }
            Job::RgbProfile(n) => self.rgb_cmd(|kb| kb.set_profile(n), format!("Lighting profile {n}"), Some(n)),
            Job::RgbBrightness(v) => self.rgb_cmd(|kb| kb.set_brightness(v), String::new(), None),
            Job::RgbLogo(on) => self.rgb_cmd(
                |kb| kb.set_logo(on),
                format!("Panel logo backlight {}", if on { "on" } else { "off" }),
                None,
            ),
            Job::RgbLoad(n) => self.load_effects(n),
            Job::RgbWrite(n, effects) => {
                self.sink.busy.set(true);
                self.rgb_cmd(|kb| kb.set_effects(n, &effects), format!("Effects written to profile {n}"), Some(n));
                self.sink.busy.set(false);
            }
            Job::SetAutoApply(on) => {
                match autoapply::set_service(on) {
                    Ok(()) if on => self.notify(NoticeKind::Success, "Auto-apply enabled"),
                    Ok(()) => self.notify(NoticeKind::Success, "Auto-apply disabled"),
                    Err(e) => self.notify(NoticeKind::Error, format!("Auto-apply service: {e}")),
                }
                self.refresh_autoapply();
            }
            Job::RgbReset(n) => {
                self.rgb_cmd(|kb| kb.reset_profile(n), format!("Profile {n} reset to factory"), Some(n))
            }
            Job::SetFlag(Flag::FanMax, on) => {
                let r = actions::set_fan_max(on);
                match r {
                    Ok(_) => self.notify(NoticeKind::Success, format!("Maximum fan speed: {}", if on { "on" } else { "off" })),
                    Err(e) => self.notify(NoticeKind::Error, format!("Maximum fan speed: {e}")),
                }
                self.refresh_autoapply();
                self.refresh_sysfs();
            }
            Job::SetFlag(Flag::Tray, on) => {
                // Значок держит служба — она заметит настройку в течение секунды.
                autoapply::remember_flag(Flag::Tray, on);
                self.notify(NoticeKind::Success, format!("Tray icon: {}", if on { "on" } else { "off" }));
                self.refresh_autoapply();
            }
            Job::SetFlag(flag, on) => {
                autoapply::remember_flag(flag, on);
                let state = if on { "on" } else { "off" };
                let what = match flag {
                    Flag::FanCurve => "Software fan auto",
                    Flag::Rapl => "CPU limits per mode",
                    Flag::Epp => "EPP per mode",
                    Flag::FanMax | Flag::Tray => unreachable!(),
                };
                let mut r = Ok(());
                if let Some(m) = power::read_state().mode {
                    if matches!(flag, Flag::Rapl | Flag::Epp) {
                        r = autoapply::apply_cpu(m).map(|_| ());
                    }
                }
                // Выключили программное авто — вернуть вентиляторы EC.
                if flag == Flag::FanCurve && !on && power::read_state().mode != Some(PowerMode::Custom) {
                    autoapply::release_fans();
                }
                match r {
                    Ok(()) => self.notify(NoticeKind::Success, format!("{what}: {state}")),
                    Err(e) => self.notify(NoticeKind::Error, format!("{what}: {e}")),
                }
                self.refresh_autoapply();
                self.refresh_sysfs();
            }
            Job::SetCpuBoost(on) => {
                // Не через sysfs_result: его подсказка про режим «Свой» тут не к месту.
                match actions::set_cpu_boost(on) {
                    Ok(()) => self.notify(NoticeKind::Success, format!("CPU Boost: {}", if on { "on" } else { "off" })),
                    Err(e) => self.notify(NoticeKind::Error, format!("CPU Boost: {e}")),
                }
                self.refresh_autoapply();
                self.refresh_sysfs();
            }
            Job::SetNotify(kind, on) => {
                autoapply::remember_notify(kind, on);
                self.refresh_autoapply();
            }
            Job::ActivatePreset(name) => {
                match presets::activate(&name) {
                    Ok(_) => self.notify(NoticeKind::Success, format!("Custom Mode preset: {name}")),
                    Err(e) => self.notify(NoticeKind::Error, format!("Preset “{name}”: {e}")),
                }
                self.refresh_config();
                self.refresh_autoapply();
                self.refresh_sysfs();
            }
            Job::SavePreset(name) => {
                match presets::save_current(&name) {
                    Ok(()) => self.notify(NoticeKind::Success, format!("Preset “{}” saved", name.trim())),
                    Err(e) => self.notify(NoticeKind::Error, format!("Preset: {e}")),
                }
                self.refresh_config();
            }
            Job::DeletePreset(name) => {
                match presets::delete(&name) {
                    Ok(()) => self.notify(NoticeKind::Success, format!("Preset “{name}” deleted")),
                    Err(e) => self.notify(NoticeKind::Error, format!("Preset: {e}")),
                }
                self.refresh_config();
            }
            Job::SaveRules(rules) => {
                match automation::store(&rules) {
                    Ok(()) => self.notify(NoticeKind::Success, "Actions saved"),
                    Err(e) => self.notify(NoticeKind::Error, format!("Actions: {e}")),
                }
                self.refresh_config();
            }
            Job::RunRule(name) => self.run_rule(&name),
            Job::RgbExport(n, path) => self.rgb_export(n, &path),
            Job::RgbImport(n, path) => {
                let res = std::fs::read_to_string(actions::expand_home(&path))
                    .map_err(|e| format!("{path}: {e}"))
                    .and_then(|t| profile_file::import(&t));
                match res {
                    Ok(effects) => {
                        let count = effects.len();
                        self.sink.busy.set(true);
                        self.rgb_cmd(
                            |kb| kb.set_effects(n, &effects),
                            format!("{count} effects imported into profile {n}"),
                            Some(n),
                        );
                        self.sink.busy.set(false);
                    }
                    Err(e) => self.notify(NoticeKind::Error, format!("Import: {e}")),
                }
            }
            Job::DeactivateGpu => {
                let list = gpu_apps::processes();
                let n = gpu_apps::terminate(&list);
                let kept = list.iter().filter(|p| !p.killable).count();
                let text = match (n, kept) {
                    (0, 0) => "No apps are using the discrete GPU".to_string(),
                    (n, 0) => format!("Asked {n} apps to close"),
                    (n, k) => format!("Asked {n} apps to close; {k} system processes keep the GPU awake"),
                };
                self.notify(NoticeKind::Success, text);
                std::thread::sleep(Duration::from_millis(500));
                self.sink.gpu_apps.set(gpu_apps::processes());
            }
        }
    }

    /// Быстрое действие из окна. Шаги идут в отдельном потоке (в них бывают
    /// паузы); подсветке поток открывает свой дескриптор контроллера.
    fn run_rule(&mut self, name: &str) {
        let Some(rule) = automation::load().into_iter().find(|r| r.name == name) else {
            return self.notify(NoticeKind::Error, format!("No action named “{name}”"));
        };
        let (sink, simulate) = (self.sink, self.simulate);
        let spawned = std::thread::Builder::new().name("legion-action".into()).spawn(move || {
            let mut ctx = actions::Ctx { kb: None, simulate };
            match actions::run_all(&rule.steps, &mut ctx) {
                Ok(_) => post(&sink, NoticeKind::Success, format!("Action “{}” done", rule.name)),
                Err(e) => post(&sink, NoticeKind::Error, format!("Action “{}”: {e}", rule.name)),
            }
        });
        if spawned.is_err() {
            self.notify(NoticeKind::Error, "Could not start the action");
        }
    }

    fn rgb_export(&mut self, n: u8, path: &str) {
        let Some(kb) = self.kb.as_mut() else {
            return self.notify(NoticeKind::Error, "The lighting controller is not connected");
        };
        let res = kb
            .effects(n)
            .map_err(|e| e.to_string())
            .and_then(|e| std::fs::write(actions::expand_home(path), profile_file::export(&e)).map_err(|e| format!("{path}: {e}")));
        // Чтение слоёв делает профиль активным — перечитать состояние.
        self.rgb_status();
        match res {
            Ok(()) => self.notify(NoticeKind::Success, format!("Profile {n} exported to {path}")),
            Err(e) => self.notify(NoticeKind::Error, format!("Export: {e}")),
        }
    }

    fn label_of(&self, name: &str) -> String {
        power::read_tunables().into_iter().find(|t| t.name == name).map(|t| t.label).unwrap_or_else(|| name.into())
    }

    // --- подсветка ---------------------------------------------------------

    fn connect_rgb(&mut self) {
        self.last_rgb_try = Some(Instant::now());
        let res = Keyboard::open(self.simulate).and_then(|mut kb| {
            let m = kb.key_matrix()?;
            Ok((kb, m))
        });
        match res {
            Ok((kb, matrix)) => {
                let name = kb.name();
                self.kb = Some(kb);
                self.matrix = matrix.clone();
                self.sink.rgb.set(RgbState { link: RgbLink::Ready, name, matrix, ..Default::default() });
                self.rgb_status();
            }
            Err(e) => self.rgb_fail(e),
        }
    }

    fn rgb_fail(&mut self, e: KbError) {
        self.kb = None;
        let link = match e {
            KbError::NotFound => RgbLink::NotFound,
            KbError::Permission(p) => RgbLink::NoAccess(p),
            e => RgbLink::Error(e.to_string()),
        };
        self.sink.rgb.set(RgbState { link, ..Default::default() });
    }

    /// Профиль, яркость и логотип могут поменяться с клавиатуры (Fn+Пробел).
    fn rgb_status(&mut self) {
        let Some(kb) = self.kb.as_mut() else { return };
        let res = (|| Ok::<_, KbError>((kb.profile()?, kb.brightness()?, kb.logo()?)))();
        match res {
            Ok((profile, brightness, logo)) => {
                let name = kb.name();
                let matrix = self.matrix.clone();
                let st = RgbState { link: RgbLink::Ready, name, profile, brightness, logo, matrix };
                self.sink.rgb.set(st);
            }
            Err(e) => self.rgb_fail(e),
        }
    }

    fn rgb_frame(&mut self) {
        let Some(kb) = self.kb.as_mut() else { return };
        match kb.state() {
            Ok(Some(frame)) => self.sink.rgb_frame.set(frame),
            Ok(None) => {}
            Err(e) => self.rgb_fail(e),
        }
    }

    fn load_effects(&mut self, n: u8) {
        let Some(kb) = self.kb.as_mut() else { return };
        match kb.effects(n) {
            Ok(e) => self.sink.rgb_effects.set_always(Some((n, e))),
            Err(e) => {
                self.notify(NoticeKind::Error, format!("Could not read profile {n}: {e}"));
            }
        }
    }

    fn rgb_cmd(&mut self, f: impl FnOnce(&mut Keyboard) -> crate::spectrum::device::Result<()>, ok: String, reload: Option<u8>) {
        let Some(kb) = self.kb.as_mut() else {
            return self.notify(NoticeKind::Error, "The lighting controller is not connected");
        };
        match f(kb) {
            Ok(()) => {
                if !ok.is_empty() {
                    self.notify(NoticeKind::Success, ok);
                }
                std::thread::sleep(Duration::from_millis(60));
                self.rgb_status();
                if let Some(n) = reload {
                    self.load_effects(n);
                }
            }
            Err(e) => {
                self.notify(NoticeKind::Error, format!("Lighting: {e}"));
                if matches!(e, KbError::NotFound | KbError::Io(_)) {
                    self.rgb_fail(e);
                }
            }
        }
    }
}
