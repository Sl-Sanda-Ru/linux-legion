//! Действия: правила «когда → что сделать» и быстрые действия, как раздел
//! Actions в Lenovo Legion Toolkit. Правила с триггером выполняет служба,
//! быстрые — кнопка здесь или `linux_legion run ИМЯ`.

use crate::actions::{self, Action, Kind};
use crate::automation::{Param, Rule, Trigger, TriggerKind};
use crate::ui::app::{AppCtx, Draft, DraftVals};
use crate::ui::icons;
use crate::ui::widgets::{card, card_with, nothing, page_header, reactive, reactive_box};
use crate::worker::Job;
use syngui::prelude::*;

pub fn view(ctx: AppCtx) -> impl Widget {
    let (c_banner, c_list, c_edit) = (ctx.clone(), ctx.clone(), ctx.clone());
    Column::new()
        .gap(18.0)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .child(page_header(
            "Actions",
            "Run steps automatically when something happens, or with one click — like Actions in Legion Toolkit.",
        ))
        .child(reactive_box(move || service_banner(&c_banner)))
        .child(reactive_box(move || editor(&c_edit)))
        .child(rules_card(c_list))
        .child(delete_dialog(ctx))
        .class("page")
}

fn service_banner(ctx: &AppCtx) -> Box<dyn Widget> {
    let (svc, _) = ctx.sink.autoapply.get();
    let rules = ctx.sink.rules.get();
    let automatic = rules.iter().any(|r| r.enabled && r.trigger != Trigger::Manual);
    if !automatic || (svc.enabled && svc.active) {
        return nothing();
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
                            "Actions with a trigger run in the linux-legion service, and it is not running. \
                             Quick actions still work from here.",
                        )
                        .class("row-desc grow"),
                    )
                    .child(
                        Button::new("Start the service")
                            .icon(icons::PLAY)
                            .disabled(!svc.installed)
                            .on_click(move || c.send(Job::SetAutoApply(true)))
                            .class("btn-primary"),
                    ),
            )
            .class("banner banner-info"),
    )
}

fn steps_summary(steps: &[Action]) -> String {
    if steps.is_empty() {
        return "No steps".into();
    }
    steps.iter().map(Action::describe).collect::<Vec<_>>().join(" → ")
}

fn rules_card(ctx: AppCtx) -> impl Widget {
    let c_new = ctx.clone();
    card_with(
        icons::ACTIONS,
        "Actions",
        move || {
            let c = c_new.clone();
            Button::new("New action").icon(icons::ADD).on_click(move || c.open_draft(None)).class("btn-primary")
        },
        reactive(move || {
            let rules = ctx.sink.rules.get();
            let mut col = Column::new().gap(10.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
            if rules.is_empty() {
                return col.child(
                    Text::new(
                        "No actions yet. For example: switch to Quiet and dim the keyboard when the charger is \
                         unplugged, or load a Custom Mode preset when a game starts.",
                    )
                    .class("muted"),
                );
            }
            for (i, r) in rules.iter().enumerate() {
                let (c_on, c_run, c_edit, c_del) = (ctx.clone(), ctx.clone(), ctx.clone(), ctx.clone());
                let manual = r.trigger == Trigger::Manual;
                let name = r.name.clone();
                let lead: Box<dyn Widget> = if manual {
                    Box::new(
                        DecoratedBox::new()
                            .child(Center::new().child(Icon::new(icons::PLAY).class("row-icon")))
                            .class("row-icon-badge"),
                    )
                } else {
                    Box::new(Toggle::new().on(r.enabled).on_change(move |v| {
                        let mut rules = c_on.sink.rules.get_untracked();
                        if let Some(r) = rules.get_mut(i) {
                            r.enabled = v;
                        }
                        c_on.send(Job::SaveRules(rules));
                    }))
                };
                col = col.child(
                    DecoratedBox::new()
                        .child(
                            Row::new()
                                .gap(14.0)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .children(vec![lead])
                                .child(
                                    Column::new()
                                        .gap(3.0)
                                        .child(Text::new(r.name.clone()).class("row-title"))
                                        .child(Text::new(r.trigger.describe()).class("row-desc"))
                                        .child(Text::new(steps_summary(&r.steps)).class("scale-label"))
                                        .class("grow"),
                                )
                                .child(
                                    ToolButton::new(icons::PLAY)
                                        .tooltip("Run now")
                                        .disabled(r.steps.is_empty())
                                        .on_click(move || c_run.send(Job::RunRule(name.clone()))),
                                )
                                .child(ToolButton::new(icons::EDIT).tooltip("Edit").on_click(move || c_edit.open_draft(Some(i))))
                                .child(ToolButton::new(icons::DELETE).tooltip("Delete").on_click(move || {
                                    c_del.act_delete.set(Some(i));
                                    c_del.act_delete_open.set(true);
                                })),
                        )
                        .class("layer-row"),
                );
            }
            col
        }),
    )
}

fn delete_dialog(ctx: AppCtx) -> impl Widget {
    let c = ctx.clone();
    ConfirmDialog::new("Delete action", "The action will be removed from automation.conf.", ctx.act_delete_open, move |yes| {
        if let Some(i) = c.act_delete.get_untracked().filter(|_| yes) {
            let mut rules = c.sink.rules.get_untracked();
            if i < rules.len() {
                rules.remove(i);
                c.send(Job::SaveRules(rules));
            }
        }
        c.act_delete.set(None);
    })
}

// ---------------------------------------------------------------------------
// Редактор

fn choice_items(list: &[(String, String)]) -> Vec<DropdownItem> {
    list.iter().map(|(v, l)| DropdownItem::new(v.clone(), l.clone())).collect()
}

/// Значение по умолчанию для вида шага/триггера — первый вариант списка.
pub fn default_step_value(k: Kind) -> String {
    match k.value() {
        actions::Value::Choice(list) => list.first().map(|(v, _)| v.clone()).unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn default_trigger_value(k: TriggerKind) -> String {
    match k.param() {
        Param::Choice(list) => list.first().map(|(v, _)| v.clone()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Поле значения: выпадающий список или текст. Значение пишется в `vals`
/// (окно его не перечитывает — иначе поле теряло бы фокус на каждой букве).
fn value_field(
    choices: Option<Vec<(String, String)>>,
    hint: &str,
    current: String,
    set: impl FnMut(&str) + Send + 'static,
) -> Box<dyn Widget> {
    match choices {
        Some(list) if list.is_empty() => Box::new(Text::new("Nothing to choose from yet").class("muted")),
        Some(list) => Box::new(Dropdown::new().items(choice_items(&list)).selected(current).width(240.0).on_change(set)),
        None => Box::new(TextField::new().text(current).placeholder(hint).width(320.0).on_change(set)),
    }
}

fn editor(ctx: &AppCtx) -> Box<dyn Widget> {
    let Some(d) = ctx.act_draft.get() else { return nothing() };
    let vals = ctx.act_vals.get_untracked();
    let mut col = Column::new().gap(18.0).cross_axis_alignment(CrossAxisAlignment::Stretch);

    let c = ctx.clone();
    col = col.child(field(
        "Name",
        TextField::new().text(vals.name.clone()).placeholder("On battery").width(320.0).on_change(move |t| {
            c.act_vals.update(|v| v.name = t.to_string());
        }),
    ));

    // Триггер.
    let c = ctx.clone();
    let mut trig = Row::new().gap(12.0).cross_axis_alignment(CrossAxisAlignment::Center).child(
        Dropdown::new()
            .items(TriggerKind::ALL.iter().map(|k| DropdownItem::new(k.key(), k.label())).collect())
            .selected(d.trigger.key())
            .width(320.0)
            .on_change(move |key| {
                let Some(k) = TriggerKind::from_key(key) else { return };
                c.act_vals.update(|v| v.trigger = default_trigger_value(k));
                c.act_draft.update(|d| {
                    if let Some(d) = d {
                        d.trigger = k;
                    }
                });
            }),
    );
    let c = ctx.clone();
    let set_trigger = move |t: &str| c.act_vals.update(|v| v.trigger = t.to_string());
    match d.trigger.param() {
        Param::None => {}
        Param::Choice(list) => trig = trig.children(vec![value_field(Some(list), "", vals.trigger.clone(), set_trigger)]),
        Param::Text(hint) => trig = trig.children(vec![value_field(None, hint, vals.trigger.clone(), set_trigger)]),
    }
    col = col.child(field("When", trig));

    // Шаги.
    let mut steps = Column::new().gap(10.0).cross_axis_alignment(CrossAxisAlignment::Stretch);
    let n = d.steps.len();
    for (i, k) in d.steps.iter().copied().enumerate() {
        let (c_kind, c_val, c_up, c_dn, c_del) = (ctx.clone(), ctx.clone(), ctx.clone(), ctx.clone(), ctx.clone());
        let current = vals.steps.get(i).cloned().unwrap_or_default();
        let set = move |t: &str| {
            c_val.act_vals.update(|v| {
                if let Some(s) = v.steps.get_mut(i) {
                    *s = t.to_string();
                }
            })
        };
        let value: Box<dyn Widget> = match k.value() {
            actions::Value::None => nothing(),
            actions::Value::Choice(list) => value_field(Some(list), "", current, set),
            actions::Value::Text(hint) => value_field(None, hint, current, set),
        };
        steps = steps.child(
            Row::new()
                .gap(10.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .child(Text::new(format!("{}.", i + 1)).class("field-label"))
                .child(
                    Dropdown::new()
                        .items(Kind::ALL.iter().map(|k| DropdownItem::new(k.key(), k.label())).collect())
                        .selected(k.key())
                        .width(280.0)
                        .on_change(move |key| {
                            let Some(k) = Kind::from_key(key) else { return };
                            c_kind.act_vals.update(|v| {
                                if let Some(s) = v.steps.get_mut(i) {
                                    *s = default_step_value(k);
                                }
                            });
                            c_kind.act_draft.update(|d| {
                                if let Some(s) = d.as_mut().and_then(|d| d.steps.get_mut(i)) {
                                    *s = k;
                                }
                            });
                        }),
                )
                .children(vec![value])
                .child(DecoratedBox::new().class("grow"))
                .child(ToolButton::new(icons::UP).tooltip("Move up").disabled(i == 0).on_click(move || c_up.move_step(i, i - 1)))
                .child(
                    ToolButton::new(icons::DOWN)
                        .tooltip("Move down")
                        .disabled(i + 1 >= n)
                        .on_click(move || c_dn.move_step(i, i + 1)),
                )
                .child(ToolButton::new(icons::DELETE).tooltip("Remove step").on_click(move || c_del.remove_step(i))),
        );
    }
    let c = ctx.clone();
    steps = steps.child(
        Row::new().child(
            Button::new("Add step")
                .icon(icons::ADD)
                .on_click(move || {
                    let k = Kind::PowerMode;
                    c.act_vals.update(|v| v.steps.push(default_step_value(k)));
                    c.act_draft.update(|d| {
                        if let Some(d) = d {
                            d.steps.push(k);
                        }
                    });
                })
                .class("btn-ghost"),
        ),
    );
    col = col.child(field("Then", steps));

    if let Some(e) = &d.error {
        col = col.child(
            Row::new()
                .gap(8.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .child(Icon::new(icons::ERROR).class("status-icon tone-bad"))
                .child(Text::new(e.clone()).class("row-warn")),
        );
    }

    let (c_save, c_cancel) = (ctx.clone(), ctx.clone());
    col = col.child(
        Row::new()
            .gap(8.0)
            .child(DecoratedBox::new().class("grow"))
            .child(Button::new("Cancel").icon(icons::CLOSE).on_click(move || c_cancel.act_draft.set(None)).class("btn-ghost"))
            .child(Button::new("Save").icon(icons::SAVE).on_click(move || c_save.save_draft()).class("btn-primary")),
    );
    let title = if d.index.is_some() { "Edit action" } else { "New action" };
    Box::new(card(title, "", col))
}

fn field(label: &str, control: impl Widget + 'static) -> impl Widget {
    Column::new()
        .gap(8.0)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .child(Text::new(label).class("field-label"))
        .child(control)
}

/// Собрать правило из черновика или объяснить, что не так.
pub fn build_rule(d: &Draft, v: &DraftVals, existing: &[Rule]) -> std::result::Result<Rule, String> {
    let name = v.name.trim();
    if !crate::automation::valid_name(name) {
        return Err("Give the action a name (without [ ] = #).".into());
    }
    if existing.iter().enumerate().any(|(i, r)| Some(i) != d.index && r.name.eq_ignore_ascii_case(name)) {
        return Err(format!("An action named “{name}” already exists."));
    }
    let trigger = Trigger::from_parts(d.trigger, &v.trigger)?;
    let steps = d
        .steps
        .iter()
        .zip(v.steps.iter())
        .enumerate()
        .map(|(i, (k, val))| Action::from_parts(*k, val).map_err(|e| format!("Step {}: {e}", i + 1)))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if steps.is_empty() {
        return Err("Add at least one step.".into());
    }
    let enabled = d.index.and_then(|i| existing.get(i)).map_or(true, |r| r.enabled);
    Ok(Rule { name: name.to_string(), enabled, trigger, steps })
}
