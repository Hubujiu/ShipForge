use super::*;

fn time(input: &str) -> DateTime<Utc> {
    input.parse().unwrap()
}
fn scope(store: &Store) -> Context<'_> {
    Context {
        db: &store.db,
        actor: "owner",
        space: "default",
    }
}
fn automatic(c: &Context<'_>, anchor: &str) -> Settings {
    let mut s = policy(c).unwrap().0;
    s.mode = Mode::Automatic;
    s.timezone = "America/Los_Angeles".into();
    s.anchor_date = date(anchor).unwrap();
    s.weeks = 1;
    s.cooldown_days = 2;
    s.future_count = 2;
    s
}
fn rows(c: &Context<'_>) -> Vec<Record> {
    let mut rows = c.records("iteration").unwrap();
    rows.sort_by_key(|r| {
        r.fields["start_date"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    });
    rows
}
fn task(c: &Context<'_>, id: &str, status: &str, iteration: Option<&str>) -> Record {
    c.create(
        "task",
        Some(id),
        id,
        json!({"creator":"owner","type":"task","status":status,"iteration":iteration}),
    )
    .unwrap()
}
fn stamp(c: &Context<'_>, task: &Record, at: &str) {
    c.db.execute(
        "UPDATE event_log SET at=? WHERE kind='task' AND space=? AND id=? AND rev=?",
        params![time(at).timestamp_millis(), c.space, task.id, task.rev],
    )
    .unwrap();
}
fn command(db: &mut Store, actor: &str, q: Value) -> Value {
    db.execute(actor, q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}

#[test]
fn automatic_meets_space_midnight_across_dst_carries_only_open_work_and_catches_up_idempotently() {
    let mut db = Store::memory().unwrap();
    let c = scope(&db);
    let s = automatic(&c, "2026-03-02");
    configure(
        &c,
        &policy(&c).unwrap().0,
        &s,
        1,
        time("2026-03-02T09:00:00Z"),
    )
    .unwrap();
    let list = rows(&c);
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].fields["end_date"], "2026-03-08");
    assert_eq!(list[1].fields["start_date"], "2026-03-11");
    let first = list[0].id.clone();
    let next = list[1].id.clone();
    for (id, status) in [("todo", "todo"), ("done", "done"), ("cancel", "cancelled")] {
        task(&c, id, status, Some(&first));
    }
    maintain_space(&mut db.db, "default", time("2026-03-09T06:59:59Z")).unwrap();
    assert_eq!(
        scope(&db).get("iteration", &first).unwrap().fields["status"],
        "active"
    );
    maintain_space(&mut db.db, "default", time("2026-03-09T07:00:00Z")).unwrap();
    let c = scope(&db);
    assert_eq!(
        c.get("iteration", &first).unwrap().fields["status"],
        "closed"
    );
    assert_eq!(c.get("task", "todo").unwrap().fields["iteration"], next);
    assert_eq!(c.get("task", "done").unwrap().fields["iteration"], first);
    assert_eq!(c.get("task", "cancel").unwrap().fields["iteration"], first);
    assert_eq!(
        history::read(&c, &first).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(rows(&c).iter().all(|r| r.fields["status"] != "active"));
    maintain_space(&mut db.db, "default", time("2026-04-01T12:00:00Z")).unwrap();
    let c = scope(&db);
    let list = rows(&c);
    let current = list
        .iter()
        .find(|r| r.fields["status"] == "active")
        .unwrap();
    assert_eq!(
        c.get("task", "todo").unwrap().fields["iteration"],
        current.id
    );
    let event_count: i64 =
        c.db.query_row("SELECT count(*) FROM event_log", [], |r| r.get(0))
            .unwrap();
    maintain_space(&mut db.db, "default", time("2026-04-01T22:00:00Z")).unwrap();
    assert_eq!(
        db.db
            .query_row("SELECT count(*) FROM event_log", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        event_count
    );
}

#[test]
fn enabling_rules_replays_only_real_transitions_in_the_current_or_cooling_window() {
    let db = Store::memory().unwrap();
    let c = scope(&db);
    let mut s = automatic(&c, "2026-03-02");
    s.auto_join_in_progress = true;
    s.auto_join_completed = true;
    s.auto_join_active = true;
    let before = "2026-03-01T18:00:00Z";
    let initial = task(&c, "unchanged", "in_progress", None);
    stamp(&c, &initial, before);
    let mut f = initial.fields.clone();
    f["description"] = json!("edit only");
    let edit = c.update(&initial, &initial.title, f).unwrap();
    stamp(&c, &edit, "2026-03-03T16:30:00Z");
    for (id, st) in [
        ("started", "in_progress"),
        ("finished", "done"),
        ("with-due", "backlog"),
        ("cancelled", "cancelled"),
    ] {
        let raw = task(&c, id, "backlog", None);
        stamp(&c, &raw, before);
        let mut f = raw.fields.clone();
        f["status"] = json!(st);
        if id == "with-due" {
            f["due_date"] = json!("2026-03-20");
        }
        let updated = c.update(&raw, &raw.title, f).unwrap();
        stamp(&c, &updated, "2026-03-03T17:00:00Z");
    }
    configure(
        &c,
        &policy(&c).unwrap().0,
        &s,
        1,
        time("2026-03-04T00:00:00Z"),
    )
    .unwrap();
    let list = rows(&c);
    let current = &list[0].id;
    for id in ["started", "finished", "with-due"] {
        assert_eq!(c.get("task", id).unwrap().fields["iteration"], *current);
    }
    assert!(c.get("task", "unchanged").unwrap().fields["iteration"].is_null());
    assert!(c.get("task", "cancelled").unwrap().fields["iteration"].is_null());
    let cooling = time("2026-03-10T15:00:00Z");
    cadence::maintain(
        &c,
        &s,
        cooling.with_timezone(&s.zone().unwrap()).date_naive(),
    )
    .unwrap();
    let new = task(&c, "cool", "in_progress", None);
    stamp(&c, &new, "2026-03-09T07:01:00Z");
    let old = task(&c, "before-cooling", "in_progress", None);
    stamp(&c, &old, "2026-03-09T06:59:00Z");
    history::backfill(&c, &s, cooling).unwrap();
    assert_eq!(
        c.get("task", "cool").unwrap().fields["iteration"],
        list[1].id
    );
    assert!(c.get("task", "before-cooling").unwrap().fields["iteration"].is_null());
}

#[test]
fn manual_owners_default_dates_multiple_active_iterations_and_acl_filtered_history() {
    let mut db = Store::memory().unwrap();
    for actor in ["a", "b"] {
        command(
            &mut db,
            "owner",
            json!({"op":"create","kind":"member","id":actor,"title":actor,"fields":{"actor":actor,"role":"member"}}),
        );
    }
    let q = json!({"op":"iteration.configure","fields":{"mode":"manual","weeks":3,"timezone":"Asia/Tokyo"},"expected_rev":1,"request_id":"settings-once"});
    assert!(db.execute("a", q.clone()).is_err());
    let result = command(&mut db, "owner", q.clone());
    assert_eq!(command(&mut db, "owner", q), result);
    for (id, actor) in [("a1", "a"), ("a2", "a"), ("b1", "b")] {
        let row = command(
            &mut db,
            actor,
            json!({"op":"create","kind":"iteration","id":id,"title":id}),
        );
        assert_eq!(row["fields"]["owner"], actor);
        let start = date(row["fields"]["start_date"].as_str().unwrap()).unwrap();
        assert_eq!(
            date(row["fields"]["end_date"].as_str().unwrap()).unwrap() - start,
            Duration::days(20)
        );
        command(&mut db, actor, json!({"op":"iteration.start","id":id}));
    }
    let mine = command(
        &mut db,
        "a",
        json!({"op":"iteration.timeline","owner":"@me"}),
    );
    assert_eq!(mine.as_array().unwrap().len(), 2);
    assert!(
        mine.as_array()
            .unwrap()
            .iter()
            .all(|r| r["fields"]["planning"]["phase"] == "active")
    );
    for (id, status, vis) in [("pending", "todo", "space"), ("private", "done", "private")] {
        command(
            &mut db,
            "a",
            json!({"op":"create","kind":"task","id":id,"title":id,"fields":{"status":status,"iteration":"a1","visibility":vis}}),
        );
    }
    command(
        &mut db,
        "a",
        json!({"op":"iteration.close","id":"a1","target":"a2"}),
    );
    assert_eq!(
        command(&mut db, "a", json!({"op":"iteration.history","id":"a1"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let peer = command(&mut db, "b", json!({"op":"iteration.history","id":"a1"}));
    assert_eq!(peer["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(peer["tasks"][0]["id"], "pending");
    assert!(
        db.execute(
            "a",
            json!({"op":"create","kind":"task","title":"unstarted","fields":{"iteration":"a1"}})
        )
        .is_err()
    );
    command(
        &mut db,
        "a",
        json!({"op":"create","kind":"task","id":"late","title":"late","fields":{"status":"done","iteration":"a1"}}),
    );
    assert_eq!(
        command(&mut db, "a", json!({"op":"iteration.history","id":"a1"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn public_protocol_assigns_live_changes_and_disabling_keeps_history_and_unplans_future_work() {
    let mut db = Store::memory().unwrap();
    let today = Utc::now().date_naive();
    command(
        &mut db,
        "owner",
        json!({"op":"iteration.configure","fields":{"mode":"automatic","weeks":1,"anchor_date":today,"future_count":1,"auto_join_in_progress":true}}),
    );
    let rows = command(&mut db, "owner", json!({"op":"iteration.timeline"}));
    let current = rows[1]["id"].as_str().unwrap();
    let next = rows[0]["id"].as_str().unwrap();
    let task = command(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"work","title":"Work","fields":{"status":"in_progress"}}),
    );
    assert_eq!(task["fields"]["iteration"], current);
    command(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"next","title":"Next","fields":{"iteration":next}}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"done","title":"Done","fields":{"iteration":current,"status":"done"}}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"update","kind":"space","id":"default","fields":{"iterations_enabled":false}}),
    );
    let s = command(&mut db, "owner", json!({"op":"iteration.settings"}));
    assert_eq!(s["fields"]["mode"], "disabled");
    assert_eq!(
        scope(&db).get("iteration", current).unwrap().fields["status"],
        "closed"
    );
    assert!(scope(&db).find("iteration", next).unwrap().is_none());
    for id in ["next", "work"] {
        let task = command(&mut db, "owner", json!({"op":"get","kind":"task","id":id}));
        assert!(task["fields"]["iteration"].is_null());
        assert_eq!(task["fields"]["status"], "backlog");
    }
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"iteration.history","id":current})
        )["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    command(
        &mut db,
        "owner",
        json!({"op":"update","kind":"space","id":"default","fields":{"iterations_enabled":true}}),
    );
    assert_eq!(
        command(&mut db, "owner", json!({"op":"iteration.settings"}))["fields"]["mode"],
        "manual"
    );
}

#[test]
fn deliberate_removal_and_imported_states_wait_for_a_new_qualifying_transition() {
    let mut db = Store::memory().unwrap();
    let today = Utc::now().date_naive();
    command(
        &mut db,
        "owner",
        json!({"op":"create","kind":"task","id":"deliberate","title":"Deliberate","fields":{"status":"in_progress","iteration":null}}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"import","data":[{"kind":"task","id":"source","title":"Historic source","fields":{"status":"in_progress"}}]}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"iteration.configure","fields":{"mode":"automatic","anchor_date":today,"weeks":1,"auto_join_in_progress":true}}),
    );
    let tasks = command(&mut db, "owner", json!({"op":"list","kind":"task"}));
    for row in tasks.as_array().unwrap() {
        assert!(row["fields"]["iteration"].is_null(), "{row}");
    }
    command(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"deliberate","fields":{"description":"still old"}}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"deliberate","fields":{"status":"todo"}}),
    );
    let new = command(
        &mut db,
        "owner",
        json!({"op":"update","kind":"task","id":"deliberate","fields":{"status":"in_progress"}}),
    );
    assert!(new["fields"]["iteration"].is_string());
}

#[test]
fn rescheduling_preserves_occupied_future_ids_and_advancing_the_next_restarts_the_series() {
    let mut db = Store::memory().unwrap();
    let today = Utc::now().date_naive();
    command(
        &mut db,
        "owner",
        json!({"op":"iteration.configure","fields":{"mode":"automatic","weeks":1,"anchor_date":today,"future_count":2}}),
    );
    let list = command(&mut db, "owner", json!({"op":"iteration.timeline"}));
    let active = &list[2]["id"];
    let next = &list[1]["id"];
    let extra = &list[0]["id"];
    for (id, it) in [("promised", active), ("future", next), ("extra", extra)] {
        command(
            &mut db,
            "owner",
            json!({"op":"create","kind":"task","id":id,"title":id,"fields":{"iteration":it}}),
        );
    }
    command(
        &mut db,
        "owner",
        json!({"op":"iteration.configure","fields":{"weeks":2,"cooldown_days":2,"future_count":1}}),
    );
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"get","kind":"task","id":"future"})
        )["fields"]["iteration"],
        *next
    );
    assert!(
        command(
            &mut db,
            "owner",
            json!({"op":"get","kind":"task","id":"extra"})
        )["fields"]["iteration"]
            .is_null()
    );
    assert!(
        db.execute("owner", json!({"op":"get","kind":"iteration","id":extra}))
            .is_err()
    );
    assert!(
        db.execute("owner", json!({"op":"iteration.close","id":active}))
            .is_err()
    );
    let started = command(&mut db, "owner", json!({"op":"iteration.start","id":next}));
    assert_eq!(started["fields"]["start_date"], today.to_string());
    assert_eq!(
        started["fields"]["end_date"],
        (today + Duration::days(13)).to_string()
    );
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"get","kind":"task","id":"promised"})
        )["fields"]["iteration"],
        *next
    );
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"get","kind":"iteration","id":active})
        )["fields"]["status"],
        "closed"
    );
}

#[test]
fn unjoined_organization_admin_cannot_disable_private_space_iterations_through_metadata() {
    let mut db = Store::memory().unwrap();
    command(
        &mut db,
        "owner",
        json!({"op":"create","kind":"member","id":"org-admin","title":"Organization admin","fields":{"actor":"org-admin","role":"admin"}}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"create","kind":"space","id":"private","title":"Private","fields":{"visibility":"private"}}),
    );
    command(
        &mut db,
        "owner",
        json!({"op":"iteration.configure","space":"private","fields":{"mode":"automatic"}}),
    );
    assert!(db.execute("org-admin",json!({"op":"update","kind":"space","space":"private","id":"private","fields":{"iterations_enabled":false}})).is_err());
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"iteration.settings","space":"private"})
        )["fields"]["mode"],
        "automatic"
    );
}

#[test]
fn no_precreated_next_period_releases_work_in_cooldown_and_history_is_listed_when_disabled() {
    let mut db = Store::memory().unwrap();
    let c = scope(&db);
    let mut s = automatic(&c, "2026-10-26");
    s.future_count = 0;
    configure(
        &c,
        &policy(&c).unwrap().0,
        &s,
        1,
        time("2026-10-31T20:00:00Z"),
    )
    .unwrap();
    let active = rows(&c).remove(0);
    task(&c, "open", "todo", Some(&active.id));
    maintain_space(&mut db.db, "default", time("2026-11-02T07:59:00Z")).unwrap();
    assert_eq!(
        scope(&db).get("iteration", &active.id).unwrap().fields["status"],
        "active"
    );
    maintain_space(&mut db.db, "default", time("2026-11-02T08:00:00Z")).unwrap();
    assert!(scope(&db).get("task", "open").unwrap().fields["iteration"].is_null());
    assert_eq!(
        scope(&db).get("task", "open").unwrap().fields["status"],
        "backlog"
    );
    assert_eq!(history::list(&scope(&db)).unwrap()[0]["id"], active.id);
}

#[test]
fn space_date_settings_and_iteration_timezone_follow_each_other() {
    let mut db = Store::memory().unwrap();
    command(
        &mut db,
        "owner",
        json!({"op":"iteration.configure","fields":{"mode":"manual","timezone":"Asia/Tokyo"}}),
    );
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"get","kind":"space","id":"default"})
        )["fields"]["timezone"],
        "Asia/Tokyo"
    );
    command(
        &mut db,
        "owner",
        json!({"op":"update","kind":"space","id":"default","fields":{"timezone":"America/New_York"}}),
    );
    assert_eq!(
        command(&mut db, "owner", json!({"op":"iteration.settings"}))["fields"]["timezone"],
        "America/New_York"
    );
    assert!(db.execute("owner",json!({"op":"update","kind":"space","id":"default","fields":{"timezone":"invalid/place"}})).is_err());
    assert_eq!(
        command(
            &mut db,
            "owner",
            json!({"op":"get","kind":"space","id":"default"})
        )["fields"]["timezone"],
        "America/New_York"
    );
}
