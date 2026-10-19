//! Пресеты режима «Свой» (как Custom Mode presets в Legion Toolkit): именованные
//! наборы лимитов прошивки и оборотов вентиляторов
//! (`~/.config/linux-legion/presets.conf`).
//!
//! Включить пресет — значит сделать его значения сохранёнными значениями
//! «Своего» (их держит служба) и сразу применить, если режим «Свой» включён.
//!
//! ```text
//! active=Gaming
//! [Gaming]
//! tunable.ppt_pl1_spl=120
//! fan.1=0
//! [Silent]
//! tunable.ppt_pl1_spl=60
//! ```

use crate::autoapply;
use crate::hw::power;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub tunables: BTreeMap<String, i64>,
    pub fans: BTreeMap<u8, u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presets {
    pub active: Option<String>,
    pub list: Vec<Preset>,
}

impl Presets {
    pub fn get(&self, name: &str) -> Option<&Preset> {
        self.list.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    fn parse(text: &str) -> Self {
        let mut out = Presets::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                out.list.push(Preset { name: name.trim().to_string(), ..Default::default() });
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match out.list.last_mut() {
                None if k == "active" && !v.is_empty() => out.active = Some(v.to_string()),
                None => {}
                Some(p) => {
                    if let Some(name) = k.strip_prefix("tunable.") {
                        if let Ok(v) = v.parse() {
                            p.tunables.insert(name.to_string(), v);
                        }
                    } else if let Some(i) = k.strip_prefix("fan.") {
                        if let (Ok(i), Ok(v)) = (i.parse(), v.parse()) {
                            p.fans.insert(i, v);
                        }
                    }
                }
            }
        }
        out.list.retain(|p| !p.name.is_empty());
        if out.active.as_deref().is_some_and(|a| out.get(a).is_none()) {
            out.active = None;
        }
        out
    }

    fn render(&self) -> String {
        let mut s = String::from("# linux-legion: Custom Mode presets\n");
        if let Some(a) = &self.active {
            s.push_str(&format!("active={a}\n"));
        }
        for p in &self.list {
            s.push_str(&format!("[{}]\n", p.name));
            for (k, v) in &p.tunables {
                s.push_str(&format!("tunable.{k}={v}\n"));
            }
            for (k, v) in &p.fans {
                s.push_str(&format!("fan.{k}={v}\n"));
            }
        }
        s
    }
}

fn path() -> std::path::PathBuf {
    autoapply::config_dir().join("presets.conf")
}

pub fn load() -> Presets {
    std::fs::read_to_string(path()).map(|t| Presets::parse(&t)).unwrap_or_default()
}

fn store(p: &Presets) -> Result<(), String> {
    autoapply::write_config(&path(), &p.render()).map_err(|e| e.to_string())
}

/// Имя пресета: видимые символы без скобок и `=`, до 40 знаков.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.chars().count() <= 40 && !n.contains(['[', ']', '=', '\n', '#'])
}

/// Сохранить текущие значения «Своего» как пресет (новый или поверх старого).
/// Берутся настраиваемые атрибуты прошивки как есть сейчас и сохранённые обороты.
pub fn save_current(name: &str) -> Result<(), String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("a preset name may not be empty or contain [ ] = #".into());
    }
    let tunables = power::read_tunables()
        .into_iter()
        .filter(|t| t.writable && t.min < t.max)
        .map(|t| (t.name, t.value))
        .collect();
    let fans = autoapply::load().fans;
    let mut all = load();
    let preset = Preset { name: name.to_string(), tunables, fans };
    match all.list.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name)) {
        Some(p) => *p = preset,
        None => all.list.push(preset),
    }
    all.active = Some(name.to_string());
    store(&all)
}

pub fn delete(name: &str) -> Result<(), String> {
    let mut all = load();
    let before = all.list.len();
    all.list.retain(|p| !p.name.eq_ignore_ascii_case(name));
    if all.list.len() == before {
        return Err(format!("no preset named “{name}”"));
    }
    if all.active.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(name)) {
        all.active = None;
    }
    store(&all)
}

/// Сделать пресет активным: его значения становятся сохранёнными значениями
/// «Своего» и сразу применяются, если режим «Свой» включён.
pub fn activate(name: &str) -> Result<Vec<String>, String> {
    let mut all = load();
    let p = all.get(name).cloned().ok_or_else(|| format!("no preset named “{name}”"))?;
    autoapply::replace_custom(p.tunables.clone(), p.fans.clone()).map_err(|e| e.to_string())?;
    all.active = Some(p.name.clone());
    store(&all)?;
    autoapply::apply_custom()
}

/// Следующий пресет по кругу (шаг автоматизации «Cycle Custom Mode preset»).
pub fn cycle() -> Result<String, String> {
    let all = load();
    if all.list.is_empty() {
        return Err("no Custom Mode presets saved".into());
    }
    let cur = all.active.as_deref().and_then(|a| all.list.iter().position(|p| p.name == a));
    let next = all.list[cur.map_or(0, |i| (i + 1) % all.list.len())].name.clone();
    activate(&next)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut p = Presets::default();
        let mut a = Preset { name: "Gaming".into(), ..Default::default() };
        a.tunables.insert("ppt_pl1_spl".into(), 120);
        a.fans.insert(1, 0);
        p.list.push(a);
        p.list.push(Preset { name: "Silent mode".into(), ..Default::default() });
        p.active = Some("Gaming".into());
        assert_eq!(Presets::parse(&p.render()), p);
        // Активный, которого нет в списке, сбрасывается; строки до первой секции,
        // кроме active, игнорируются.
        let q = Presets::parse("active=Nope\nfan.1=2\n[A]\nfan.x=1\ntunable.b=2\n");
        assert_eq!(q.active, None);
        assert_eq!(q.list[0].tunables.get("b"), Some(&2));
        assert!(q.list[0].fans.is_empty());
        assert!(valid_name("Night 2") && !valid_name(" ") && !valid_name("a=b"));
    }
}
