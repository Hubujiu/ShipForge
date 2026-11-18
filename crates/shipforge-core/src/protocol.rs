use crate::worker;
use serde_json::{Value, json};

pub(crate) const KINDS: &[&str] = &[
    "space",
    "organization",
    "member",
    "task",
    "task_type",
    "status",
    "field",
    "tag",
    "relation",
    "relation_type",
    "view",
    "iteration",
    "project",
    "comment",
    "notification",
    "chat",
    "message",
    "document",
    "artifact",
    "agent",
    "crew",
    "skill",
    "machine",
    "run",
    "automation",
    "connector",
    "tool",
    "settings",
    "invitation",
    "note",
];
const WRITES: &[&str] = &[
    "invitation.issue",
    "invitation.copy",
    "invitation.revoke",
    "invitation.accept",
    "chat.manage",
    "chat.preferences",
    "chat.share.create",
    "chat.share.revoke",
    "onboarding.display",
    "onboarding.setup",
    "preferences.update",
    "task.move",
    "task.assign",
    "task.conversation",
    "task.command",
    "create",
    "update",
    "delete",
    "iteration.close",
    "triage",
    "follow",
    "notification.act",
    "document.restore",
    "artifact.save",
    "import",
    "provider.ingest",
    "im.bind",
    "im.reply",
    "im.unbind",
    "im.claim",
    "im.ack",
    "im.retry",
    "attachment.upload",
];
const READS: &[&str] = &[
    "invitation.list",
    "chat.info",
    "chat.invitations",
    "chat.share.read",
    "onboarding.catalog",
    "onboarding.get",
    "preferences.get",
    "schema",
    "whoami",
    "list",
    "get",
    "search",
    "space.list",
    "organization.list",
    "events",
    "view.run",
    "dashboard",
    "iteration.preview",
    "iteration.timeline",
    "task.relations",
    "project.progress",
    "document.history",
    "export",
    "import.preview",
    "provider.preview",
    "im.outbox",
    "im.threads",
    "attachment.download",
];
pub(crate) fn operation_names() -> Vec<&'static str> {
    READS
        .iter()
        .chain(WRITES)
        .chain(worker::OPS)
        .copied()
        .collect()
}
pub fn is_read_only(op: &str) -> bool {
    READS.contains(&op) || worker::READ_OPS.contains(&op)
}

/// JSON Schema for the versioned operation envelope. Operation-specific data is checked by core.
pub fn schema() -> Value {
    let mut schema = json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema","title":"ShipForge workspace request","type":"object",
        "required":["op"],
        "properties":{
            "op":{"type":"string","enum":operation_names()},
            "kind":{"type":"string","enum":KINDS},
            "space":{"type":"string","default":"default"},"id":{"type":"string","minLength":1},
            "title":{"type":"string","minLength":1},"fields":{"type":"object"},
            "expected_rev":{"type":"integer","minimum":1},"request_id":{"type":"string","minLength":1,"maxLength":160},
            "filter":{"type":"object"},"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":5000},"offset":{"type":"integer","minimum":0},"as_of":{"type":"string","format":"date"},
            "after":{"type":"integer","minimum":0},"target":{"type":["string","null"]},"version":{"type":"integer","minimum":1},
            "provider":{"type":"string"},"payload":{"type":"object"},"event_id":{"type":"string","minLength":1,"maxLength":256},
            "filename":{"type":"string","minLength":1,"maxLength":255},"media_type":{"type":"string"},"attachment_id":{"type":"string"},"content_base64":{"type":"string","contentEncoding":"base64","maxLength":5592408}
        },
        "allOf":[
           {"if":{"properties":{"op":{"enum":["list","get","create","update","delete"]}}},"then":{"required":["kind"]}},
           {"if":{"properties":{"op":{"enum":["get","update","delete","view.run","iteration.preview","iteration.close","triage","follow","document.history","document.restore","artifact.save","notification.act","task.relations","project.progress"]}}},"then":{"required":["id"]}},
           {"if":{"properties":{"op":{"const":"create"}}},"then":{"required":["title"]}},
           {"if":{"properties":{"op":{"enum":["provider.preview","provider.ingest"]}}},"then":{"required":["id","provider","payload"],"properties":{"provider":{"enum":crate::providers::NAMES}}}},
           {"if":{"properties":{"op":{"const":"attachment.upload"}}},"then":{"required":["kind","filename","content_base64"],"properties":{"kind":{"enum":["document","artifact","message","task"]}},"if":{"properties":{"kind":{"enum":["artifact","message","task"]}}},"then":{"required":["id"]}}},
           {"if":{"properties":{"op":{"const":"attachment.download"}}},"then":{"required":["kind","id","attachment_id"],"properties":{"kind":{"enum":["document","artifact","message","task"]}}}}
        ]
    });
    schema["properties"]["sidebar_visible"] = json!({"type":"boolean"});
    schema["properties"]["sidebar_expanded"] = json!({"type":"boolean"});
    schema["properties"]["work_type"] = json!({"type":"string","minLength":1,"maxLength":80});
    schema["properties"]["task_types"] = json!({"type":"array","minItems":1,"uniqueItems":true,"items":{"enum":["requirement","bug","task","story"]}});
    schema["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":"onboarding.setup"}}},"then":{"required":["organization_name","organization_slug","work_type","task_types"],"properties":{"organization_name":{"type":"string","minLength":1,"maxLength":100},"organization_slug":{"type":"string","pattern":"^[a-z0-9]+(-[a-z0-9]+)*$","maxLength":63}}}}));
    schema["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"op":{"const":"onboarding.display"}}},"then":{"anyOf":[{"required":["sidebar_visible"]},{"required":["sidebar_expanded"]}]}}));
    worker::extend_schema(&mut schema);
    crate::invitations::extend_schema(&mut schema);
    crate::chat::extend_schema(&mut schema);
    crate::im::extend_schema(&mut schema);
    crate::preferences::extend_schema(&mut schema);
    crate::task_commands::assignees::schema(&mut schema);
    schema["allOf"].as_array_mut().unwrap().extend([json!({"if":{"properties":{"op":{"const":"task.move"}}},"then":{"required":["id","target"],"properties":{"target":{"type":"string","minLength":1}}}}),json!({"if":{"properties":{"op":{"enum":["task.conversation","task.command"]}}},"then":{"required":["id"]}})]);
    schema
}
