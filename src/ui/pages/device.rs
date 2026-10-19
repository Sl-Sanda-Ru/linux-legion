//! Устройство: аппаратные переключатели и сведения о системе.

use crate::hw::device::Toggle as Switch;
use crate::autoapply::Flag;
use crate::notify;
use crate::ui::app::AppCtx;
use crate::ui::icons;
use crate::ui::widgets::{access_banner, card, card_with, kv, page_header, reactive, reactive_box, setting_row};
use crate::worker::Job;
use syngui::prelude::*;

fn toggle_icon(t: Switch) -> &'static str {
    match t {
        Switch::FnLock => icons::LOCK,
        Switch::Camera => icons::VIDEOCAM,
        Switch::UsbCharging => icons::USB,
    }
}

pub fn view(ctx: AppCtx) -> impl Widget {
    let (c_t, c_i, c_n) = (ctx.clone(), ctx.clone(), ctx);
    Column::new()
        .gap(18.0)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .child(page_header("Device", "Hardware switches and laptop information."))
        .child(reactive_box(move || {
            let t = c_t.sink.toggles.get();
            if t.items.is_empty() {
                return Box::new(card("Switches", "", Text::new("The ideapad_acpi driver was not found.").class("muted")));
            }
            let mut col = Column::new().gap(18.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            if t.items.iter().any(|i| !i.2) {
                col = col.child(access_banner());
            }
            let mut rows = Column::new().gap(20.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (kind, on, writable) in t.items {
                let c = c_t.clone();
                rows = rows.child(
                    Row::new()
                        .gap(14.0)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .child(
                            DecoratedBox::new()
                                .child(Center::new().child(Icon::new(toggle_icon(kind)).class("row-icon")))
                                .class("row-icon-badge"),
                        )
                        .child(
                            DecoratedBox::new()
                                .child(setting_row(
                                    kind.label(),
                                    kind.hint(),
                                    Box::new(
                                        Toggle::new()
                                            .on(on)
                                            .on_change(move |v| c.send(Job::SetToggle(kind, v)))
                                            .class(if writable { "" } else { "toggle-off" }),
                                    ),
                                ))
                                .class("grow"),
                        ),
                );
            }
            Box::new(col.child(card("Switches", "", rows)))
        }))
        .child(notifications(c_n))
        .child(card(
            "System information",
            "",
            reactive(move || {
                let i = c_i.sink.info.get();
                let rgb = c_i.sink.rgb.get();
                Column::new()
                    .gap(2.0)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .child(kv("Model", format!("{} {}", i.vendor, i.model)))
                    .child(kv("Machine type", i.machine_type.clone()))
                    .child(kv("BIOS", format!("{} ({})", i.bios, i.bios_date)))
                    .child(kv("CPU", format!("{} · {} threads", i.cpu, i.cpu_threads)))
                    .child(kv("GPU", if i.gpu.is_empty() { "—".into() } else { i.gpu.clone() }))
                    .child(kv("Memory", format!("{:.1} GB", i.memory_gb)))
                    .child(kv("Lighting controller", if rgb.name.is_empty() { "—".into() } else { rgb.name.clone() }))
                    .child(kv("System", format!("{} · kernel {}", i.os, i.kernel)))
            }),
        ))
        .class("page")
}

/// Уведомления рабочего стола, которые показывает служба.
fn notifications(ctx: AppCtx) -> impl Widget {
    card_with(
        icons::NOTIFICATIONS,
        "Tray and notifications",
        || Text::new("").class("card-hint"),
        reactive(move || {
            let (svc, saved) = ctx.sink.autoapply.get();
            let mut col = Column::new().gap(16.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            let note = if svc.enabled && svc.active {
                "The linux-legion service shows these on the desktop."
            } else {
                "The linux-legion service shows these on the desktop — turn it on on the Performance page."
            };
            col = col.child(Text::new(note).class("row-desc"));
            let c_tray = ctx.clone();
            col = col.child(setting_row(
                "Tray icon",
                "Power mode, presets, CPU Boost, fans, battery mode, keyboard profiles and quick actions in \
                 one menu. In GNOME it needs the AppIndicator extension.",
                Box::new(Toggle::new().on(saved.tray).on_change(move |v| c_tray.send(Job::SetFlag(Flag::Tray, v)))),
            ));
            for kind in notify::Kind::ALL {
                let c = ctx.clone();
                col = col.child(setting_row(
                    kind.label(),
                    kind.hint(),
                    Box::new(Toggle::new().on(saved.notifies(kind)).on_change(move |v| c.send(Job::SetNotify(kind, v)))),
                ));
            }
            col
        }),
    )
}
