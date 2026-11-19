use super::*;
pub(super) fn publish(host: &Host, client: &mut Client) -> Result<()> {
    host.call(client,json!({"op":"machine.runtime.publish","capabilities":crate::runtime_tools::inventory(&host.cfg,&host.dir)}))?;
    Ok(())
}
pub(super) fn poll(host: Host, job: Value) -> Result<Value> {
    let mut client = Client::from_env()?;
    let outcome = crate::runtime_tools::perform(
        &host.cfg,
        &host.dir,
        job["runtime"].as_str().ok_or("runtime missing")?,
        job["action"].as_str().ok_or("action missing")?,
    );
    publish(&host, &mut client)?;
    host.call(&mut client,json!({"op":"machine.runtime.finish","id":job["id"],"lease":job["lease"],"outcome":outcome}))
}
