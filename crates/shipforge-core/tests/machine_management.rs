use serde_json::{Value, json};
use shipforge_core::Store;
const KEY: &str = "machine-management-key-for-isolated-capability-tests";
fn agent(s: &mut Store, mut q: Value) -> shipforge_core::Result<Value> {
    q["machine_id"] = json!("test-box");
    q["machine_key"] = json!(KEY);
    s.execute("owner", q)
}
#[test]
fn owners_request_and_workers_lease_approved_runtime_actions() {
    let mut s = Store::memory().unwrap();
    s.execute("owner",json!({"op":"create","kind":"member","id":"second","title":"Second","fields":{"role":"member"}})).unwrap();
    agent(
        &mut s,
        json!({"op":"machine.register","title":"Isolated","shared":true,"runtimes":[]}),
    )
    .unwrap();
    let cap = json!({"name":"test-model","status":"not_detected","installable":true,"upgradable":true,"testable":true,"program":"private/local/path","environment":"sensitive"});
    agent(
        &mut s,
        json!({"op":"machine.runtime.publish","capabilities":[cap]}),
    )
    .unwrap();
    let list = json!({"op":"machine.runtimes","id":"test-box"});
    let shared = s.execute("second", list.clone()).unwrap();
    assert_eq!(shared["can_manage"], false);
    assert!(!shared.to_string().contains("sensitive"));
    assert!(!shared.to_string().contains("private/local/path"));
    let q = json!({"op":"machine.runtime.request","id":"test-box","runtime":"test-model","action":"install","request_id":"owned-install"});
    assert!(s.execute("second", q.clone()).is_err());
    assert!(s.execute("owner",json!({"op":"machine.runtime.request","id":"test-box","runtime":"arbitrary-program","action":"install"})).is_err());
    s.execute("owner", q.clone()).unwrap();
    let mut other = q.clone();
    other["request_id"] = json!("another");
    assert!(s.execute("owner", other).is_err());
    let job = agent(&mut s, json!({"op":"machine.runtime.jobs"})).unwrap();
    let running = s.execute("owner", list.clone()).unwrap();
    assert_eq!(running["runtimes"][0]["status"], "installing");
    assert!(
        s.execute(
            "owner",
            json!({"op":"machine.runtime.jobs","machine_id":"test-box","machine_key":"wrong"})
        )
        .is_err()
    );
    let mut cap = cap;
    cap["status"] = json!("available");
    cap["version"] = json!("2.1.0");
    agent(
        &mut s,
        json!({"op":"machine.runtime.publish","capabilities":[cap]}),
    )
    .unwrap();
    let finish = json!({"op":"machine.runtime.finish","id":"owned-install","lease":job["lease"],"outcome":"success"});
    let mut bad = finish.clone();
    bad["lease"] = json!("wrong");
    assert!(agent(&mut s, bad).is_err());
    agent(&mut s, finish.clone()).unwrap();
    agent(&mut s, finish).unwrap();
    assert_eq!(s.execute("owner", q.clone()).unwrap()["duplicate"], true);
    assert!(agent(&mut s,json!({"op":"machine.runtime.finish","id":"owned-install","lease":job["lease"],"outcome":"failed"})).is_err());
    agent(&mut s, json!({"op":"machine.stopped"})).unwrap();
    assert_eq!(
        s.execute("owner", list).unwrap()["runtimes"][0]["status"],
        "offline"
    );
    assert_eq!(s.execute("owner", q).unwrap()["duplicate"], true);
    assert!(s.execute("owner",json!({"op":"machine.runtime.request","id":"test-box","runtime":"test-model","action":"test"})).is_err());
}
