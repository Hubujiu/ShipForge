use base64::{Engine as _,engine::general_purpose::STANDARD};
use serde_json::{json,Value};
use shipforge_client::Client;
use std::{fs,io::{Read,Write},path::Path};
const LIMIT:usize=4*1024*1024;
pub fn attachment(client:&mut Client,mut request:Value,path:&Path)->Result<Value,String>{
    if request["op"]=="attachment.download"{
        let value=client.execute(request)?;
        let bytes=STANDARD.decode(value["content_base64"].as_str().ok_or("下载结果不含文件内容")?).map_err(|e|e.to_string())?;
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|format!("保存 {} 失败：{e}",path.display()))?;
        if let Err(e)=file.write_all(&bytes).and_then(|()|file.sync_all()){drop(file);let _=fs::remove_file(path);return Err(e.to_string());}
        return Ok(json!({"bytes":bytes.len(),"path":path}));
    }
    let mut bytes=Vec::new();
    fs::File::open(path).map_err(|e|format!("打开 {} 失败：{e}",path.display()))?.take(LIMIT as u64+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len()>LIMIT{return Err("文件超过 4 MiB 上传上限。".into());}
    request["filename"]=json!(path.file_name().and_then(|s|s.to_str()).ok_or("文件名不能作为 Unicode 文本读取")?);
    request["content_base64"]=json!(STANDARD.encode(bytes));
    client.execute(request)
}
