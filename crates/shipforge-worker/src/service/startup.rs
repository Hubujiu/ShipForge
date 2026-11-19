use super::*;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
fn powershell() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("SystemRoot").ok_or("missing Windows installation directory")?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe"))
}
#[cfg(windows)]
fn run(script: &str, env: &[(&str, &std::ffi::OsStr)]) -> Result<String> {
    let mut cmd = Command::new(powershell()?);
    hidden(&mut cmd);
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ])
    .envs(env.iter().copied())
    .stdin(Stdio::null());
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("Windows could not update the current user's Startup shortcut".into());
    }
    String::from_utf8(out.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|e| e.to_string())
}
pub(super) fn configure(config: &Path, data: &Path, enable: bool) -> Result<()> {
    #[cfg(windows)]
    {
        let folder = run(
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); [Environment]::GetFolderPath([Environment+SpecialFolder]::Startup)",
            &[],
        )?;
        if !Path::new(&folder).is_absolute() {
            return Err("Windows Startup folder unavailable".into());
        }
        let desktop = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("shipforge-desktop.exe");
        configure_at(Path::new(&folder), &desktop, config, data, enable)
    }
    #[cfg(not(windows))]
    {
        let _ = (config, data);
        if enable {
            Err("user login startup is available on Windows".into())
        } else {
            Ok(())
        }
    }
}
#[cfg(windows)]
fn configure_at(
    folder: &Path,
    desktop: &Path,
    config: &Path,
    data: &Path,
    enable: bool,
) -> Result<()> {
    let link = folder.join(format!("ShipForge Worker {}.lnk", &identity(config)[..16]));
    if !enable {
        return match fs::remove_file(link) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    if !desktop.is_file() {
        return Err(
            "install the Windows desktop executable beside the worker to enable login startup"
                .into(),
        );
    }
    let args = format!(
        "--worker-login {} --data-dir {}",
        quote(config),
        quote(data)
    );
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let temp = tempfile::tempdir_in(folder).map_err(|e| e.to_string())?;
    let draft = temp.path().join("worker.lnk");
    run(
        "$ErrorActionPreference='Stop'; $shell=New-Object -ComObject WScript.Shell; $link=$shell.CreateShortcut($env:SHIPFORGE_SHORTCUT); $link.TargetPath=$env:SHIPFORGE_TARGET; $link.Arguments=$env:SHIPFORGE_ARGUMENTS; $link.Description='ShipForge machine supervisor'; $link.Save()",
        &[
            ("SHIPFORGE_SHORTCUT", draft.as_os_str()),
            ("SHIPFORGE_TARGET", desktop.as_os_str()),
            ("SHIPFORGE_ARGUMENTS", args.as_ref()),
        ],
    )?;
    fs::rename(draft, link).map_err(|e| e.to_string())
}
#[cfg(windows)]
fn quote(value: &Path) -> String {
    // Windows filenames cannot contain double quotes. A final backslash must be doubled before the closing quote.
    let s = dunce::simplified(value).to_string_lossy();
    let n = s.chars().rev().take_while(|c| *c == '\\').count();
    format!("\"{s}{}\"", "\\".repeat(n))
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn windows_shortcut_is_real_and_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let desktop = std::env::current_exe().unwrap();
        let config = tmp.path().join("machine 测试.json");
        let data = tmp.path().join("private QA");
        configure_at(tmp.path(), &desktop, &config, &data, true).unwrap();
        let link = tmp
            .path()
            .join(format!("ShipForge Worker {}.lnk", &identity(&config)[..16]));
        let text=run("[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$l=(New-Object -ComObject WScript.Shell).CreateShortcut($env:SHIPFORGE_SHORTCUT); $l.Arguments",&[("SHIPFORGE_SHORTCUT",link.as_os_str())]).unwrap();
        assert_eq!(
            text,
            format!(
                "--worker-login {} --data-dir {}",
                quote(&config),
                quote(&data)
            )
        );
        assert!(
            configure_at(
                tmp.path(),
                &tmp.path().join("absent.exe"),
                &config,
                &data,
                true
            )
            .is_err()
        );
        assert!(link.exists());
        configure_at(tmp.path(), &desktop, &config, &data, false).unwrap();
        assert!(!link.exists());
    }
}
