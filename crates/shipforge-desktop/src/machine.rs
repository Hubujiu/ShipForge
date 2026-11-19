//! Invoke the packaged supervisor alongside the desktop executable. Command arguments never enter a shell.
use serde_json::{Value, json};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub fn service(
    dir: &Path,
    op: &str,
    config: Option<&Path>,
    options: &Value,
) -> Result<Value, String> {
    let mut cmd = command(&binary()?, dir);
    cmd.arg("service").arg(op).arg("--directory").arg(dir);
    if let Some(file) = config {
        cmd.arg("--config").arg(file);
    }
    if let Some(mode) = options["mode"].as_str() {
        cmd.arg("--mode").arg(mode);
    }
    for (key, name) in [
        ("keep_background", "--keep-background"),
        ("close_to_tray", "--close-to-tray"),
    ] {
        if let Some(b) = options[key].as_bool() {
            cmd.arg(name).arg(if b { "true" } else { "false" });
        }
    }
    serde_json::from_str(&output(&mut cmd)?).map_err(|e| e.to_string())
}
pub fn default_config(dir: Option<&Path>, space: &str) -> Result<PathBuf, String> {
    let root = dir
        .map(Path::to_owned)
        .map(Ok)
        .unwrap_or_else(shipforge_client::data_dir)?;
    if space == "default" {
        return Ok(root.join("machine.json"));
    }
    if space.len() > 128
        || space.is_empty()
        || !space
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err("请手动指定此空间的工作机配置路径。".into());
    }
    Ok(root.join("machines").join(format!("{space}.json")))
}
fn binary() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|e| e.to_string())
        .map(|p| {
            p.with_file_name(if cfg!(windows) {
                "shipforge-worker.exe"
            } else {
                "shipforge-worker"
            })
        })
        .and_then(|p| {
            #[cfg(test)]
            if !p.is_file()
                && p.parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "deps")
                && let Some(file) = p
                    .parent()
                    .and_then(Path::parent)
                    .zip(p.file_name())
                    .map(|(root, name)| root.join(name))
                    .filter(|candidate| candidate.is_file())
            {
                return Ok(file);
            }
            if p.is_file() {
                Ok(p)
            } else {
                Err("安装目录中未找到 shipforge-worker，请使用包含工作机程序的完整安装包。".into())
            }
        })
}
fn command(program: &Path, dir: &Path) -> Command {
    let mut c = Command::new(program);
    c.env("SHIPFORGE_DATA_DIR", dir).stdin(Stdio::null());
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    c
}
pub(crate) fn start_bot(dir: Option<&Path>, space: &str, id: &str) -> Result<(), String> {
    let dir = dir
        .map(Path::to_owned)
        .map(Ok)
        .unwrap_or_else(shipforge_client::data_dir)?;
    command(&binary()?, &dir)
        .args(["bot-sync", "--binding", id, "--space", space, "--watch"])
        .arg("--directory")
        .arg(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn output(command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}
pub fn control(
    dir: Option<&Path>,
    space: &str,
    fields: &Value,
    stop: bool,
) -> Result<Value, String> {
    let program = binary()?;
    let dir = dir
        .map(Path::to_owned)
        .map(Ok)
        .unwrap_or_else(shipforge_client::data_dir)?;
    if fields["probe"] == true {
        let raw = output(command(&program, &dir).arg("probe"))?;
        let found: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        return Ok(found);
    }
    let config = Path::new(fields["config"].as_str().unwrap_or_default());
    if fields["all"] == true && fields["service_operation"] == "desktop-exit" {
        return service(&dir, "desktop-exit", None, fields);
    }
    if !config.is_absolute() {
        return Err("需要工作机配置文件的绝对路径。".into());
    }
    if fields["local_operation"].is_string() || fields["service_operation"].is_string() {
        let options: Value = serde_json::from_slice(&fs::read(config).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if options["space"].as_str().unwrap_or("default") != space {
            return Err("所选配置属于另一个空间。".into());
        }
    }
    if let Some(op) = fields["service_operation"].as_str() {
        return service(&dir, op, Some(config), fields);
    }
    if let Some(op) = fields["local_operation"].as_str() {
        let mut cmd = command(&program, &dir);
        match op {
            "diagnose" => {
                cmd.arg("diagnose").arg("--config").arg(config);
            }
            "approve-directory" => {
                cmd.arg("approve-directory")
                    .arg("--config")
                    .arg(config)
                    .arg("--name")
                    .arg(fields["profile"].as_str().unwrap_or(""))
                    .arg("--directory")
                    .arg(fields["workspace"].as_str().unwrap_or(""));
                if fields["direct"] == true {
                    cmd.arg("--direct");
                }
            }
            _ => return Err("未知工作机操作".into()),
        }
        return serde_json::from_str(&output(&mut cmd)?).map_err(|e| e.to_string());
    }
    if stop {
        service(&dir, "stop", Some(config), &Value::Null)?;
        return Ok(json!({"message":"工作机已停止。"}));
    }
    let executable = fields["program"].as_str().unwrap_or_default().trim();
    let workspace = fields["workspace"].as_str().unwrap_or_default().trim();
    if config.exists() {
        if !executable.is_empty() || !workspace.is_empty() {
            return Err(
                "此配置文件已存在。启动现有配置时请清空外部程序与工作目录，或指定新配置文件。"
                    .into(),
            );
        }
        let existing: Value = serde_json::from_slice(&fs::read(config).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if existing["space"].as_str().unwrap_or("default") != space {
            return Err("所选配置属于另一个空间。".into());
        }
    } else {
        let mut init = command(&program, &dir);
        init.arg("init")
            .arg("--config")
            .arg(config)
            .arg("--space")
            .arg(space)
            .arg("--example");
        if !workspace.is_empty() {
            init.arg("--workspace").arg(workspace);
        }
        if !executable.is_empty() {
            if !Path::new(executable).is_absolute() {
                return Err("外部程序需要绝对路径。".into());
            }
            init.arg("--program").arg(executable).arg("--runtime").arg(
                fields["runtime"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("local-tool"),
            );
            for a in fields["args"]
                .as_array()
                .ok_or("固定参数应为 JSON 字符串数组")?
            {
                init.arg("--arg")
                    .arg(a.as_str().ok_or("固定参数应为字符串")?);
            }
        }
        output(&mut init)?;
    }
    let live = service(&dir, "start", Some(config), &Value::Null)?;
    Ok(
        json!({"message":if live["status"]=="online"{"工作机已上线。"}else{"工作机正在注册。"},"service":live}),
    )
}
