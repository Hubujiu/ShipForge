use serde_json::{Value, json};
use shipforge_client::Client;
use std::path::Path;

pub fn attachment(client: &mut Client, mut request: Value, path: &Path) -> Result<Value, String> {
    if matches!(
        request["op"].as_str(),
        Some("attachment.download" | "chat.share.image")
    ) {
        let response = client.execute(request)?;
        shipforge_client::save_attachment(path, &response)?;
        return Ok(json!({"path":path}));
    }
    let upload = shipforge_client::upload_file(path)?;
    let fields = request.as_object_mut().ok_or("invalid upload request")?;
    if let Value::Object(file) = upload {
        fields.extend(file.into_iter().filter(|(key, _)| key != "op"));
    }
    client.execute(request)
}
