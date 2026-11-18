use super::*;

impl Workbench {
    pub(super) fn iteration_page(&self, cx: &Context<Self>) -> AnyElement {
        let mut page = col().flex_1().min_h_0().w_full().child(self.list_tabs(cx));
        let mut timeline = col()
            .id("iteration-timeline")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(22.))
            .py(px(12.));
        let mut count = 0;
        for item in self.list("iteration") {
            let Some(plan) = item.fields.get("planning") else {
                continue;
            };
            let phase = plan["phase"].as_str().unwrap_or("unscheduled");
            if !self.loc.section.is_empty() && self.loc.section != phase {
                continue;
            }
            let (status, color) = match phase {
                "active" => ("进行中", GREEN),
                "upcoming" => ("即将开始", VIOLET),
                "closed" => ("已结束", MUTED),
                "overdue" => ("待结项", ERROR),
                _ => ("待排期", DIM),
            };
            let start = plan["start"].as_str().unwrap_or("待排期");
            let end = plan["end"].as_str().unwrap_or("");
            let total = plan["total"].as_u64().unwrap_or(0);
            let completed = plan["completed"].as_u64().unwrap_or(0);
            let capacity = plan["capacity"]
                .as_u64()
                .map(|n| format!("容量 {total}/{n}"));
            let title = self
                .button(
                    format!("open-iteration-{}", item.id),
                    item.title.clone(),
                    UiAction::Open(item.clone()),
                    cx,
                )
                .w_full()
                .h(px(52.))
                .justify_start()
                .gap(px(10.))
                .child(mark(color))
                .child(headline(item.title.clone(), 14.).truncate())
                .child(div().flex_1())
                .child(chip(status, PALE, color))
                .when_some(plan["estimate"].as_f64(), |line, estimate| {
                    line.child(text(
                        format!(
                            "工作量 {}/{estimate}",
                            plan["completed_estimate"].as_f64().unwrap_or_default()
                        ),
                        11.,
                        MUTED,
                    ))
                })
                .when_some(capacity, |line, n| line.child(text(n, 11., MUTED)))
                .child(text(format!("完成 {completed}/{total}"), 11., INK));
            let dates = col()
                .w(px(94.))
                .flex_shrink_0()
                .gap(px(2.))
                .child(text(start.to_owned(), 12., INK))
                .when(!end.is_empty(), |d| {
                    d.child(text(format!("至 {end}"), 11., MUTED))
                });
            let entry = row().w_full().min_h(px(75.)).child(dates).child(
                div()
                    .min_w_0()
                    .flex_1()
                    .border_l_1()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .pl(px(10.))
                    .py(px(10.))
                    .child(title),
            );
            timeline = timeline.child(motion::layout(
                format!("iteration-{}-{}", item.space, item.id),
                self.revision,
                entry,
            ));
            count += 1;
        }
        if count == 0 {
            let create = self
                .writable()
                .then(|| ("新建迭代".into(), UiAction::Create("iteration".into())));
            page = page.child(self.empty_page(
                "calendar-range",
                "暂无符合条件的迭代",
                "设置开始、结束日期和容量后，可在时间轴查看任务范围与完成情况。",
                create,
                cx,
            ));
        } else {
            page = page.child(timeline);
        }
        page.into_any_element()
    }
}
