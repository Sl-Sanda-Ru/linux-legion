//! Состояние вокруг ноутбука для триггеров автоматизации: крышка, внешние
//! мониторы, запущенные программы.

use super::sysfs;
use std::collections::BTreeSet;

/// Крышка закрыта (`/proc/acpi/button/lid/*/state`). `None` — нет датчика.
pub fn lid_closed() -> Option<bool> {
    let dir = std::fs::read_dir("/proc/acpi/button/lid").ok()?.flatten().next()?.path();
    let s = std::fs::read_to_string(dir.join("state")).ok()?;
    Some(s.contains("closed"))
}

/// Сколько внешних мониторов подключено (DRM-разъёмы, кроме встроенной панели).
pub fn external_displays() -> usize {
    let Ok(rd) = std::fs::read_dir(sysfs::sys("/sys/class/drm")) else { return 0 };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("card") && n.contains('-') && !["eDP", "LVDS", "DSI"].iter().any(|i| n.contains(i))
        })
        .filter(|e| sysfs::read(&e.path().join("status")).as_deref() == Some("connected"))
        .count()
}

/// Имена запущенных программ в нижнем регистре: `comm` (до 15 знаков) и имя
/// исполняемого файла — чтобы «steam» и «cs2» находились по любому из них.
pub fn process_names() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let p = e.path();
        if let Ok(c) = std::fs::read_to_string(p.join("comm")) {
            out.insert(c.trim().to_lowercase());
        }
        if let Ok(cmd) = std::fs::read(p.join("cmdline")) {
            let argv0 = cmd.split(|&b| b == 0).next().unwrap_or_default();
            let argv0 = String::from_utf8_lossy(argv0);
            // Windows-игры под Wine/Proton: «C:\…\game.exe».
            if let Some(name) = argv0.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()) {
                out.insert(name.to_lowercase());
            }
        }
    }
    out
}

/// Есть ли среди запущенных хотя бы одна из `names` (без учёта регистра;
/// «game» совпадает и с «game.exe»).
pub fn any_running(running: &BTreeSet<String>, names: &[String]) -> bool {
    names.iter().map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()).any(|n| {
        running.contains(&n) || running.contains(&format!("{n}.exe")) || n.strip_suffix(".exe").is_some_and(|b| running.contains(b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_process_names() {
        let running: BTreeSet<String> = ["steam", "cs2.exe", "firefox"].into_iter().map(String::from).collect();
        assert!(any_running(&running, &["Steam".into()]));
        assert!(any_running(&running, &["cs2".into()]));
        assert!(any_running(&running, &["firefox.exe".into()]));
        assert!(!any_running(&running, &["".into(), "obs".into()]));
    }
}
