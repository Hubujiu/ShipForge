use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use shipforge_core::Store;

fn ok(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn add(db: &mut Store, kind: &str, id: &str, f: Value) -> Value {
    ok(
        db,
        "owner",
        json!({"op":"create","kind":kind,"id":id,"title":id,"fields":f}),
    )
}

#[test]
fn iteration_timeline_tracks_dates_capacity_and_visible_scope() {
    let mut db = Store::memory().unwrap();
    add(
        &mut db,
        "member",
        "reader",
        json!({"role":"viewer","actor":"reader"}),
    );
    add(
        &mut db,
        "iteration",
        "spring",
        json!({"start_date":"2027-03-01","end_date":"2027-03-30","capacity":4}),
    );
    add(
        &mut db,
        "iteration",
        "april",
        json!({"start":"2027-04-01","end":"2027-04-20"}),
    );
    add(
        &mut db,
        "task",
        "shared",
        json!({"iteration":"spring","status":"done"}),
    );
    add(
        &mut db,
        "task",
        "personal",
        json!({"iteration":"spring","visibility":"private"}),
    );
    let query = json!({"op":"iteration.timeline","as_of":"2027-03-10"});
    let rows = ok(&mut db, "owner", query.clone());
    assert_eq!(rows[0]["id"], "april");
    assert_eq!(rows[0]["fields"]["planning"]["phase"], "upcoming");
    let plan = &rows[1]["fields"]["planning"];
    assert_eq!(plan["phase"], "active");
    assert_eq!(plan["capacity"], 4);
    assert_eq!(plan["completed"], 1);
    assert_eq!(plan["total"], 2);
    assert_eq!(
        ok(&mut db, "reader", query)[1]["fields"]["planning"]["total"],
        1
    );
    assert_eq!(
        ok(
            &mut db,
            "owner",
            json!({"op":"iteration.timeline","as_of":"2027-04-02"})
        )[1]["fields"]["planning"]["phase"],
        "overdue"
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"update","kind":"iteration","id":"spring","fields":{"capacity":-1}})
        )
        .is_err()
    );
    assert!(
        db.execute(
            "owner",
            json!({"op":"iteration.timeline","as_of":"March 1"})
        )
        .is_err()
    );
}
#[test]
fn attachments_follow_current_and_historical_ownership_without_exposing_content_on_lists() {
    let mut db = Store::memory().unwrap();
    add(&mut db, "member", "a", json!({"role":"member"}));
    add(&mut db, "member", "b", json!({"role":"member"}));
    let new = json!({"op":"attachment.upload","kind":"document","filename":"屏幕抓图.png","content_base64":"AP+A","media_type":"image/png","request_id":"upload-once"});
    let doc = ok(&mut db, "a", new.clone());
    assert_eq!(doc["rev"], 1);
    assert_eq!(doc["fields"]["attachments"][0]["bytes"], 3);
    assert_eq!(doc, ok(&mut db, "a", new.clone()));
    let mut other = new;
    other["content_base64"] = json!("Yg==");
    assert!(db.execute("a", other).unwrap_err().starts_with("conflict:"));
    let id = &doc["id"];
    let file = &doc["fields"]["attachments"][0]["id"];
    let mut get =
        json!({"op":"attachment.download","kind":"document","id":id,"attachment_id":file});
    assert_eq!(ok(&mut db, "a", get.clone())["content_base64"], "AP+A");
    assert!(db.execute("b", get.clone()).is_err());
    assert!(
        !ok(&mut db, "a", json!({"op":"list","kind":"document"}))
            .to_string()
            .contains("AP+A")
    );
    ok(
        &mut db,
        "a",
        json!({"op":"update","kind":"document","id":id,"fields":{"visibility":"space"}}),
    );
    assert_eq!(ok(&mut db, "b", get.clone())["bytes"], 3);
    get["version"] = json!(1);
    assert!(db.execute("b", get.clone()).is_err());
    assert_eq!(ok(&mut db, "a", get)["content_base64"], "AP+A");
    let forged = json!({"op":"create","kind":"document","title":"foreign","fields":{"attachments":[{"id":file}]}});
    assert!(db.execute("b", forged).is_err());
    assert_eq!(
        ok(&mut db, "b", json!({"op":"search","query":"抓图"}))[0]["id"],
        *id
    );
    let over = STANDARD.encode(vec![0; 4 * 1024 * 1024 + 1]);
    assert!(db.execute("a",json!({"op":"attachment.upload","kind":"document","id":id,"filename":"oversized","content_base64":over})).is_err());
    assert!(db.execute("a",json!({"op":"attachment.upload","kind":"document","id":id,"filename":"../bad","content_base64":""})).is_err());
}
#[test]
fn saved_artifact_keeps_its_actual_attachment_after_sources_are_removed() {
    let mut db = Store::memory().unwrap();
    add(&mut db, "chat", "c", json!({}));
    add(&mut db, "message", "m", json!({"chat":"c","body":"Output"}));
    add(
        &mut db,
        "artifact",
        "a",
        json!({"chat":"c","message":"m","body":"Text report"}),
    );
    let artifact = ok(
        &mut db,
        "owner",
        json!({"op":"attachment.upload","kind":"artifact","id":"a","expected_rev":1,"filename":"output.bin","content_base64":"AAE="}),
    );
    let file = artifact["fields"]["attachments"][0]["id"].clone();
    let doc = ok(&mut db, "owner", json!({"op":"artifact.save","id":"a"}));
    assert_eq!(
        doc["fields"]["attachments"],
        artifact["fields"]["attachments"]
    );
    ok(
        &mut db,
        "owner",
        json!({"op":"delete","kind":"artifact","id":"a"}),
    );
    assert_eq!(
        ok(
            &mut db,
            "owner",
            json!({"op":"attachment.download","kind":"document","id":doc["id"],"attachment_id":file})
        )["content_base64"],
        "AAE="
    );
    let data = ok(&mut db, "owner", json!({"op":"export"}));
    assert!(!data.to_string().contains("AAE="));
    let mut imported = Store::memory().unwrap();
    ok(&mut imported, "owner", json!({"op":"import","data":data}));
    let copy = ok(
        &mut imported,
        "owner",
        json!({"op":"get","kind":"document","id":doc["id"]}),
    );
    assert_eq!(
        copy["fields"]["source_attachments"][0]["filename"],
        "output.bin"
    );
    assert!(copy["fields"]["attachments"].is_null());
}
#[test]
fn linked_projects_publish_metadata_and_count_only_authorized_tasks() {
    let mut db = Store::memory().unwrap();
    add(&mut db, "member", "p", json!({"role":"member"}));
    add(&mut db, "space", "anchor", json!({"visibility":"private"}));
    add(
        &mut db,
        "space",
        "participant",
        json!({"visibility":"private"}),
    );
    ok(
        &mut db,
        "owner",
        json!({"op":"create","space":"participant","kind":"member","id":"p","title":"p","fields":{"role":"member"}}),
    );
    ok(
        &mut db,
        "owner",
        json!({"op":"create","space":"anchor","kind":"project","id":"p1","title":"Release plan","fields":{"spaces":["participant"]}}),
    );
    ok(
        &mut db,
        "owner",
        json!({"op":"create","space":"anchor","kind":"project","id":"secret","title":"Hidden","fields":{"spaces":["participant"],"visibility":"private"}}),
    );
    for id in ["it1", "it2"] {
        ok(
            &mut db,
            "p",
            json!({"op":"create","space":"participant","kind":"iteration","id":id,"title":id}),
        );
    }
    ok(
        &mut db,
        "owner",
        json!({"op":"create","space":"anchor","kind":"task","title":"Anchor-only task","fields":{"project":"p1"}}),
    );
    ok(
        &mut db,
        "p",
        json!({"op":"create","space":"participant","kind":"task","id":"p-task","title":"Participant task","fields":{"project":"p1","iteration":"it1","status":"done"}}),
    );
    let q = json!({"op":"project.progress","space":"participant","id":"p1"});
    assert_eq!(ok(&mut db, "p", q.clone())["total"], 1);
    assert_eq!(ok(&mut db, "owner", q.clone())["total"], 2);
    assert_eq!(
        ok(
            &mut db,
            "p",
            json!({"op":"get","space":"participant","kind":"project","id":"p1"})
        )["title"],
        "Release plan"
    );
    assert_eq!(
        ok(
            &mut db,
            "p",
            json!({"op":"list","space":"participant","kind":"project"})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert!(
        db.execute(
            "p",
            json!({"op":"get","space":"participant","kind":"project","id":"secret"})
        )
        .is_err()
    );
    ok(
        &mut db,
        "p",
        json!({"op":"update","space":"participant","kind":"task","id":"p-task","fields":{"iteration":"it2"}}),
    );
    assert_eq!(ok(&mut db, "p", q.clone())["completed"], 1);
    let unlink =
        json!({"op":"update","space":"anchor","kind":"project","id":"p1","fields":{"spaces":[]}});
    assert!(
        db.execute("owner", unlink.clone())
            .unwrap_err()
            .starts_with("conflict:")
    );
    ok(
        &mut db,
        "p",
        json!({"op":"update","space":"participant","kind":"task","id":"p-task","fields":{"project":null}}),
    );
    assert_eq!(ok(&mut db, "p", q.clone())["total"], 0);
    ok(&mut db, "owner", unlink);
    assert!(db.execute("p", q).is_err());
}
#[test]
fn triage_source_type_and_planning_checks_apply_on_first_write() {
    let mut db = Store::memory().unwrap();
    add(&mut db, "member", "g", json!({"role":"guest"}));
    add(&mut db, "iteration", "s", json!({}));
    let config = json!({"op":"update","kind":"space","id":"default","fields":{"triage_enabled":true,"triage_priority_required":true,"triage_sources":["guest","external"],"triage_types":["bug"]}});
    ok(&mut db, "owner", config.clone());
    assert!(
        db.execute(
            "g",
            json!({"op":"create","kind":"task","title":"Request","fields":{"type":"task"}})
        )
        .is_err()
    );
    assert!(db.execute("owner",json!({"op":"create","kind":"task","title":"Bug","fields":{"type":"bug","source":"integration","iteration":"s"}})).is_err());
    let t = ok(
        &mut db,
        "g",
        json!({"op":"create","kind":"task","title":"Bug","fields":{"type":"bug","status":"done"}}),
    );
    assert_eq!(t["fields"]["status"], "triage");
    assert_eq!(t["fields"]["source"], "guest");
    let member = add(&mut db, "task", "local", json!({"type":"bug"}));
    assert_eq!(member["fields"]["status"], "todo");
    let excluded = add(
        &mut db,
        "task",
        "external",
        json!({"type":"task","source":"integration"}),
    );
    assert_eq!(excluded["fields"]["status"], "todo");
    assert!(
        db.execute(
            "owner",
            json!({"op":"update","kind":"task","id":t["id"],"fields":{"iteration":"s"}})
        )
        .is_err()
    );
    ok(
        &mut db,
        "owner",
        json!({"op":"triage","id":t["id"],"decision":"reject"}),
    );
    let mut config = config;
    config["fields"]["triage_sources"] = json!(["external"]);
    ok(&mut db, "owner", config);
    assert!(
        db.execute(
            "g",
            json!({"op":"create","kind":"task","title":"Bug","fields":{"type":"bug"}})
        )
        .is_err()
    );
}
#[test]
fn following_is_rendered_per_actor_and_excluded_from_exports() {
    let mut db = Store::memory().unwrap();
    add(&mut db, "member", "p", json!({"role":"member"}));
    add(&mut db, "task", "t", json!({}));
    let created = ok(&mut db, "owner", json!({"op":"get","kind":"task","id":"t"}));
    assert_eq!(created["fields"]["following"], true);
    ok(
        &mut db,
        "owner",
        json!({"op":"follow","id":"t","enabled":false}),
    );
    ok(&mut db, "p", json!({"op":"follow","id":"t","enabled":true}));
    let a = ok(&mut db, "p", json!({"op":"get","kind":"task","id":"SF-1"}));
    let b = ok(&mut db, "owner", json!({"op":"list","kind":"task"}));
    assert_eq!(a["fields"]["following"], true);
    assert_eq!(b[0]["fields"]["following"], false);
    let x = ok(&mut db, "p", json!({"op":"export"}));
    assert!(x["records"][0]["fields"].get("following").is_none());
}
