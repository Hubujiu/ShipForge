//! Invoke the packaged supervisor alongside the desktop executable. Command arguments never enter a shell.
use serde_json::{Value, json};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
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
    let config = Path::new(fields["config"].as_str().unwrap_or_default());
    if !config.is_absolute() {
        return Err("需要工作机配置文件的绝对路径。".into());
    }
    if stop {
        output(
            command(&program, &dir)
                .arg("stop")
                .arg("--config")
                .arg(config),
        )?;
        return Ok(json!({"message":"已请求工作机停止，运行中的任务会先完成。"}));
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
    let log = config.with_extension("worker.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|e| e.to_string())?;
    let mut child = command(&program, &dir)
        .arg("run")
        .arg("--config")
        .arg(config)
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(220));
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        return Err(format!(
            "工作机已退出（{status}）。请查看 {}",
            log.display()
        ));
    }
    Ok(
        json!({"message":format!("已启动工作机。当前空间的机器列表将在注册后更新。日志位于 {}",log.display())}),
    )
}
