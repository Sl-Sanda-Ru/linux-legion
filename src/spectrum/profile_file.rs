//! Файлы профилей подсветки в формате Lenovo Legion Toolkit.
//!
//! Legion Toolkit экспортирует профиль как JSON-массив слоёв (Newtonsoft,
//! перечисления строками), а коды зон в нём — те же коды контроллера, что и
//! здесь. Поэтому профиль, сохранённый в Windows, загружается в Linux и
//! наоборот:
//!
//! ```json
//! [
//!   {
//!     "Type": "ColorWave",
//!     "Speed": "Speed2",
//!     "Direction": "LeftToRight",
//!     "ClockwiseDirection": "None",
//!     "Colors": [ { "R": 255, "G": 0, "B": 0 } ],
//!     "Keys": [ 1, 2, 3 ],
//!     "UseVantageColorBoost": false,
//!     "ColorMode": "ColorList"
//!   }
//! ]
//! ```

use super::protocol::{ColorKind, Direction, Effect, EffectType, Rgb, Rotation};
use crate::json::{self, Obj, Out, Value};

fn type_name(t: EffectType) -> &'static str {
    use EffectType::*;
    match t {
        Always => "Always",
        RainbowScrew => "RainbowScrew",
        RainbowWave => "RainbowWave",
        ColorChange => "ColorChange",
        ColorWave => "ColorWave",
        ColorPulse => "ColorPulse",
        Smooth => "Smooth",
        Rain => "Rain",
        Ripple => "Ripple",
        Type => "Type",
        AudioBounce => "AudioBounce",
        AudioRipple => "AudioRipple",
        AuroraSync => "AuroraSync",
    }
}

fn type_from(s: &str) -> Option<EffectType> {
    use EffectType::*;
    [
        Always, RainbowScrew, RainbowWave, ColorChange, ColorWave, ColorPulse, Smooth, Rain, Ripple, Type, AudioBounce,
        AudioRipple, AuroraSync,
    ]
    .into_iter()
    .find(|t| type_name(*t).eq_ignore_ascii_case(s))
}

fn direction_name(d: Direction) -> &'static str {
    match d {
        Direction::None => "None",
        Direction::BottomToTop => "BottomToTop",
        Direction::TopToBottom => "TopToBottom",
        Direction::LeftToRight => "LeftToRight",
        Direction::RightToLeft => "RightToLeft",
    }
}

fn direction_from(s: &str) -> Direction {
    Direction::ALL.into_iter().find(|d| direction_name(*d).eq_ignore_ascii_case(s)).unwrap_or_default()
}

fn rotation_name(r: Rotation) -> &'static str {
    match r {
        Rotation::None => "None",
        Rotation::Clockwise => "Clockwise",
        Rotation::CounterClockwise => "CounterClockwise",
    }
}

fn rotation_from(s: &str) -> Rotation {
    [Rotation::Clockwise, Rotation::CounterClockwise]
        .into_iter()
        .find(|r| rotation_name(*r).eq_ignore_ascii_case(s))
        .unwrap_or_default()
}

/// Скорость: `Speed1`…`Speed3`; Newtonsoft без конвертера пишет число.
fn speed_from(v: Option<&Value>) -> u8 {
    match v {
        Some(Value::String(s)) => s.strip_prefix("Speed").and_then(|n| n.parse().ok()).unwrap_or(0),
        Some(Value::Number(n)) => *n as u8,
        _ => 0,
    }
}

/// Перечисление может прийти строкой или числом (без `StringEnumConverter`).
fn enum_str<'a>(v: Option<&'a Value>, by_index: &'a [&'a str]) -> &'a str {
    match v {
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => by_index.get(*n as usize).copied().unwrap_or("None"),
        _ => "None",
    }
}

pub fn export(effects: &[Effect]) -> String {
    let items = effects
        .iter()
        .map(|e| {
            let color_mode = match e.kind.color_kind() {
                ColorKind::None => "None",
                _ if e.colors.is_empty() => "RandomColor",
                _ => "ColorList",
            };
            let speed = if e.speed == 0 { "None".to_string() } else { format!("Speed{}", e.speed) };
            Out::Obj(Obj(vec![
                ("Type", Out::Str(type_name(e.kind).into())),
                ("Speed", Out::Str(speed)),
                ("Direction", Out::Str(direction_name(e.direction).into())),
                ("ClockwiseDirection", Out::Str(rotation_name(e.rotation).into())),
                (
                    "Colors",
                    Out::List(
                        e.colors
                            .iter()
                            .map(|c| {
                                Out::Obj(Obj(vec![
                                    ("R", Out::Int(c.r as i64)),
                                    ("G", Out::Int(c.g as i64)),
                                    ("B", Out::Int(c.b as i64)),
                                ]))
                            })
                            .collect(),
                    ),
                ),
                ("Keys", Out::List(e.keys.iter().map(|&k| Out::Int(k as i64)).collect())),
                ("UseVantageColorBoost", Out::Bool(false)),
                ("ColorMode", Out::Str(color_mode.into())),
            ]))
        })
        .collect();
    json::render(&Out::List(items))
}

pub fn import(text: &str) -> Result<Vec<Effect>, String> {
    let v = json::parse(text)?;
    let items = v.as_array().ok_or("expected a JSON array of effects")?;
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let n = i + 1;
        let kind_s = enum_str(
            it.get("Type"),
            &[
                "Always", "RainbowScrew", "RainbowWave", "ColorChange", "ColorWave", "ColorPulse", "Smooth", "Rain",
                "Ripple", "Type", "AudioBounce", "AudioRipple", "AuroraSync",
            ],
        );
        let kind = type_from(kind_s).ok_or_else(|| format!("effect {n}: unknown type “{kind_s}”"))?;
        let colors: Vec<Rgb> = it
            .get("Colors")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|c| {
                let ch = |k| c.get(k).and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 255.0) as u8;
                Rgb::new(ch("R"), ch("G"), ch("B"))
            })
            .collect();
        let keys: Vec<u16> = it
            .get("Keys")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_f64)
            .filter(|k| (1.0..=65535.0).contains(k))
            .map(|k| k as u16)
            .collect();
        if keys.is_empty() && !kind.is_all_zones() {
            return Err(format!("effect {n} ({}) has no zones", kind.label()));
        }
        let random = enum_str(it.get("ColorMode"), &["None", "RandomColor", "ColorList"]) == "RandomColor";
        let mut e = Effect::new(kind, keys);
        e.speed = speed_from(it.get("Speed"));
        e.direction = direction_from(enum_str(
            it.get("Direction"),
            &["None", "BottomToTop", "TopToBottom", "LeftToRight", "RightToLeft"],
        ));
        e.rotation = rotation_from(enum_str(it.get("ClockwiseDirection"), &["None", "Clockwise", "CounterClockwise"]));
        e.colors = if random { Vec::new() } else { colors };
        e.normalize();
        out.push(e);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut wave = Effect::new(EffectType::ColorWave, vec![1, 2, 3]);
        wave.colors = vec![Rgb::new(255, 0, 0), Rgb::new(0, 0, 255)];
        wave.direction = Direction::RightToLeft;
        let mut screw = Effect::new(EffectType::RainbowScrew, vec![0x42]);
        screw.rotation = Rotation::CounterClockwise;
        screw.speed = 3;
        let random = Effect::new(EffectType::Rain, vec![7]);
        let effects = vec![Effect::new(EffectType::Always, vec![4, 5]), wave, screw, random, Effect::new(EffectType::AudioBounce, vec![])];
        let text = export(&effects);
        assert!(text.contains("\"Type\": \"ColorWave\""));
        assert!(text.contains("\"ColorMode\": \"RandomColor\""));
        assert_eq!(import(&text).unwrap(), effects);
    }

    /// Файл, сохранённый Legion Toolkit в Windows (числовые перечисления тоже
    /// встречаются — у старых версий без `StringEnumConverter`).
    #[test]
    fn imports_legion_toolkit_export() {
        let text = r#"[
          {"Type":"Always","Speed":"None","Direction":"None","ClockwiseDirection":"None",
           "Colors":[{"R":21,"G":141,"B":221}],"Keys":[1,2,3],"UseVantageColorBoost":false,"ColorMode":"ColorList"},
          {"Type":2,"Speed":2,"Direction":3,"ClockwiseDirection":0,"Colors":[],"Keys":[0x10]}
        ]"#
        .replace("0x10", "16");
        let e = import(&text).unwrap();
        assert_eq!(e[0].kind, EffectType::Always);
        assert_eq!(e[0].colors, vec![Rgb::new(21, 141, 221)]);
        assert_eq!(e[1].kind, EffectType::RainbowWave);
        assert_eq!(e[1].speed, 2);
        assert_eq!(e[1].direction, Direction::LeftToRight);
        assert!(import("[{\"Type\":\"Nope\",\"Keys\":[1]}]").is_err());
        assert!(import("[{\"Type\":\"Always\",\"Keys\":[]}]").is_err());
    }
}
