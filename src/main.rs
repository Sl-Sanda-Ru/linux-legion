//! linux-legion — центр управления ноутбуками Lenovo Legion в Linux:
//! режимы питания, лимиты мощности, вентиляторы, подсветка Spectrum, батарея.
//!
//! `linux_legion`              — работа с ноутбуком;
//! `linux_legion --simulate`   — демо-режим без железа;
//! `linux_legion --page N`     — открыть раздел N (0 главная … 5 действия);
//! `linux_legion --apply`      — применить сохранённые значения режима «Свой»;
//! `linux_legion --daemon`     — служба автоприменения (см. `autoapply`);
//! `linux_legion get|set|run…` — командная строка (см. `cli`);
//! `LEGION_TRACE=1`            — печатать пакеты подсветки в stderr.

mod actions;
mod automation;
mod autoapply;
mod cli;
mod hw;
mod json;
mod notify;
mod presets;
mod spectrum;
mod tray;
mod ui;
mod worker;

use syngui::prelude::*;
use syngui::text::icon_fonts::material;
use ui::app::{build_root, AppCtx};

const STYLES: &str = include_str!("../styles/app.mss");
const ICON: &[u8] = include_bytes!("../assets/icon/linux-legion-256.png");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let simulate = args.iter().any(|a| a == "--simulate");
    if simulate {
        let root = hw::sim::install();
        autoapply::set_dir(root.join("config"));
    }
    if args.iter().any(|a| a == "--daemon") {
        autoapply::daemon();
    }
    if args.iter().any(|a| a == "--apply") {
        match autoapply::apply() {
            Ok(done) if done.is_empty() => println!("nothing to apply (not in Custom mode, or the values are already set)"),
            Ok(done) => println!("applied: {}", done.join(", ")),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    // Команда (`get`, `set`, `run`…) — без окна.
    let positional: Vec<String> = args.iter().skip(1).filter(|a| *a != "--simulate").cloned().collect();
    if positional.first().is_some_and(|a| !a.starts_with("--") || a == "--help") {
        match cli::run(&positional, simulate) {
            Some(Ok(())) => return,
            Some(Err(e)) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
            None if positional[0].parse::<u32>().is_err() => {
                eprintln!("unknown command “{}”\n\n{}", positional[0], cli::HELP);
                std::process::exit(2);
            }
            None => {}
        }
    }
    // Интерфейс английский — кнопки диалогов тоже, какой бы ни была локаль.
    syngui::widgets::set_dialog_labels("OK", "Cancel");
    // Сигналы создаются один раз, до run().
    let ctx = AppCtx::new(simulate);
    if let Some(n) = args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)?.parse().ok()) {
        ctx.go(n);
    }

    App::new()
        .title("Legion Control")
        .app_id("linux-legion")
        .with_window_icon_png(ICON)
        .size(1360, 900)
        .min_size(1180, 720)
        .with_icon_font(material::FONT_DATA)
        // Одно окно: повторный запуск (из трея, меню, ярлыка) выводит его вперёд.
        .with_single_instance(if simulate { "linux-legion-simulate" } else { "linux-legion" })
        .with_styles_str(STYLES)
        .run(move |_| {
            ctx.start();
            Box::new(build_root(ctx.clone()))
        });
}
