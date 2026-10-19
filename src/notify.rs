//! Уведомления рабочего стола (как всплывающие уведомления Legion Toolkit):
//! смена режима питания по Fn+Q, Fn Lock, камера, подключение адаптера,
//! сообщения автоматизации.
//!
//! Отправляются через `notify-send`, а если его нет — через `gdbus` напрямую
//! в `org.freedesktop.Notifications`. Повторное уведомление того же вида
//! заменяет прошлое, а не копится стопкой.

use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    PowerMode,
    FnLock,
    Camera,
    AcAdapter,
    Automation,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::PowerMode, Kind::FnLock, Kind::Camera, Kind::AcAdapter, Kind::Automation];

    /// Ключ в custom.conf: `notify.power_mode=1`.
    pub fn key(self) -> &'static str {
        match self {
            Kind::PowerMode => "notify.power_mode",
            Kind::FnLock => "notify.fn_lock",
            Kind::Camera => "notify.camera",
            Kind::AcAdapter => "notify.ac_adapter",
            Kind::Automation => "notify.automation",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::PowerMode => "Power Mode",
            Kind::FnLock => "Fn Lock",
            Kind::Camera => "Camera",
            Kind::AcAdapter => "AC adapter",
            Kind::Automation => "Actions",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Kind::PowerMode => "When the power mode changes, including with Fn+Q.",
            Kind::FnLock => "When Fn Lock is turned on or off.",
            Kind::Camera => "When the camera is turned on or off.",
            Kind::AcAdapter => "When the charger is connected or disconnected, and when it is a low wattage one.",
            Kind::Automation => "When an action shows a notification or fails.",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Kind::PowerMode => "linux-legion",
            Kind::FnLock => "input-keyboard",
            Kind::Camera => "camera-web",
            Kind::AcAdapter => "ac-adapter",
            Kind::Automation => "linux-legion",
        }
    }
}

/// Показать уведомление. Не ждёт и не сообщает об ошибках: уведомления —
/// украшение, служба без них работает так же.
pub fn send(kind: Kind, summary: &str, body: &str) {
    let tag = format!("string:x-canonical-private-synchronous:linux-legion-{}", kind.key());
    let notify_send = Command::new("notify-send")
        .args(["--app-name=Legion Control", "--expire-time=3000", "--icon", kind.icon(), "--hint", &tag, summary, body])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let child = match notify_send {
        Ok(c) => Ok(c),
        Err(_) => Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest=org.freedesktop.Notifications",
                "--object-path=/org/freedesktop/Notifications",
                "--method=org.freedesktop.Notifications.Notify",
                "Legion Control",
                "0",
                kind.icon(),
                summary,
                body,
                "[]",
                "{}",
                "3000",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn(),
    };
    // Забрать завершившийся процесс, чтобы не копились зомби.
    if let Ok(mut c) = child {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}
