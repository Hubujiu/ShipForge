use crate::{Context, Result, required as req};
use serde_json::{Value, json};

/// Structured requests on the worker protocol. Ordinary text can describe a question.
pub fn requests_input(value: &Value) -> bool {
    let signal = |v: &Value| {
        v.as_str().is_some_and(|s| {
            matches!(
                s,
                "request_user_input"
                    | "ask_user"
                    | "ask_user_question"
                    | "AskUserQuestion"
                    | "permission_request"
                    | "approval_request"
                    | "request_approval"
                    | "input_required"
                    | "waiting_for_input"
                    | "waiting_for_approval"
            )
        })
    };
    value["status"] == "waiting"
        || ["type", "event", "status"]
            .iter()
            .any(|key| signal(&value[*key]))
        || [
            "question",
            "questions",
            "permission_request",
            "approval_request",
        ]
        .iter()
        .any(|key| match &value[*key] {
            Value::Object(o) => !o.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::String(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            _ => false,
        })
        || value["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|call| {
                ["name", "tool", "type"].iter().any(|k| signal(&call[*k]))
                    || signal(&call["function"]["name"])
            })
}

/// Classify only recognized provider/worker signals. Private output never enters diagnostic records.
pub fn classify(error: &Value, detail: &str) -> Value {
    let supplied = error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .or_else(|| error.as_str())
        .or_else(|| error["category"].as_str())
        .unwrap_or("");
    let code = supplied.to_ascii_lowercase();
    let status = error
        .get("http_status")
        .or_else(|| error.get("status"))
        .or_else(|| error.get("status_code"))
        .and_then(Value::as_u64)
        .filter(|s| (100..600).contains(s));
    let category = match code.as_str() {
        "unattended_input" => "unattended_input",
        "network_unavailable"
        | "connection_refused"
        | "econnrefused"
        | "enotfound"
        | "dns_error"
        | "certificate_verify_failed"
        | "api_connection_error" => "network_unavailable",
        "vision_unsupported"
        | "unsupported_image_input"
        | "vision_not_supported"
        | "image_input_not_supported" => "vision_unsupported",
        "workspace_setup" | "worktree_setup_failed" => "workspace_setup",
        "bound_directory_missing" => "bound_directory_missing",
        "authentication" | "invalid_api_key" | "authentication_error" | "invalid_token" => {
            "authentication"
        }
        "permission" | "permission_denied" | "permission_error" | "access_denied" => "permission",
        "billing_quota"
        | "insufficient_quota"
        | "billing_hard_limit_reached"
        | "credit_balance_too_low" => "billing_quota",
        "rate_limit" | "rate_limit_exceeded" | "rate_limit_error" | "too_many_requests" => {
            "rate_limit"
        }
        "overloaded" | "overloaded_error" | "server_overloaded" => "overloaded",
        "model_unavailable" | "model_not_found" | "model_not_supported" => "model_unavailable",
        "context_length" | "context_length_exceeded" | "prompt_too_long" => "context_length",
        "payload_size" | "payload_too_large" | "request_too_large" => "payload_size",
        "image_size" | "image_too_large" | "invalid_image_size" => "image_size",
        "image_format" | "invalid_image_format" | "unsupported_image" => "image_format",
        "capability" | "unsupported_parameter" | "unsupported_capability" => "capability",
        "content_policy" | "content_policy_violation" | "content_filter" => "content_policy",
        "request_timeout" | "gateway_timeout" => "request_timeout",
        "run_timeout"
        | "queue_timeout"
        | "directory_unavailable"
        | "runtime_unavailable"
        | "lease_lost"
        | "provider_limit"
        | "provider_error"
        | "runtime_error" => code.as_str(),
        _ => match status {
            Some(401) => "authentication",
            Some(403) => "permission",
            Some(408 | 504) => "request_timeout",
            Some(413) => "payload_size",
            Some(429) => "provider_limit",
            Some(503 | 529) => "overloaded",
            Some(_) => "provider_error",
            None => {
                let note = detail.to_ascii_lowercase();
                if note == "runtime timeout" {
                    "run_timeout"
                } else if note.starts_with("git worktree:") {
                    "workspace_setup"
                } else if note.starts_with("directory lock")
                    || note.contains("approved directory")
                    || note.contains("workspace profile")
                    || note.starts_with("git worktree")
                    || note.contains("direct directory execution")
                {
                    "directory_unavailable"
                } else if note.starts_with("launch ") || note.contains("runtime is not enabled") {
                    "runtime_unavailable"
                } else if note.starts_with("renew lease") {
                    "lease_lost"
                } else {
                    "runtime_error"
                }
            }
        },
    };
    let (message, guidance) = match category {
        "bound_directory_missing" => (
            "对话绑定的目录已不可用",
            "恢复机器原目录及其授权，或在新对话选择其他目录。",
        ),
        "workspace_setup" => (
            "隔离工作树准备失败",
            "确认获准路径是有提交的 Git 仓库，核对工作机磁盘与文件权限。",
        ),
        "network_unavailable" => (
            "无法连接模型服务",
            "检查工作机到服务的网络、代理、证书及服务地址。",
        ),
        "vision_unsupported" => (
            "模型无法处理图片输入",
            "选择支持视觉输入的模型，或将图片转为文字后继续。",
        ),
        "authentication" => ("服务认证失败", "在工作机检查服务账户和凭据。"),
        "permission" => ("服务拒绝访问", "核对账户对所选模型、项目及资源的权限。"),
        "billing_quota" => ("服务额度不足", "检查账单、余额及项目预算。"),
        "rate_limit" => ("请求频率受限", "降低并发，按服务返回的等待时间重试。"),
        "provider_limit" => ("服务返回 429", "核对服务错误代码，区分请求速率与账户额度。"),
        "overloaded" => ("服务繁忙", "稍后重试或切换可用的服务。"),
        "model_unavailable" => ("所选模型不可用", "核对模型名和服务账户的模型权限。"),
        "context_length" => ("模型上下文过长", "缩短历史或拆分任务。"),
        "payload_size" => ("请求体过大", "缩小上传文件或拆分请求。"),
        "image_size" => ("图片尺寸超限", "按模型要求缩小图片。"),
        "image_format" => ("图片格式不支持", "转换图片格式或选择支持视觉输入的模型。"),
        "capability" => ("模型能力不匹配", "核对模型支持的输入、工具与参数。"),
        "content_policy" => ("服务拒绝处理内容", "查看服务的内容要求并调整输入。"),
        "request_timeout" => ("上游请求超时", "查看服务状态、代理及网络延迟。"),
        "run_timeout" => ("运行已超时", "检查执行步骤及 Agent 和工作机的运行时限。"),
        "unattended_input" => (
            "无人值守运行需要人工输入",
            "调整规则指令，或在有人值守的会话中处理提问和授权。",
        ),
        "queue_timeout" => (
            "排队等待已超时",
            "检查工作时间、空闲目录、机器和 Agent 并发。",
        ),
        "directory_unavailable" => ("工作目录不可用", "检查机器的目录授权、Git 状态与占用。"),
        "runtime_unavailable" => ("运行时不可用", "在工作机检查运行时程序及配置。"),
        "lease_lost" => ("与工作机的租约已中断", "确认机器在线，核实已有产物后重试。"),
        "provider_error" => ("上游服务返回错误", "查看服务状态及此运行的 HTTP 状态码。"),
        _ => ("外部运行时执行失败", "在工作机核对运行时配置及本机诊断。"),
    };
    json!({"category":category,"message":message,"guidance":guidance,"http_status":status})
}
pub(crate) fn report(c: &Context<'_>, q: &Value) -> Result<Value> {
    let r = c.readable("run", req(q, "id")?)?;
    let f = &r.fields;
    let failure = if f["failure"].is_object() {
        classify(&f["failure"], "")
    } else if f["status"] == "failed" {
        classify(&Value::Null, f["result"].as_str().unwrap_or(""))
    } else {
        Value::Null
    };
    Ok(
        json!({"id":r.id,"status":f["status"],"failure":failure,"attempt":f["attempt"],"machine_id":f["machine_id"],"runtime":f["runtime"],"created_at":r.created_at,"started_at":f["started_at"],"finished_at":f["finished_at"],"waiting_reason":f["waiting_reason"],"next_start_at":f["next_start_at"],"queue_deadline":f["queue_deadline"],"run_deadline":f["run_deadline"]}),
    )
}
pub(crate) fn machine(c: &Context<'_>, q: &Value) -> Result<Value> {
    let m = c.readable("machine", req(q, "id")?)?;
    let f = &m.fields;
    let mut runs = Vec::new();
    for r in c.records("run")? {
        if r.fields["machine_id"] == m.id
            && matches!(r.fields["status"].as_str(), Some("running" | "cancelling"))
            && c.visible(&r)?
        {
            runs.push(json!({"id":r.id,"status":r.fields["status"],"runtime":r.fields["runtime"],"started_at":r.fields["started_at"]}));
        }
    }
    Ok(
        json!({"machine_id":m.id,"online":crate::machine_is_online(f),"last_seen":f["last_seen"],"capacity":f["capacity"],"runtimes":f["runtimes"],"visible_runs":runs}),
    )
}
