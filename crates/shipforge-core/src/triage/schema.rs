use serde_json::{Value, json};

pub(crate) fn extend(schema: &mut Value) {
    let ids = json!({"type":"array","items":{"type":"string","minLength":1},"uniqueItems":true});
    let priority = json!({"enum":["none","low","medium","high","urgent"]});
    let date = json!({"type":"string","format":"date"});
    let optional_date = json!({"type":["string","null"],"format":"date"});
    let rule = json!({"type":"object","additionalProperties":false,"required":["id","when","set"],"properties":{
        "id":{"type":"string","minLength":1},"title":{"type":"string"},"enabled":{"type":"boolean"},
        "when":{"type":"object","additionalProperties":false,"properties":{"status":{"type":"string"},"priority":priority,"assignee":{"type":"string"},"creator":{"type":"string"},"project":{"type":"string"},"labels":ids,"type":{"type":"string"},"source":{"type":"string"},"due_before":date,"due_after":date,"created_after":{"type":"integer"},"created_before":{"type":"integer"},"contains":{"type":"string"}}},
        "set":{"type":"object","minProperties":1,"additionalProperties":false,"properties":{"space":{"type":"string"},"status":{"type":"string"},"assignee":{"type":["string","null"]},"labels":ids,"project":{"type":["string","null"]},"priority":priority}}
    }});
    let mut decision = json!({"decision":{"enum":["accept","reject","duplicate","defer","restore"]},"until":{"type":"integer"},"target":{"type":"string"},"target_space":{"type":"string"},"reason":{"type":"string"},"comment":{"type":"string"},"subscribe":{"type":"boolean"},"priority":priority,"assignees":ids,"executor":{"type":["object","null"]},"project":{"type":["string","null"]},"labels":ids,"start_date":date,"due_date":date});
    let mut constraints = vec![
        json!({"if":{"required":["kind"],"properties":{"op":{"enum":["create","update"]},"kind":{"enum":["task","project"]}}},"then":{"properties":{"fields":{"type":"object","properties":{"start_date":optional_date,"due_date":optional_date,"planned_start":optional_date,"planned_end":optional_date}}}}}),
        json!({"if":{"required":["kind"],"properties":{"op":{"enum":["create","update"]},"kind":{"const":"project"}}},"then":{"properties":{"fields":{"type":"object","properties":{"owner":{"type":["string","null"]},"owner_kind":{"enum":["member","agent"]},"status":{"enum":["draft","planned","active","paused","done","cancelled"]},"priority":priority,"spaces":ids,"primary_space":{"type":["string","null"]}}}}}}),
        json!({"if":{"properties":{"op":{"const":"customer_request.convert"}}},"then":{"required":["id"],"properties":{"type":{"type":"string"},"custom":{"type":"object"}}}}),
        json!({"if":{"properties":{"op":{"const":"project.timeline"}}},"then":{"required":["id"],"properties":{"basis":{"enum":["schedule","plan","start","due"]},"as_of":date}}}),
        json!({"if":{"properties":{"op":{"const":"triage.configure"}}},"then":{"properties":{"enabled":{"type":"boolean"},"priority_required":{"type":"boolean"},"types":{"oneOf":[ids,{"type":"null"}]},"sources":{"type":["array","null"],"uniqueItems":true,"items":{"enum":["guest","external"]}},"duty":{"type":"object","additionalProperties":false,"required":["mode"],"properties":{"mode":{"enum":["none","notify","assign"]},"member":{"type":"string"}},"if":{"properties":{"mode":{"enum":["notify","assign"]}}},"then":{"required":["member"]}},"rules":{"type":"array","maxItems":50,"items":rule},"confirm":{"type":"boolean"}}}}),
        json!({"if":{"properties":{"op":{"const":"triage.queue"}}},"then":{"properties":{"order":{"enum":["oldest","newest"]},"include_deferred":{"type":"boolean"}}}}),
        json!({"if":{"properties":{"op":{"const":"triage.submit"}}},"then":{"required":["title"],"properties":{"type":{"type":"string"},"priority":priority,"custom":{"type":"object"},"description":{"type":"string"}}}}),
        json!({"if":{"properties":{"op":{"const":"task.share"}}},"then":{"required":["id"]}}),
        json!({"if":{"properties":{"op":{"const":"task.open"}}},"then":{"required":["link"],"properties":{"link":{"type":"string","pattern":"^shipforge://task/"}}}}),
        json!({"if":{"properties":{"op":{"const":"triage"}}},"then":{"required":["id","decision"],"properties":decision}}),
        json!({"if":{"required":["decision"],"properties":{"op":{"enum":["triage","triage.batch"]},"decision":{"const":"defer"}}},"then":{"required":["until"]}}),
        json!({"if":{"required":["decision"],"properties":{"op":{"const":"triage"},"decision":{"const":"duplicate"}}},"then":{"required":["target"]}}),
    ];
    decision["decision"] = json!({"enum":["accept","reject","defer"]});
    decision["tasks"] = json!({"type":"array","minItems":1,"maxItems":500,"uniqueItems":true,"items":{"type":"string"}});
    decision["revisions"] =
        json!({"type":"object","additionalProperties":{"type":"integer","minimum":1}});
    constraints.push(json!({"if":{"properties":{"op":{"const":"triage.batch"}}},"then":{"required":["tasks","decision"],"properties":decision}}));
    schema["allOf"].as_array_mut().unwrap().extend(constraints);
}
