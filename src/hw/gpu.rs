//! Программы, которые держат дискретную видеокарту NVIDIA включённой (как
//! «Deactivate GPU» в Legion Toolkit). Пока хоть одна из них открыла
//! `/dev/nvidiaN`, карта не уходит в сон и тратит батарею.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuProcess {
    pub pid: i32,
    pub name: String,
    /// Можно завершить: процесс текущего пользователя и не графическая оболочка.
    pub killable: bool,
}

/// Оболочки и службы, которые закрывать нельзя: вместе с ними пропадёт сеанс.
const PROTECTED: [&str; 16] = [
    "xorg", "xwayland", "gnome-shell", "kwin_wayland", "kwin_x11", "plasmashell", "mutter", "sway", "hyprland",
    "cosmic-comp", "niri", "labwc", "wayfire", "nvidia-persistenced", "systemd", "gdm",
];

fn uid_of(pid: i32) -> Option<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Процессы с открытым `/dev/nvidiaN`. Чужие процессы видны, только если
/// хватает прав читать их `fd`.
pub fn processes() -> Vec<GpuProcess> {
    let me = unsafe { libc::getuid() };
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else { continue };
        let holds = fds.flatten().any(|fd| {
            std::fs::read_link(fd.path()).is_ok_and(|l| {
                l.to_string_lossy().strip_prefix("/dev/nvidia").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
        });
        if !holds {
            continue;
        }
        let name = std::fs::read_to_string(e.path().join("comm")).map(|s| s.trim().to_string()).unwrap_or_default();
        let protected = PROTECTED.contains(&name.to_lowercase().as_str());
        let killable = !protected && uid_of(pid) == Some(me) && pid != std::process::id() as i32;
        out.insert(pid, GpuProcess { pid, name, killable });
    }
    out.into_values().collect()
}

/// Попросить программы завершиться (SIGTERM) — только те, что можно.
/// Возвращает, скольким отправлен сигнал.
pub fn terminate(list: &[GpuProcess]) -> usize {
    list.iter().filter(|p| p.killable).filter(|p| unsafe { libc::kill(p.pid, libc::SIGTERM) } == 0).count()
}
