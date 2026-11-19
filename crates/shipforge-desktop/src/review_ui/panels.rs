use super::*;
use crate::{appearance::paint as rgb, controls::*};
use chrono::NaiveDate;
impl Workbench {
    fn review_button(
        &self,
        id: String,
        label: &str,
        a: UiAction,
        cx: &Context<Self>,
    ) -> gpui_kit::base::Button {
        outlined(self.button(id, label.to_owned(), a, cx))
            .child(crate::locale::label(label).to_owned())
    }
    pub fn review_actions(&self, record: &Record, cx: &Context<Self>) -> Div {
        let mut actions = row().flex_wrap().gap(px(4.));
        if !self.can_review() {
            return actions;
        }
        for choice in ["accept", "duplicate", "reject", "defer"] {
            actions = actions.child(self.review_button(
                format!("triage-{choice}-{}", record.id),
                decision_label(choice),
                Self::decide_form(vec![record.clone()], choice),
                cx,
            ));
        }
        if record
            .fields
            .get("deferred_until")
            .and_then(Value::as_i64)
            .is_some_and(|t| t > Local::now().timestamp_millis())
        {
            actions = actions.child(self.review_button(
                format!("triage-restore-{}", record.id),
                decision_label("restore"),
                Self::decide_form(vec![record.clone()], "restore"),
                cx,
            ));
        }
        actions
    }
    pub fn review_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let s = &self.review.settings;
        let mut page = col()
            .id("triage-review")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(14.))
            .gap(px(7.));
        let button =
            |id: &str, label: &str, a: UiAction| self.review_button(id.into(), label, a, cx);
        let mut bar = row().flex_wrap().gap(px(5.)).child(headline(
            format!(
                "待分拣 · {}",
                self.review.queue["total"].as_u64().unwrap_or(0)
            ),
            16.,
        ));
        if s["enabled"] == true && (self.can_review() || self.role == "guest") {
            bar = bar.child(button(
                "triage-submit",
                "创建待分拣任务",
                UiAction::Form(Mode::Review(ReviewForm::Submit(self.loc.space.clone()))),
            ));
        }
        if s["can_configure"] == true {
            bar = bar.child(button(
                "triage-settings",
                "分拣设置",
                UiAction::Form(Mode::Review(ReviewForm::Config(s.clone()))),
            ));
        }
        page = page.child(bar);
        let duty = &s["duty"];
        if let Some(id) = duty["member"].as_str() {
            page = page.child(text(
                format!(
                    "分拣责任 · {} · {}",
                    if duty["mode"] == "assign" {
                        "指派"
                    } else {
                        "通知"
                    },
                    self.name("member", id)
                ),
                12.,
                MUTED,
            ));
        }
        if s["enabled"] != true {
            return page;
        }
        let mut display =
            row()
                .flex_wrap()
                .gap(px(4.))
                .child(button("triage-filter", "筛选", UiAction::Filter));
        for (field, label, on) in [
            ("newest", "最新进入", self.review.newest),
            ("deferred", "显示稍后处理", self.review.deferred),
            ("codes", "显示任务编号", self.review.codes),
            ("dates", "显示截止日期", self.review.dates),
        ] {
            display = display.child(
                button(
                    &format!("triage-display-{field}"),
                    label,
                    UiAction::Review(Action::Toggle(field)),
                )
                .when(on, |b| b.bg(rgb(SELECTED))),
            );
        }
        page = page.child(display);
        if self.can_review() && !self.review.selected.is_empty() {
            let mut bulk = row().flex_wrap().gap(px(5.)).child(text(
                format!("已选择 {} 项", self.review.selected.len()),
                12.,
                MUTED,
            ));
            for d in ["accept", "reject", "defer"] {
                bulk = bulk.child(button(
                    &format!("triage-bulk-{d}"),
                    decision_label(d),
                    UiAction::Review(Action::Batch(d)),
                ));
            }
            page = page.child(bulk);
        }
        let rows = self.queue_rows();
        if rows.is_empty() {
            page = page.child(text("没有符合条件的待分拣任务", 13., MUTED));
        }
        for task in rows {
            let id = &task.id;
            let mut line = row()
                .w_full()
                .min_h(px(40.))
                .gap(px(6.))
                .border_b_1()
                .border_color(rgb(LINE));
            if self.can_review() {
                line = line.child(button(
                    &format!("triage-select-{id}"),
                    if self.review.selected.contains(id) {
                        "取消选择"
                    } else {
                        "选择"
                    },
                    UiAction::Review(Action::Select(id.clone())),
                ));
            }
            if self.review.codes {
                line = line.child(text(task.code(), 11., MUTED));
            }
            line = line.child(
                self.button(
                    format!("triage-open-{id}"),
                    task.title.clone(),
                    UiAction::Review(Action::Open(task.clone())),
                    cx,
                )
                .min_w_0()
                .flex_1()
                .justify_start()
                .child(task.title.clone()),
            );
            if self.review.dates && !task.field("due_date").is_empty() {
                line = line.child(text(task.field("due_date").to_owned(), 11., INK));
            }
            if task.fields.get("triage_snoozed") == Some(&json!(true)) {
                line = line.child(chip("已搁置", PALE, MUTED));
            }
            if task.fields.get("triage_rule_failed") == Some(&json!(true)) {
                line = line.child(chip("规则需检查", PALE, ERROR));
            }
            page = page.child(line);
        }
        if s["rules"].is_array() {
            let mut heading = row().gap(px(8.)).child(headline("分拣规则", 14.));
            let manage = s["can_configure"] == true;
            if manage {
                heading = heading.child(button(
                    "triage-new-rule",
                    "新建规则",
                    UiAction::Form(Mode::Review(ReviewForm::Rule {
                        settings: s.clone(),
                        index: None,
                    })),
                ));
            }
            page = page.child(heading);
            if let Some(conflicts) = s["conflicts"].as_array().filter(|a| !a.is_empty()) {
                let labels = conflicts
                    .iter()
                    .filter_map(|x| x["field"].as_str())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join("、");
                page = page.child(text(
                    format!("规则可能重复设置 {labels}，以后匹配的规则为准"),
                    12.,
                    ERROR,
                ));
            }
            let rules = s["rules"].as_array().unwrap();
            for (i, r) in rules.iter().enumerate() {
                let mut line = row().gap(px(4.)).flex_wrap().child(text(
                    r["title"]
                        .as_str()
                        .or(r["id"].as_str())
                        .unwrap_or("")
                        .to_owned(),
                    13.,
                    INK,
                ));
                if manage {
                    line = line.child(button(
                        &format!("triage-rule-edit-{i}"),
                        "编辑",
                        UiAction::Form(Mode::Review(ReviewForm::Rule {
                            settings: s.clone(),
                            index: Some(i),
                        })),
                    ));
                    line = line.child(button(
                        &format!("triage-rule-toggle-{i}"),
                        if r["enabled"] == false {
                            "启用"
                        } else {
                            "停用"
                        },
                        UiAction::Review(Action::ToggleRule(i)),
                    ));
                    if i > 0 {
                        line = line.child(button(
                            &format!("triage-rule-up-{i}"),
                            "上移",
                            UiAction::Review(Action::Shift(i, -1)),
                        ));
                    }
                    if i + 1 < rules.len() {
                        line = line.child(button(
                            &format!("triage-rule-down-{i}"),
                            "下移",
                            UiAction::Review(Action::Shift(i, 1)),
                        ));
                    }
                    line = line.child(button(
                        &format!("triage-rule-delete-{i}"),
                        "删除",
                        UiAction::Review(Action::DeleteRule(i)),
                    ));
                }
                page = page.child(line);
            }
        }
        page
    }
    pub fn project_chart(&self, r: &Record, cx: &Context<Self>) -> Div {
        let mut chart = col().gap(px(6.)).child(headline("任务时间线", 15.));
        let data = &self.review.timeline;
        let current = if self.review.basis.is_empty() {
            "schedule"
        } else {
            &self.review.basis
        };
        let mut bar = row().flex_wrap().gap(px(4.));
        for (basis, label) in [
            ("schedule", "全部日期"),
            ("start", "开始日"),
            ("due", "截止日"),
            ("plan", "计划区间"),
        ] {
            bar = bar.child(
                self.review_button(
                    format!("timeline-{basis}"),
                    label,
                    UiAction::Review(Action::Basis(basis)),
                    cx,
                )
                .when(current == basis, |b| b.bg(rgb(SELECTED))),
            );
        }
        chart = chart.child(bar);
        if data["project"]["id"] != r.id
            || data["project"]["space"] != r.space
            || data["basis"] != current
        {
            return chart;
        }
        let date = |v: &Value| {
            v.as_str()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        };
        let bounds = date(&data["start"]).zip(date(&data["end"]));
        if let Some((start, end)) = bounds {
            chart = chart.child(text(format!("{start} 至 {end}"), 12., MUTED));
        }
        for group in ["scheduled", "undated"] {
            let Some(entries) = data[group].as_array() else {
                continue;
            };
            if group == "undated" && !entries.is_empty() {
                chart = chart.child(headline("未排期", 13.));
            }
            for entry in entries {
                let Ok(task) = serde_json::from_value::<Record>(entry["task"].clone()) else {
                    continue;
                };
                let mut line = col().gap(px(3.)).child(
                    self.button(
                        format!("timeline-task-{}-{}", task.space, task.id),
                        task.title.clone(),
                        UiAction::Review(Action::Open(task.clone())),
                        cx,
                    )
                    .justify_start()
                    .child(format!("{} · {}", task.code(), task.title)),
                );
                if let (Some((min, max)), Some(from), Some(to)) =
                    (bounds, date(&entry["start"]), date(&entry["end"]))
                {
                    let size = (max - min).num_days() as f32 + 1.;
                    line = line.child(
                        row()
                            .w_full()
                            .h(px(7.))
                            .rounded(px(3.))
                            .bg(rgb(PALE))
                            .child(div().w(relative((from - min).num_days() as f32 / size)))
                            .child(
                                div()
                                    .w(relative(((to - from).num_days() as f32 + 1.) / size))
                                    .h_full()
                                    .rounded(px(3.))
                                    .bg(rgb(if entry["overdue"] == true {
                                        ERROR
                                    } else {
                                        VIOLET
                                    })),
                            ),
                    );
                    line = line.child(text(
                        format!(
                            "{from} 至 {to}{}",
                            if entry["overdue"] == true {
                                " · 已逾期"
                            } else {
                                ""
                            }
                        ),
                        11.,
                        MUTED,
                    ));
                }
                if let Some(deps) = entry["blocked_by"].as_array().filter(|a| !a.is_empty()) {
                    line = line.child(text(
                        format!(
                            "前置任务 · {}",
                            deps.iter()
                                .filter_map(|d| d["title"].as_str())
                                .collect::<Vec<_>>()
                                .join("、")
                        ),
                        11.,
                        MUTED,
                    ));
                }
                chart = chart.child(line);
            }
        }
        chart
    }
}
