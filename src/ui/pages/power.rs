//! Производительность: режимы питания, лимиты мощности и вентиляторы режима «Свой».

use super::home::mode_icon;
use crate::autoapply::Flag;
use crate::hw::power::PowerMode;
use crate::ui::app::AppCtx;
use crate::ui::icons;
use crate::ui::widgets::{
    access_banner, bar, card, card_with, kv, labeled, page_header, reactive, reactive_box, setting_row,
};
use crate::worker::Job;
use syngui::prelude::*;
use syngui::widgets::*;
use syngui::CursorIcon;

pub fn view(ctx: AppCtx) -> impl Widget {
    let c = ctx.clone();
    Column::new()
        .gap(18.0)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .child(page_header(
            "Performance",
            "The power mode changes the power limits and how the fans behave. On the keyboard: Fn+Q.",
        ))
        .child(reactive_box(move || {
            let st = c.sink.power.get();
            if st.mode.is_some() && !st.writable {
                Box::new(access_banner())
            } else {
                crate::ui::widgets::nothing()
            }
        }))
        .child(card("Power mode", "", modes(ctx.clone())))
        .child(custom_notice(ctx.clone()))
        .child(tunables(ctx.clone()))
        .child(presets(ctx.clone()))
        .child(cpu(ctx.clone()))
        .child(fans(ctx.clone()))
        .child(autoapply(ctx))
        .class("page")
}

fn modes(ctx: AppCtx) -> impl Widget {
    reactive(move || {
        let st = ctx.sink.power.get();
        let modes = if st.choices.is_empty() { PowerMode::ALL.to_vec() } else { st.choices.clone() };
        let mut row = Row::new().gap(12.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
        for m in modes {
            let c = ctx.clone();
            let on = st.mode == Some(m);
            let cls = if on { format!("mode-tile mode-tile-on tile-{}", m.tone()) } else { "mode-tile".to_string() };
            row = row.child(
                GestureDetector::new()
                    .on_click(move || c.send(Job::SetPowerMode(m)))
                    .cursor(CursorIcon::Pointer)
                    .child(
                        DecoratedBox::new()
                            .child(
                                Column::new()
                                    .gap(10.0)
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .child(
                                        DecoratedBox::new()
                                            .child(Center::new().child(Icon::new(mode_icon(m)).class("mode-icon")))
                                            .class(&format!("mode-icon-badge badge-{}", m.tone())),
                                    )
                                    .child(Text::new(m.label()).class("mode-label"))
                                    .child(Text::new(m.hint()).class("mode-hint")),
                            )
                            .class(&cls),
                    )
                    .class("grow"),
            );
        }
        row
    })
}

/// Подсказка: лимиты и вентиляторы меняются только в режиме «Свой».
fn custom_notice(ctx: AppCtx) -> impl Widget {
    reactive_box(move || {
        let st = ctx.sink.power.get();
        if st.mode == Some(PowerMode::Custom) || !st.choices.contains(&PowerMode::Custom) {
            return crate::ui::widgets::nothing();
        }
        let c = ctx.clone();
        Box::new(
            DecoratedBox::new()
                .child(
                    Row::new()
                        .gap(14.0)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .child(Icon::new(icons::INFO).class("banner-icon-info"))
                        .child(
                            Text::new(
                                "The firmware allows the power limits and fan speeds to be changed only in Custom mode. \
                                 In the other modes they are shown for reference.",
                            )
                            .class("row-desc grow"),
                        )
                        .child(
                            Button::new("Switch to Custom")
                                .icon(icons::MODE_CUSTOM)
                                .on_click(move || c.send(Job::SetPowerMode(PowerMode::Custom)))
                                .class("btn-primary"),
                        ),
                )
                .class("banner banner-info"),
        )
    })
}

fn tunables(ctx: AppCtx) -> impl Widget {
    let (c_apply, c_reset) = (ctx.clone(), ctx.clone());
    let actions = move || {
        let custom = c_apply.power_mode() == Some(PowerMode::Custom);
        let dirty = !c_apply.tun_edit.get().is_empty();
        let (ca, cr, cm) = (c_apply.clone(), c_reset.clone(), c_reset.clone());
        Row::new()
            .gap(8.0)
            .child(
                Button::new("Maximum")
                    .icon(icons::BOLT)
                    .disabled(!custom)
                    .on_click(move || {
                        // Атрибуты с диапазоном 0–0 только читаются: их `max`
                        // равен нулю, и запись обнулила бы лимит.
                        let map = cm
                            .sink
                            .tunables
                            .get_untracked()
                            .into_iter()
                            .filter(|t| t.writable && t.min < t.max)
                            .map(|t| (t.name, t.max))
                            .collect();
                        cm.tun_edit.set(map);
                    })
                    .class("btn-ghost"),
            )
            .child(
                Button::new("Defaults")
                    .icon(icons::RESTART)
                    .disabled(!custom)
                    .on_click(move || {
                        let map = cr
                            .sink
                            .tunables
                            .get_untracked()
                            .into_iter()
                            .filter(|t| t.writable && t.min < t.max)
                            .filter_map(|t| t.default.map(|d| (t.name, d)))
                            .collect();
                        cr.tun_edit.set(map);
                    })
                    .class("btn-ghost"),
            )
            .child(
                Button::new("Apply")
                    .icon(icons::CHECK)
                    .disabled(!custom || !dirty)
                    .on_click(move || {
                        for (name, v) in ca.tun_edit.get_untracked() {
                            ca.send(Job::SetTunable(name, v));
                        }
                        ca.tun_edit.set(Default::default());
                    })
                    .class("btn-primary"),
            )
    };
    card_with(
        icons::BOLT,
        "Power limits",
        actions,
        reactive_box(move || {
            let list = ctx.sink.tunables.get();
            if list.is_empty() {
                return Box::new(
                    Text::new("The lenovo-wmi-other driver exposes no tunable limits on this model.").class("muted"),
                );
            }
            let custom = ctx.power_mode() == Some(PowerMode::Custom);
            let edit = ctx.tun_edit.get();
            let mut col = Column::new().gap(22.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            for t in list {
                let v = edit.get(&t.name).copied().unwrap_or(t.value);
                let changed = v != t.value;
                let value = if changed { format!("{} → {v} {}", t.value, t.unit) } else { format!("{v} {}", t.unit) };
                // Прошивка отдаёт часть атрибутов с диапазоном 0–0: значение
                // только читается, ползунок для него бессмыслен.
                let fixed = t.min >= t.max;
                let mut row =
                    Column::new().gap(8.0).cross_axis_alignment(CrossAxisAlignment::Stretch).child(labeled(&t.label, value));
                if !fixed {
                    let c = ctx.clone();
                    let name = t.name.clone();
                    row = row.child(
                        Slider::new()
                            .value(v as f32)
                            .range(t.min as f32, t.max as f32)
                            .step(t.step as f32)
                            .disabled(!custom || !t.writable)
                            .on_change(move |x| {
                                let x = x.round() as i64;
                                let mut m = c.tun_edit.get_untracked();
                                m.insert(name.clone(), x);
                                c.tun_edit.set(m);
                            })
                            .class("wide-slider"),
                    );
                }
                let scale = if fixed {
                    "fixed by the firmware".to_string()
                } else {
                    format!("{}–{} {}", t.min, t.max, t.unit)
                };
                col = col.child(row.child(
                    Row::new()
                        .gap(8.0)
                        .child(Text::new(t.hint.clone()).class("row-desc grow"))
                        .child(Text::new(scale).class("scale-label")),
                ));
            }
            Box::new(col)
        }),
    )
}

fn fans(ctx: AppCtx) -> impl Widget {
    let c_apply = ctx.clone();
    let actions = move || {
        let custom = c_apply.power_mode() == Some(PowerMode::Custom);
        let dirty = !c_apply.fan_edit.get().is_empty();
        let (ca, cr) = (c_apply.clone(), c_apply.clone());
        Row::new()
            .gap(8.0)
            .child(
                Button::new("All auto")
                    .icon(icons::AUTO)
                    .disabled(!custom)
                    .on_click(move || {
                        let map = cr.sink.fans.get_untracked().iter().map(|f| (f.index, 0)).collect();
                        cr.fan_edit.set(map);
                    })
                    .class("btn-ghost"),
            )
            .child(
                Button::new("Apply")
                    .icon(icons::CHECK)
                    .disabled(!custom || !dirty)
                    .on_click(move || {
                        for (i, rpm) in ca.fan_edit.get_untracked() {
                            ca.send(Job::SetFanTarget(i, rpm));
                        }
                        ca.fan_edit.set(Default::default());
                    })
                    .class("btn-primary"),
            )
    };
    card_with(
        icons::FAN,
        "Fans",
        actions,
        reactive_box(move || {
            let list = ctx.sink.fans.get();
            if list.is_empty() {
                return Box::new(Text::new("No fans found (the lenovo-wmi-other driver is required).").class("muted"));
            }
            let custom = ctx.power_mode() == Some(PowerMode::Custom);
            let edit = ctx.fan_edit.get();
            let (svc, saved) = ctx.sink.autoapply.get();
            let mut col = Column::new().gap(22.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            let c = ctx.clone();
            let desc = if saved.fan_curve && !(svc.enabled && svc.active) {
                "Enabled, but the service is not running — turn it on in the Service section below."
            } else if saved.fan_curve {
                "The service drives the fans by CPU and GPU temperature in every mode; in Custom \
                 the speeds set here are kept as they are (above 95 °C — never below the curve)."
            } else {
                "Once a manual speed has been written, the Legion Pro 7 Gen 10 EC never goes back to its own \
                 curve: not on auto, not on a mode switch, not after sleep or a reboot — only after an EC reset \
                 (shut down, unplug the charger, hold the power button for 30 s). If you have set speeds by \
                 hand, turn this on and the fans will follow temperature again."
            };
            let c_max = ctx.clone();
            col = col.child(setting_row(
                "Maximum fan speed",
                if saved.fan_max {
                    "All fans run at their maximum rated speed. Turn off to go back to the software fan curve."
                } else {
                    "Bypass the fan curves and run every fan at its maximum rated speed, in any power mode. \
                     Turns on software auto as well, so the fans can return to the curve afterwards."
                },
                Box::new(
                    Toggle::new()
                        .on(saved.fan_max)
                        .on_change(move |v| c_max.send(Job::SetFlag(Flag::FanMax, v)))
                        .class(if list.iter().any(|f| f.writable) { "" } else { "toggle-off" }),
                ),
            ));
            col = col.child(setting_row(
                "Software auto (by temperature)",
                desc,
                Box::new(Toggle::new().on(saved.fan_curve).on_change(move |v| c.send(Job::SetFlag(Flag::FanCurve, v)))),
            ));
            for f in list {
                let target = edit.get(&f.index).copied().unwrap_or(f.target);
                let auto = target == 0;
                let changed = edit.contains_key(&f.index) && target != f.target;
                let (c_t, c_s) = (ctx.clone(), ctx.clone());
                let (idx, min, step) = (f.index, f.min, f.step);
                let frac = if f.max > 0 { f.rpm as f32 / f.max as f32 } else { 0.0 };
                let set = move |c: &AppCtx, v: u32| {
                    let mut m = c.fan_edit.get_untracked();
                    m.insert(idx, v);
                    c.fan_edit.set(m);
                };
                let target_text = if auto { "auto".to_string() } else { format!("{target} rpm") };
                col = col.child(
                    Column::new()
                        .gap(10.0)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .child(
                            Row::new()
                                .gap(12.0)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .child(Icon::new(icons::FAN).class("fan-icon"))
                                .child(
                                    Column::new()
                                        .gap(2.0)
                                        .child(Text::new(format!("{} · №{}", f.label, f.index)).class("row-title"))
                                        .child(
                                            Text::new(format!(
                                                "Now {} rpm · target: {}{}",
                                                f.rpm,
                                                target_text,
                                                if changed { " (not applied)" } else { "" }
                                            ))
                                            .class(if changed { "row-desc changed" } else { "row-desc" }),
                                        )
                                        .class("grow"),
                                )
                                .child(Text::new("Auto").class("field-label"))
                                .child(
                                    Toggle::new()
                                        .on(auto)
                                        .on_change(move |on| {
                                            let v = if on { 0 } else { min };
                                            set(&c_t, v)
                                        })
                                        .class(if custom && f.writable { "" } else { "toggle-off" }),
                                ),
                        )
                        .child(bar(frac, "fan"))
                        .child(
                            Slider::new()
                                .value(if auto { f.min as f32 } else { target as f32 })
                                .range(f.min as f32, f.max as f32)
                                .step(f.step as f32)
                                .disabled(!custom || auto || !f.writable)
                                .on_change(move |x| {
                                    let v = ((x / step as f32).round() as u32) * step;
                                    set(&c_s, v)
                                })
                                .class("wide-slider"),
                        )
                        .child(
                            Row::new()
                                .child(Text::new(format!("{} rpm", f.min)).class("scale-label"))
                                .child(DecoratedBox::new().class("grow"))
                                .child(Text::new(format!("{} rpm", f.max)).class("scale-label")),
                        ),
                );
            }
            Box::new(col)
        }),
    )
}

/// Служба: автоприменение «Своего» после перезагрузки и сна, лимиты CPU, вентиляторы.
fn autoapply(ctx: AppCtx) -> impl Widget {
    card_with(
        icons::RESTART,
        "Service",
        || Text::new("").class("card-hint"),
        reactive_box(move || {
            let (svc, saved) = ctx.sink.autoapply.get();
            let tunables = ctx.sink.tunables.get();
            let fans = ctx.sink.fans.get();
            let mut col = Column::new().gap(16.0).cross_axis_alignment(CrossAxisAlignment::Stretch);

            let c = ctx.clone();
            let desc = if !svc.installed {
                "The service is installed together with the package. When building from source, copy \
                 packaging/linux-legion-autoapply.service into ~/.config/systemd/user/."
            } else if svc.enabled && svc.active {
                "The service is running: it restores the Custom values at login, whenever Custom is switched \
                 on and after resume, keeps the CPU limits and EPP in line with the mode, and drives the fans \
                 by temperature."
            } else if svc.enabled {
                "The service is enabled but is not running right now — check journalctl --user -u linux-legion-autoapply."
            } else {
                "The firmware forgets the limits and fan speeds after a reboot, and nothing but the service \
                 ever writes the CPU limits into RAPL. The service does it by itself (linux-legion --daemon)."
            };
            col = col.child(setting_row(
                "linux-legion service",
                desc,
                Box::new(
                    Toggle::new()
                        .on(svc.enabled)
                        .on_change(move |v| c.send(Job::SetAutoApply(v)))
                        .class(if svc.installed { "" } else { "toggle-off" }),
                ),
            ));

            let mut items: Vec<String> = saved
                .tunables
                .iter()
                .map(|(name, v)| {
                    let t = tunables.iter().find(|t| &t.name == name);
                    let label = t.map_or(name.clone(), |t| t.label.clone());
                    format!("{label}: {v} {}", t.map_or("", |t| t.unit))
                })
                .collect();
            items.extend(saved.fans.iter().map(|(i, rpm)| {
                let label = fans.iter().find(|f| f.index == *i).map_or("Fan", |f| f.label);
                let v = if *rpm == 0 { "auto".to_string() } else { format!("{rpm} rpm") };
                format!("Fan “{label}”: {v}")
            }));
            if items.is_empty() {
                col = col.child(
                    Text::new(
                        "Nothing has been saved yet: values are remembered by the Apply buttons in Custom mode.",
                    )
                    .class("muted"),
                );
            } else {
                let mut list = Column::new().gap(2.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
                for it in items {
                    let (k, v) = it.rsplit_once(": ").unwrap_or((it.as_str(), ""));
                    list = list.child(crate::ui::widgets::kv(k, v.to_string()));
                }
                col = col.child(Text::new("Saved values").class("field-label")).child(list);
            }
            Box::new(col)
        }),
    )
}

/// Лимиты CPU (RAPL) и EPP по режиму — то, что в Windows делает Legion Space.
fn cpu(ctx: AppCtx) -> impl Widget {
    card_with(
        icons::MEMORY,
        "CPU",
        || Text::new("").class("card-hint"),
        reactive_box(move || {
            let st = ctx.sink.cpu.get();
            let (_, saved) = ctx.sink.autoapply.get();
            let power = ctx.sink.power.get();
            if !st.rapl.present() && st.epp.current.is_none() && st.boost.is_none() {
                return Box::new(Text::new("RAPL (powercap) and EPP (cpufreq) are unavailable.").class("muted"));
            }
            let mut col = Column::new().gap(16.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            if let Some(b) = st.boost {
                let c = ctx.clone();
                let desc = if !b.writable {
                    "No permission to change it — update the package's udev rule (wheel or sudo group)."
                } else if b.on {
                    "The CPU may run above its base clock (Turbo Boost) in every power mode, up to the \
                     power limits below. The service keeps this choice after a reboot."
                } else {
                    "Off: the CPU stays at its base clock in every mode — cooler and quieter, but much \
                     slower under load. The service keeps this choice after a reboot."
                };
                col = col.child(setting_row(
                    "CPU Boost",
                    desc,
                    Box::new(
                        Toggle::new()
                            .on(b.on)
                            .on_change(move |v| c.send(Job::SetCpuBoost(v)))
                            .class(if b.writable { "" } else { "toggle-off" }),
                    ),
                ));
            }
            let fmt = |l: Option<crate::hw::rapl::Limits>| match l {
                Some(l) => format!("{} / {} W", l.pl1, l.pl2),
                None => "—".to_string(),
            };
            if st.rapl.present() {
                let c = ctx.clone();
                let desc = if !st.rapl.writable {
                    "No permission to write to powercap — update the package's udev rule (wheel or sudo group)."
                } else {
                    "The BIOS writes 30/30 W into RAPL MMIO at boot and never touches it again: without this \
                     the CPU is stuck at 30 W in every mode. The service keeps the mode's factory limits in \
                     RAPL (MSR and MMIO), and in Custom the limits set above."
                };
                col = col.child(setting_row(
                    "CPU limits per mode (RAPL)",
                    desc,
                    Box::new(
                        Toggle::new()
                            .on(saved.rapl)
                            .on_change(move |v| c.send(Job::SetFlag(Flag::Rapl, v)))
                            .class(if st.rapl.writable { "" } else { "toggle-off" }),
                    ),
                ));
                let mut list = Column::new().gap(2.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
                let modes = if power.choices.is_empty() { PowerMode::ALL.to_vec() } else { power.choices.clone() };
                for m in modes {
                    let v = saved.limits_for(m);
                    let mark = if power.mode == Some(m) { " ●" } else { "" };
                    list = list.child(kv(&format!("{}{mark}", m.label()), fmt(v)));
                }
                list = list.child(kv("Now in RAPL: MSR", fmt(st.rapl.msr)));
                list = list.child(kv("Now in RAPL: MMIO", fmt(st.rapl.mmio)));
                col = col.child(Text::new("PL1 / PL2 per mode (your own: rapl.<mode>=PL1/PL2 in custom.conf)").class("field-label")).child(list);
            }
            if let Some(cur) = st.epp.current.clone() {
                let c = ctx.clone();
                let desc = if !st.epp.writable {
                    "No permission to write EPP — update the package's udev rule (wheel or sudo group)."
                } else {
                    "performance in Performance/Extreme/Custom, balance_performance in Balanced, \
                     balance_power in Quiet. Overrides tuned and power-profiles-daemon. Without performance \
                     the CPU does not reach the top turbo steps."
                };
                let want = power.mode.map(|m| m.epp()).unwrap_or("—");
                col = col.child(setting_row(
                    "EPP per mode",
                    desc,
                    Box::new(
                        Toggle::new()
                            .on(saved.epp)
                            .on_change(move |v| c.send(Job::SetFlag(Flag::Epp, v)))
                            .class(if st.epp.writable { "" } else { "toggle-off" }),
                    ),
                ));
                let now = if st.epp.pinned() {
                    format!("{cur} — governor performance, EPP pinned to performance")
                } else {
                    format!("{cur} (for the mode: {want})")
                };
                col = col.child(kv("EPP now", now));
            }
            Box::new(col)
        }),
    )
}

/// Пресеты «Своего» — как Custom Mode presets в Legion Toolkit.
fn presets(ctx: AppCtx) -> impl Widget {
    let c_dialog = ctx.clone();
    Column::new()
        .gap(0.0)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .child(card_with(
            icons::BOOKMARK,
            "Custom Mode presets",
            || Text::new("").class("card-hint"),
            reactive_box(move || {
                let all = ctx.sink.presets.get();
                let custom = ctx.power_mode() == Some(PowerMode::Custom);
                let mut col = Column::new().gap(14.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
                col = col.child(
                    Text::new(
                        "Save the current power limits and fan speeds under a name and switch between them. The \
                         active preset becomes the Custom values the service restores; it is applied at once in Custom.",
                    )
                    .class("row-desc"),
                );
                if all.list.is_empty() {
                    col = col.child(Text::new("No presets yet.").class("muted"));
                } else {
                    let mut row = Row::new().gap(8.0).cross_axis_alignment(CrossAxisAlignment::Center);
                    for p in &all.list {
                        let c = ctx.clone();
                        let name = p.name.clone();
                        let on = all.active.as_deref() == Some(p.name.as_str());
                        row = row.child(
                            Button::new(p.name.clone())
                                .icon(if on { icons::CHECK } else { icons::BOOKMARK })
                                .on_click(move || c.send(Job::ActivatePreset(name.clone())))
                                .class(if on { "profile-btn profile-btn-on" } else { "profile-btn" }),
                        );
                    }
                    if all.active.is_some() {
                        let c = ctx.clone();
                        row = row.child(DecoratedBox::new().class("grow")).child(
                            Button::new("Delete")
                                .icon(icons::DELETE)
                                .on_click(move || c.preset_delete_open.set(true))
                                .class("btn-ghost"),
                        );
                    }
                    col = col.child(row);
                }
                let (c_name, c_save, c_submit) = (ctx.clone(), ctx.clone(), ctx.clone());
                col = col.child(
                    Row::new()
                        .gap(8.0)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .child(
                            TextField::new()
                                .text(ctx.preset_name.get_untracked())
                                .placeholder("Preset name")
                                .width(260.0)
                                .on_change(move |t| c_name.preset_name.set(t.to_string()))
                                .on_submit(move |t| {
                                    if custom {
                                        c_submit.send(Job::SavePreset(t.to_string()))
                                    }
                                }),
                        )
                        .child(
                            Button::new("Save current values")
                                .icon(icons::SAVE)
                                .disabled(!custom)
                                .on_click(move || c_save.send(Job::SavePreset(c_save.preset_name.get_untracked())))
                                .class("btn-ghost"),
                        ),
                );
                if !custom {
                    col = col.child(Text::new("Presets are saved from Custom mode.").class("scale-label"));
                }
                Box::new(col)
            }),
        ))
        .child({
            let c = c_dialog.clone();
            ConfirmDialog::new("Delete preset", "The active preset will be deleted.", c_dialog.preset_delete_open, move |yes| {
                if let Some(name) = c.sink.presets.get_untracked().active.filter(|_| yes) {
                    c.send(Job::DeletePreset(name));
                }
            })
        })
}
