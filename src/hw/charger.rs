//! Источник питания: штатный адаптер или USB-C. Legion Toolkit предупреждает
//! о «маломощной зарядке» — у Legion Pro 7 штатный адаптер на 400 Вт, а USB-C
//! даёт не больше 140 Вт: ноутбук от него разряжается под нагрузкой и не
//! пускает в тяжёлые режимы.

use super::sysfs;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Charger {
    #[default]
    None,
    /// Штатный адаптер (круглый/прямоугольный разъём).
    Barrel,
    /// Питание по USB-C Power Delivery; мощность, если порт её сообщает.
    UsbC { watts: Option<u32> },
}

impl Charger {
    pub fn connected(self) -> bool {
        self != Charger::None
    }

    /// Питание только по USB-C — для этой модели заведомо маломощное.
    pub fn low_wattage(self) -> bool {
        matches!(self, Charger::UsbC { .. })
    }

    pub fn label(self) -> String {
        match self {
            Charger::None => "Not connected".into(),
            Charger::Barrel => "AC adapter".into(),
            Charger::UsbC { watts: Some(w) } => format!("USB-C, {w} W"),
            Charger::UsbC { watts: None } => "USB-C".into(),
        }
    }
}

fn online(p: &Path) -> bool {
    sysfs::read_bool(&p.join("online")).unwrap_or(false)
}

pub fn read() -> Charger {
    let Ok(rd) = std::fs::read_dir(sysfs::sys("/sys/class/power_supply")) else { return Charger::None };
    let mut supplies: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    supplies.sort();
    let kind = |p: &Path| sysfs::read(&p.join("type")).unwrap_or_default();
    // Порт USB-C, от которого сейчас идёт питание: напряжение и ток есть только у него.
    let usb = supplies.iter().find(|p| {
        matches!(kind(p).as_str(), "USB" | "USB_PD" | "USB_C" | "USB_PD_DRP")
            && online(p)
            && sysfs::read_num::<u64>(&p.join("voltage_now")).unwrap_or(0) > 0
    });
    if let Some(p) = usb {
        let num = |f: &str| sysfs::read_num::<u64>(&p.join(f)).unwrap_or(0);
        let watts = (num("voltage_max") * num("current_max") / 1_000_000_000_000) as u32;
        return Charger::UsbC { watts: (watts > 0).then_some(watts) };
    }
    if supplies.iter().any(|p| kind(p) == "Mains" && online(p)) {
        return Charger::Barrel;
    }
    Charger::None
}
