use serde_json::{Value, json};
use shipforge_core::Store;
use std::time::{SystemTime, UNIX_EPOCH};

const SECRET: &str = "storage-policy-synthetic-machine-secret-qa-only";
fn owner(db: &mut Store, q: Value) -> Value {
    db.execute("owner", q.clone())
        .unwrap_or_else(|e| panic!("{q}: {e}"))
}
fn host(db: &mut Store, mut q: Value) -> shipforge_core::Result<Value> {
    q["machine_id"] = json!("box");
    q["machine_key"] = json!(SECRET);
    db.execute("owner", q)
}
fn init(db: &mut Store) {
    host(
        db,
        json!({"op":"machine.register","runtimes":["qa"],"title":"Box","shared":true}),
    )
    .unwrap();
    owner(
        db,
        json!({"op":"create","kind":"member","id":"peer","title":"Peer","fields":{"role":"member"}}),
    );
}
fn read(db: &mut Store) -> Value {
    owner(db, json!({"op":"machine.storage.preview","id":"box"}))
}
fn configure(db: &mut Store, policy: Value) {
    let rev = read(db)["policy_rev"].clone();
    owner(
        db,
        json!({"op":"machine.storage.configure","id":"box","expected_rev":rev,"policy":policy}),
    );
}
fn take(db: &mut Store) -> Value {
    host(db, json!({"op":"machine.storage.take","policy_version":1})).unwrap()
}
fn finish(db: &mut Store, job: &Value, outcome: &str, listing: &Value) {
    host(db,json!({"op":"machine.storage.finish","id":job["id"],"lease":job["lease"],"outcome":outcome,"inventory":listing})).unwrap();
}
fn cache(symbol: &str, at: i64) -> Value {
    json!({"id":symbol.repeat(64),"snapshot":"e".repeat(64),"category":"cache","bytes":128,"runs":[],"last_used_at":at,"safe_for_schedule":true})
}
fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
#[test]
fn quota_monitoring_is_read_only_until_enabled_and_checks_age_pins_and_revision_again() {
    let mut db = Store::memory().unwrap();
    init(&mut db);
    assert_eq!(read(&mut db)["policy"]["enabled"], false);
    assert!(take(&mut db).is_null());
    assert!(db.execute("peer",json!({"op":"machine.storage.configure","id":"box","expected_rev":0,"policy":{"enabled":true}})).is_err());
    configure(
        &mut db,
        json!({"quota_gib":1,"min_free_gib":2,"cache_retention_days":2}),
    );
    assert!(db.execute("owner",json!({"op":"machine.storage.configure","id":"box","expected_rev":0,"policy":{"enabled":true}})).is_err());
    assert!(
        host(&mut db, json!({"op":"machine.storage.take"}))
            .unwrap()
            .is_null()
    );
    let now = timestamp();
    let first = cache("a", now - 5 * 86_400_000);
    let second = cache("b", now - 4 * 86_400_000);
    let fresh = cache("c", now);
    let listing = json!({"entries":[second,first,fresh],"total_bytes":1_010_000_000_i64,"available_bytes":65536});
    let scan = take(&mut db);
    assert_eq!(scan["command"]["action"], "scan");
    finish(&mut db, &scan, "success", &listing);
    let preview = read(&mut db);
    assert_eq!(preview["health"]["state"], "quota_and_low_disk");
    assert_eq!(preview["policy_eligible_bytes"], 256);
    assert!(take(&mut db).is_null());
    owner(
        &mut db,
        json!({"op":"machine.storage.pin","id":"box","workspace":first["id"],"pinned":true}),
    );
    configure(&mut db, json!({"enabled":true}));
    let scan = take(&mut db);
    finish(&mut db, &scan, "success", &listing);
    let job = take(&mut db);
    assert_eq!(job["command"]["workspace"], second["id"]);
    let reserve = json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]});
    configure(
        &mut db,
        json!({"enabled":false,"min_free_gib":0,"quota_gib":0}),
    );
    assert!(host(&mut db, reserve).is_err());
    finish(&mut db, &job, "conflict", &listing);
    assert!(take(&mut db).is_null());
    owner(
        &mut db,
        json!({"op":"machine.storage.pin","id":"box","workspace":first["id"],"pinned":false}),
    );
    configure(&mut db, json!({"enabled":true}));
    let scan = take(&mut db);
    finish(&mut db, &scan, "success", &listing);
    let job = take(&mut db);
    assert_eq!(job["command"]["workspace"], first["id"]);
    host(
        &mut db,
        json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]}),
    )
    .unwrap();
    let remaining = json!({"entries":[second,fresh],"total_bytes":400,"available_bytes":99999});
    finish(&mut db, &job, "success", &remaining);
    let job = take(&mut db);
    assert_eq!(job["command"]["workspace"], second["id"]);
    owner(
        &mut db,
        json!({"op":"machine.storage.pin","id":"box","workspace":second["id"],"pinned":true}),
    );
    assert!(
        host(
            &mut db,
            json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]})
        )
        .is_err()
    );
    finish(&mut db, &job, "conflict", &remaining);
    assert!(take(&mut db).is_null());
}

#[test]
fn a_delivery_appearing_after_confirmation_protects_terminal_work() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("fixture.sqlite3");
    let mut db = Store::open(&file).unwrap();
    init(&mut db);
    owner(
        &mut db,
        json!({"op":"create","kind":"agent","id":"a","title":"Agent","fields":{"machine_id":"box","runtime":"qa"}}),
    );
    let run = owner(
        &mut db,
        json!({"op":"run.enqueue","handler_id":"a","instruction":"Finish"}),
    );
    let claim = host(&mut db, json!({"op":"run.claim"})).unwrap();
    host(&mut db,json!({"op":"run.report","id":run["id"],"lease":claim["lease"],"status":"done","result":"ok","artifacts":[]})).unwrap();
    configure(&mut db, json!({"enabled":true,"retention_days":0}));
    let raw = rusqlite::Connection::open(&file).unwrap();
    let id = run["id"].as_str().unwrap();
    let old = timestamp() - 2 * 3_600_000;
    raw.execute(
        "UPDATE records SET fields=json_set(fields,'$.finished_at',?) WHERE kind='run' AND id=?",
        rusqlite::params![old, id],
    )
    .unwrap();
    let mut entry = cache("a", old);
    entry["category"] = json!("run");
    entry["runs"] = json!([id]);
    let listing = json!({"entries":[entry]});
    let scan = take(&mut db);
    finish(&mut db, &scan, "success", &listing);
    let job = take(&mut db);
    assert_eq!(job["command"]["action"], "clean");
    raw.execute("INSERT INTO sf_im_outbox(id,space,connector,route,task,kind,source,rev,body) VALUES('outbound','default','synthetic','route','task','run',?,1,'{}')",[id]).unwrap();
    assert_eq!(read(&mut db)["entries"][0]["protection"], "pending");
    assert!(
        host(
            &mut db,
            json!({"op":"machine.storage.reserve","id":job["id"],"lease":job["lease"]})
        )
        .is_err()
    );
    finish(&mut db, &job, "conflict", &listing);
    raw.execute(
        "UPDATE sf_im_outbox SET state='sent' WHERE id='outbound'",
        [],
    )
    .unwrap();
    assert!(read(&mut db)["entries"][0]["protection"].is_null());
    owner(
        &mut db,
        json!({"op":"create","kind":"task","id":"unfinished","title":"Keep this work","fields":{"status":"todo"}}),
    );
    owner(
        &mut db,
        json!({"op":"create","kind":"chat","id":"synthetic-room","title":"Continue here","fields":{"agent":"a"}}),
    );
    raw.execute("UPDATE records SET fields=json_set(fields,'$.task_id','unfinished') WHERE kind='run' AND id=?",[id]).unwrap();
    assert_eq!(read(&mut db)["entries"][0]["protection"], "open_task");
    raw.execute("UPDATE records SET fields=json_set(fields,'$.chat_id','synthetic-room') WHERE kind='run' AND id=?",[id]).unwrap();
    assert_eq!(read(&mut db)["entries"][0]["protection"], "conversation");
}

#[test]
fn pressure_alert_is_owner_only_and_reopens_the_correct_machine_once_per_episode() {
    let mut db = Store::memory().unwrap();
    init(&mut db);
    configure(&mut db, json!({"min_free_gib":1}));
    let report = |db: &mut Store, hour: u8, free: u64| {
        owner(
            db,
            json!({"op":"machine.storage.request","id":"box","action":"scan"}),
        );
        let scan = take(db);
        finish(
            db,
            &scan,
            "success",
            &json!({"entries":[],"available_bytes":free,"local_hour":hour}),
        );
    };
    let notices = |db: &mut Store, actor: &str| -> Vec<Value> {
        db.execute(actor, json!({"op":"list","kind":"notification"}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["fields"]["kind"] == "storage_pressure")
            .cloned()
            .collect()
    };
    report(&mut db, 2, 2_000);
    assert!(
        read(&mut db)["health"]["disk_pressure"] == true && notices(&mut db, "owner").is_empty()
    );
    report(&mut db, 9, 2_000);
    report(&mut db, 9, 3_000);
    let warnings = notices(&mut db, "owner");
    assert_eq!(warnings.len(), 1);
    assert!(notices(&mut db, "peer").is_empty());
    let opened = owner(&mut db, json!({"op":"inbox.open","id":warnings[0]["id"]}));
    assert_eq!(opened["target"]["id"], "box");
    assert_eq!(opened["storage"], true);
    report(&mut db, 9, 2_000_000_000);
    report(&mut db, 9, 2_000);
    assert_eq!(notices(&mut db, "owner").len(), 2);
}
