use super::{After, UiAction, Workbench};
use crate::{
    controls::{INK, MUTED, col, outlined, row, text},
    forms::{Format, Mode, Spec, item},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Map, Value, json};
#[derive(Clone)]
pub enum BotForm {
    List(Value),
    Start(&'static str),
    Qr(Value),
    Policy(Value),
    Disconnect(Value),
    Decide(Value),
}
#[derive(Clone)]
pub enum Action {
    List,
    Watch(String),
    Sync(String),
}
pub(super) enum Reply {
    List,
    Pair(Option<Value>),
    Refresh,
}
impl BotForm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::List(_) => "微信与 QQ",
            Self::Start("wechat") => "连接个人微信",
            Self::Start("qq") => "连接 QQ 机器人",
            Self::Start(_) => "使用 QQ 机器人凭据",
            Self::Qr(_) => "手机扫码授权",
            Self::Policy(_) => "接收范围",
            Self::Disconnect(_) => "解除机器人绑定",
            Self::Decide(_) => "处理回传结果",
        }
    }
    pub fn submit(&self) -> &'static str {
        match self {
            Self::List(_) => "关闭",
            Self::Start("credentials") => "验证并绑定",
            Self::Start(_) => "生成二维码",
            Self::Qr(_) => "检查授权状态",
            Self::Policy(_) => "保存范围",
            Self::Disconnect(_) => "解除绑定",
            Self::Decide(_) => "确认处理",
        }
    }
    pub fn specs(&self) -> Vec<Spec> {
        match self {
            Self::Start(provider) => {
                let mut fields = vec![item("agent", "绑定 Agent", Format::Ref("agent"), "")];
                if *provider == "credentials" {
                    fields.extend([
                        item("app_id", "QQ App ID", Format::Text, ""),
                        item("app_secret", "QQ App Secret", Format::Secret, ""),
                    ]);
                }
                fields
            }
            Self::Qr(data)
                if data["state"]
                    .as_str()
                    .is_some_and(|s| s.contains("need_verifycode")) =>
            {
                vec![item("verify_code", "手机显示的验证码", Format::Secret, "")]
            }
            Self::Policy(data) => {
                let mut fields = vec![item("dm", "接收单聊", Format::Bool, "true")];
                if data["provider"] == "qq" {
                    fields.push(item("groups", "接收群内 @机器人", Format::Bool, "true"));
                }
                fields
            }
            Self::Decide(_) => vec![item(
                "decision",
                "回传处理",
                Format::Choice(&[
                    ("confirm", "已送达"),
                    ("discard", "放弃"),
                    ("resend", "重新发送"),
                ]),
                "confirm",
            )],
            _ => vec![],
        }
    }
    pub fn prefill(&self) -> Map<String, Value> {
        if let Self::Policy(data) = self {
            data.as_object().cloned().unwrap_or_default()
        } else {
            Map::new()
        }
    }
}
fn qr_view(data: &Value) -> AnyElement {
    if let Some(bytes) = data["qr_content"]
        .as_str()
        .and_then(shipforge_client::qr_png)
    {
        return img(std::sync::Arc::new(Image::from_bytes(
            ImageFormat::Png,
            bytes,
        )))
        .size(px(220.))
        .into_any_element();
    }
    let matrix = data["qr_matrix"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if matrix.is_empty() {
        return div().into_any_element();
    }
    let unit = (245. / (matrix.len() + 8) as f32).floor().max(1.);
    let side = (matrix.len() + 8) as f32 * unit;
    canvas(
        |_, _, _| (),
        move |bounds, _, w, _| {
            w.paint_quad(fill(bounds, rgb(0xffffff)));
            for (y, row) in matrix.iter().enumerate() {
                for (x, v) in row.bytes().enumerate() {
                    if v == b'1' {
                        let origin = bounds.origin
                            + point(px((x + 4) as f32 * unit), px((y + 4) as f32 * unit));
                        w.paint_quad(fill(
                            Bounds::new(origin, size(px(unit), px(unit))),
                            rgb(0x000000),
                        ));
                    }
                }
            }
        },
    )
    .size(px(side))
    .into_any_element()
}
fn provider(v: &Value) -> &'static str {
    if v == "qq" {
        "QQ 机器人"
    } else {
        "个人微信"
    }
}
fn state(v: &Value) -> &'static str {
    match v.as_str().unwrap_or("wait") {
        "connected" => "已连接",
        "disconnected" => "已解绑",
        "replaced" => "已更换",
        "scaned" => "已扫码，等待手机确认",
        "expired" | "verify_code_blocked" => "二维码已失效，请重新生成",
        "binded_redirect" => "已存在绑定，请先检查原绑定",
        s if s.contains("need_verifycode") => "请填写手机上的验证码",
        _ => "请在手机上扫码并确认",
    }
}
impl Workbench {
    pub(super) fn bot_tick(&mut self) {
        if self.busy
            || self.bot_poll_at > std::time::Instant::now()
            || self
                .pending
                .values()
                .any(|p| matches!(p.after, After::Bot(_)))
        {
            return;
        }
        if let Some(Mode::Bot(BotForm::Qr(data))) = self.form.as_ref().map(|f| &f.mode)
            && matches!(
                data["state"].as_str(),
                Some("wait" | "scaned" | "scaned_but_redirect")
            )
        {
            self.bot_poll_at = std::time::Instant::now() + std::time::Duration::from_secs(3);
            self.send(
                json!({"op":"bot.qr.check","id":data["id"],"space":self.loc.space}),
                After::Bot(Reply::Pair(Some(data.clone()))),
                false,
            );
        }
    }
    pub(crate) fn bot_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        row().px(px(14.)).pt(px(8.)).child(
            outlined(self.button(
                "open-personal-bots",
                "微信与 QQ",
                UiAction::Bot(Action::List),
                cx,
            ))
            .child(crate::locale::label("微信与 QQ")),
        )
    }
    pub(crate) fn bot_content(&self, f: &BotForm, cx: &Context<Self>) -> AnyElement {
        let mut body = col().gap(px(8.));
        let button = |id: String, label: &'static str, a: UiAction| {
            outlined(self.button(SharedString::from(id), label, a, cx))
                .child(crate::locale::label(label))
        };
        let form = |f| UiAction::Form(Mode::Bot(f));
        match f {
            BotForm::List(data) => {
                body = body.child(
                    row()
                        .gap(px(6.))
                        .child(button(
                            "wx-qr".into(),
                            "微信扫码",
                            form(BotForm::Start("wechat")),
                        ))
                        .child(button(
                            "qq-qr".into(),
                            "QQ 扫码",
                            form(BotForm::Start("qq")),
                        ))
                        .child(button(
                            "qq-secret".into(),
                            "使用 QQ 凭据",
                            form(BotForm::Start("credentials")),
                        )),
                );
                let bindings = data["items"].as_array().into_iter().flatten();
                for b in bindings {
                    let id = b["id"].as_str().unwrap_or("");
                    let agent = b["agent"].as_str().unwrap_or("");
                    let title = self
                        .find("agent", agent)
                        .map_or(agent, |r| r.title.as_str());
                    body = body.child(text(
                        format!(
                            "{} · {} · {} · {}",
                            provider(&b["provider"]),
                            b["account"].as_str().unwrap_or(""),
                            title,
                            state(&b["state"])
                        ),
                        13.,
                        INK,
                    ));
                    if b["state"] != "connected" {
                        continue;
                    }
                    body = body.child(
                        row()
                            .gap(px(4.))
                            .child(button(
                                format!("bot-policy-{id}"),
                                "接收范围",
                                form(BotForm::Policy(b.clone())),
                            ))
                            .child(button(
                                format!("bot-watch-{id}"),
                                "启动后台接收",
                                UiAction::Bot(Action::Watch(id.into())),
                            ))
                            .child(button(
                                format!("bot-sync-{id}"),
                                "同步一次",
                                UiAction::Bot(Action::Sync(id.into())),
                            ))
                            .child(button(
                                format!("bot-unbind-{id}"),
                                "解除绑定",
                                form(BotForm::Disconnect(b.clone())),
                            )),
                    );
                    for d in b["deliveries"].as_array().into_iter().flatten() {
                        let key = d["id"].as_str().unwrap_or("");
                        let mut d = d.clone();
                        d["binding"] = b["id"].clone();
                        body = body.child(button(
                            format!("bot-delivery-{key}"),
                            "处理未确认回传",
                            form(BotForm::Decide(d)),
                        ));
                    }
                }
            }
            BotForm::Qr(q) => {
                body = body
                    .child(text(state(&q["state"]), 14., INK))
                    .child(qr_view(q));
                let kind = if q["provider"] == "qq" {
                    "qq"
                } else {
                    "wechat"
                };
                body = body.child(button(
                    "bot-new-qr".into(),
                    "重新生成二维码",
                    form(BotForm::Start(kind)),
                ));
            }
            BotForm::Disconnect(_) => {
                body = body.child(text(
                    "解绑会停止接收和回传，已有对话保存在工作区。",
                    13.,
                    MUTED,
                ));
            }
            BotForm::Decide(_) => {
                body = body.child(text(
                    "请先检查原会话中的送达情况。重新发送可能重复。",
                    13.,
                    MUTED,
                ));
            }
            _ => {}
        }
        body.into_any_element()
    }
    pub(super) fn bot_action(&mut self, a: Action, _: &mut Window, _: &mut Context<Self>) {
        match a {
            Action::List => self.send(
                json!({"op":"bot.bindings","space":self.loc.space}),
                After::Bot(Reply::List),
                false,
            ),
            Action::Sync(id) => self.send(
                json!({"op":"bot.sync","space":self.loc.space,"id":id}),
                After::Bot(Reply::Refresh),
                true,
            ),
            Action::Watch(id) => {
                match crate::machine::start_bot(self.directory.as_deref(), &self.loc.space, &id) {
                    Ok(()) => self.notify_text("已启动后台接收"),
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }
    pub(super) fn bot_submit(
        &mut self,
        f: BotForm,
        mut q: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        q["space"] = json!(self.loc.space);
        let after = match f {
            BotForm::List(_) => {
                self.close_form(w, cx);
                return Ok(());
            }
            BotForm::Start(provider) => {
                q["op"] = json!(if provider == "credentials" {
                    "bot.qq.connect"
                } else {
                    "bot.qr.start"
                });
                if provider != "credentials" {
                    q["provider"] = json!(provider);
                }
                Reply::Pair(None)
            }
            BotForm::Qr(data) => {
                q["op"] = json!("bot.qr.check");
                q["id"] = data["id"].clone();
                Reply::Pair(Some(data))
            }
            BotForm::Policy(data) => {
                q["op"] = json!("bot.policy");
                q["id"] = data["id"].clone();
                Reply::Refresh
            }
            BotForm::Disconnect(data) => {
                q["op"] = json!("bot.disconnect");
                q["id"] = data["id"].clone();
                Reply::Refresh
            }
            BotForm::Decide(data) => {
                q["op"] = json!("bot.retry");
                q["id"] = data["binding"].clone();
                q["delivery"] = data["id"].clone();
                Reply::Refresh
            }
        };
        self.send(q, After::Bot(after), true);
        Ok(())
    }
    pub(super) fn bot_reply(
        &mut self,
        a: Reply,
        data: Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bot_poll_at = std::time::Instant::now() + std::time::Duration::from_secs(3);
        if let Reply::Pair(Some(old)) = &a
            && !self
                .form
                .as_ref()
                .is_some_and(|f| matches!(&f.mode,Mode::Bot(BotForm::Qr(q)) if q["id"]==old["id"]))
        {
            return;
        }
        match a {
            Reply::List => self.open_form(Mode::Bot(BotForm::List(data)), w, cx),
            Reply::Pair(previous) if data["state"] != "connected" => {
                let mut v = previous.unwrap_or_else(|| data.clone());
                v["state"] = data["state"].clone();
                self.open_form(Mode::Bot(BotForm::Qr(v)), w, cx);
            }
            Reply::Pair(_) => {
                if let Some(id) = data["id"].as_str() {
                    self.bot_action(Action::Watch(id.to_owned()), w, cx);
                }
                self.bot_action(Action::List, w, cx);
            }
            Reply::Refresh => self.bot_action(Action::List, w, cx),
        }
    }
}
