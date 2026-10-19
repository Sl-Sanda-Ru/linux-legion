//! Батарея: режим зарядки, состояние и износ аккумулятора.

use crate::hw::battery::{ChargeType, Status};
use crate::ui::app::AppCtx;
use crate::ui::icons;
use crate::ui::widgets::{access_banner, bar, card, kv, nothing, page_header, reactive_box, ring};
use crate::worker::Job;
use syngui::prelude::*;
use syngui::widgets::*;
use syngui::CursorIcon;

fn charge_icon(t: ChargeType) -> &'static str {
    match t {
        ChargeType::Standard => icons::BATTERY_FULL,
        ChargeType::Fast => icons::BOLT,
        ChargeType::LongLife => icons::BATTERY_SAVER,
    }
}

pub fn view(ctx: AppCtx) -> impl Widget {
    Column::new()
        .gap(18.0)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .child(page_header("Battery", "Charging mode and battery health."))
        .child(reactive_box(move || {
            let Some(b) = ctx.sink.battery.get() else {
                return Box::new(Text::new("No battery found.").class("muted"));
            };
            let health = b.health();
            let src = ctx.sink.power_source.get();
            let time = match (b.status, b.minutes_left()) {
                (Status::Discharging, Some(m)) => format!("{} h {:02} min until empty", m / 60, m % 60),
                (Status::Charging, Some(m)) => format!("{} h {:02} min until full", m / 60, m % 60),
                _ => String::new(),
            };
            let pct_color = match b.percent {
                p if p <= 15 => Color::from_hex("#f43f5e"),
                p if p <= 35 => Color::from_hex("#fbbf24"),
                _ => Color::from_hex("#34d399"),
            };

            let status = Row::new()
                .gap(28.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .child(ring(b.percent as f32 / 100.0, pct_color, 170.0, format!("{}%", b.percent), b.status.label()))
                .child(
                    Column::new()
                        .gap(10.0)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .child(
                            Row::new()
                                .gap(10.0)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .child(Icon::new(if b.ac_online { icons::PLUG } else { icons::BATTERY_FULL }).class("card-icon"))
                                .child(
                                    Text::new(if b.ac_online { "Plugged in" } else { "Running on battery" })
                                        .class("big-line"),
                                ),
                        )
                        .child(if time.is_empty() { nothing() } else { Box::new(Text::new(time).class("row-desc")) })
                        .child(kv(
                            if b.status == Status::Charging { "Charge rate" } else { "Discharge rate" },
                            if b.power_w > 0.1 { format!("{:.1} W", b.power_w) } else { "—".into() },
                        ))
                        .child(kv("Energy", format!("{:.1} of {:.1} Wh", b.energy_now_wh, b.energy_full_wh)))
                        .child(kv("Voltage", format!("{:.2} V", b.voltage_v)))
                        .child(kv("Power source", src.charger.label()))
                        .child(match src.on_battery_since {
                            Some(t) if !src.charger.connected() => Box::new(kv("On battery since", since(t))) as Box<dyn Widget>,
                            _ => nothing(),
                        })
                        .child(match (src.min_discharge_w, src.max_discharge_w) {
                            (Some(lo), Some(hi)) if !src.charger.connected() => {
                                Box::new(kv("Min / max discharge rate", format!("{lo:.1} / {hi:.1} W"))) as Box<dyn Widget>
                            }
                            _ => nothing(),
                        })
                        .class("grow"),
                );

            let mut modes = Row::new().gap(12.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            for t in ChargeType::ALL {
                if !b.charge_types.contains(&t) {
                    continue;
                }
                let on = b.charge_type == Some(t);
                let c = ctx.clone();
                modes = modes.child(
                    GestureDetector::new()
                        .on_click(move || c.send(Job::SetChargeType(t)))
                        .cursor(CursorIcon::Pointer)
                        .child(
                            DecoratedBox::new()
                                .child(
                                    Column::new()
                                        .gap(10.0)
                                        .child(
                                            DecoratedBox::new()
                                                .child(Center::new().child(Icon::new(charge_icon(t)).class("mode-icon")))
                                                .class(if on { "mode-icon-badge badge-on" } else { "mode-icon-badge" }),
                                        )
                                        .child(Text::new(t.label()).class("mode-label"))
                                        .child(Text::new(t.hint()).class("mode-hint")),
                                )
                                .class(if on { "mode-tile mode-tile-on tile-accent" } else { "mode-tile" }),
                        )
                        .class("grow"),
                );
            }

            let mut col = Column::new().gap(18.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            if src.charger.low_wattage() {
                col = col.child(
                    DecoratedBox::new()
                        .child(
                            Row::new()
                                .gap(14.0)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .child(Icon::new(icons::WARNING).class("banner-icon"))
                                .child(
                                    Column::new()
                                        .gap(4.0)
                                        .child(Text::new("Low wattage charger connected").class("row-title"))
                                        .child(
                                            Text::new(
                                                "The laptop is powered over USB-C only. Under load it can still drain \
                                                 the battery, and the heavier power modes may be limited.",
                                            )
                                            .class("row-desc"),
                                        )
                                        .class("grow"),
                                ),
                        )
                        .class("banner"),
                );
            }
            if !b.charge_types.is_empty() && !b.charge_type_writable {
                col = col.child(access_banner());
            }
            col = col.child(card("Status", "", status));
            if !b.charge_types.is_empty() {
                col = col.child(card("Battery Mode", "Choose how the battery is charged — the same modes as Vantage.", modes));
            }

            let mut health_col = Column::new().gap(10.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            if let Some(h) = health {
                health_col = health_col
                    .child(
                        Row::new()
                            .gap(8.0)
                            .child(Text::new("Capacity remaining").class("stat-label"))
                            .child(DecoratedBox::new().class("grow"))
                            .child(Text::new(format!("{h:.0} %")).class("stat-value")),
                    )
                    .child(bar(h / 100.0, if h > 80.0 { "ok" } else if h > 60.0 { "warn" } else { "bad" }));
            }
            health_col = health_col
                .child(kv("Full charge capacity", format!("{:.1} Wh", b.energy_full_wh)))
                .child(kv("Design capacity", format!("{:.1} Wh", b.energy_design_wh)))
                .child(kv("Cycle count", b.cycles.map_or("—".into(), |c| c.to_string())))
                .child(kv("Model", format!("{} {}", b.manufacturer, b.model).trim().to_string()));
            col = col.child(card("Battery health", "", health_col));
            Box::new(col)
        }))
        .class("page")
}

/// «14:05 · 1 h 20 min ago» по местному времени.
fn since(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as libc::time_t;
    let (h, m) = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        (tm.tm_hour, tm.tm_min)
    };
    let ago = t.elapsed().map(|d| d.as_secs() / 60).unwrap_or(0);
    let ago = if ago >= 60 { format!("{} h {:02} min", ago / 60, ago % 60) } else { format!("{ago} min") };
    format!("{h:02}:{m:02} · {ago} ago")
}
