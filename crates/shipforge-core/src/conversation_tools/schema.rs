use super::*;

pub(crate) fn extend(schema: &mut Value) {
    let conditions = schema["allOf"].as_array_mut().unwrap();
    let id = json!({"type":"string","minLength":1});
    let ids = json!({"type":"array","uniqueItems":true,"items":id});
    for (operations, required, properties) in [
        (
            vec![
                "document.share",
                "chat.slash",
                "chat.complete",
                "task.review.state",
                "shortcut.delete",
            ],
            vec!["id"],
            json!({}),
        ),
        (
            vec!["document.open", "document.preview"],
            vec!["link"],
            json!({"link":id}),
        ),
        (
            vec!["document.embeds"],
            vec!["kind", "id"],
            json!({"kind":{"enum":["document","task","comment"]}}),
        ),
        (vec!["shortcut.history"], vec!["task"], json!({"task":id})),
        (vec!["shortcut.run"], vec!["task", "id"], json!({"task":id})),
        (
            vec!["shortcut.organize"],
            vec!["id"],
            json!({"pinned":{"type":"boolean"},"order":ids}),
        ),
        (
            vec!["shortcut.save"],
            vec![],
            json!({"title":{"type":"string","minLength":1,"maxLength":100},"prompt":{"type":"string","maxLength":65536},"handler_kind":{"enum":["agent","crew"]},"handler_id":id,"visibility":{"enum":["private","space"]}}),
        ),
        (
            vec!["chat.slash.send"],
            vec!["id", "body"],
            json!({"body":id}),
        ),
        (vec!["task.review"], vec!["id"], json!({"body":id,"run":id})),
        (vec!["chat.share.image"], vec!["share"], json!({"share":id})),
        (
            vec!["chat.share.create"],
            vec![],
            json!({"include_questions":{"type":"boolean"}}),
        ),
        (
            vec!["run.report"],
            vec![],
            json!({"needs_review":{"type":"boolean"}}),
        ),
        (
            vec!["machine.register"],
            vec![],
            json!({"slash_commands":{"type":["object","null"],"maxProperties":64,"additionalProperties":{"type":"array","maxItems":200,"items":{"type":"string","maxLength":101}}}}),
        ),
    ] {
        let mut then = json!({"properties":properties});
        if !required.is_empty() {
            then["required"] = json!(required);
        }
        conditions.push(json!({"if":{"properties":{"op":{"enum":operations}}},"then":then}));
    }
    conditions.push(json!({"if":{"properties":{"op":{"const":"shortcut.save"}}},"then":{"anyOf":[{"required":["id"]},{"required":["title","handler_id"]}]}}));
}
