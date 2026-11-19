use super::*;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
mod unicode {
    use super::*;
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    use windows::{
        Win32::{
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize, IPersistFile,
            },
            UI::Shell::{
                FOLDERID_Startup, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
            },
        },
        core::{Interface, PCWSTR},
    };
    struct Apartment;
    impl Apartment {
        fn enter() -> windows::core::Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            }
            Ok(Self)
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }
    pub(super) fn startup() -> windows::core::Result<PathBuf> {
        let _apartment = Apartment::enter()?;
        unsafe {
            let ptr = SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DEFAULT, None)?;
            let folder = PathBuf::from(OsString::from_wide(ptr.as_wide()));
            CoTaskMemFree(Some(ptr.0.cast()));
            Ok(folder)
        }
    }
    pub(super) fn save(file: &Path, desktop: &Path, args: &str) -> windows::core::Result<()> {
        let _apartment = Apartment::enter()?;
        let target = wide(dunce::simplified(desktop).as_os_str());
        let args = wide(args.as_ref());
        let file = wide(dunce::simplified(file).as_os_str());
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(PCWSTR(target.as_ptr()))?;
            link.SetArguments(PCWSTR(args.as_ptr()))?;
            link.SetDescription(windows::core::w!("ShipForge machine supervisor"))?;
            let persistence: IPersistFile = link.cast()?;
            persistence.Save(PCWSTR(file.as_ptr()), true)
        }
    }
    #[cfg(test)]
    pub(super) fn read(file: &Path) -> windows::core::Result<(PathBuf, String)> {
        use windows::Win32::System::Com::STGM_READ;
        let _apartment = Apartment::enter()?;
        let filename = wide(dunce::simplified(file).as_os_str());
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let persistence: IPersistFile = link.cast()?;
            persistence.Load(PCWSTR(filename.as_ptr()), STGM_READ)?;
            let mut path = vec![0u16; 32768];
            link.GetPath(&mut path, std::ptr::null_mut(), 0)?;
            let n = path.iter().position(|u| *u == 0).unwrap_or(path.len());
            let mut args = vec![0u16; 32768];
            link.GetArguments(&mut args)?;
            let m = args.iter().position(|u| *u == 0).unwrap_or(args.len());
            Ok((
                PathBuf::from(OsString::from_wide(&path[..n])),
                String::from_utf16_lossy(&args[..m]),
            ))
        }
    }
}
pub(super) fn configure(config: &Path, data: &Path, enable: bool) -> Result<()> {
    #[cfg(windows)]
    {
        let folder = unicode::startup().map_err(|e| e.to_string())?;
        if !folder.is_absolute() {
            return Err("Windows Startup folder unavailable".into());
        }
        let desktop = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("shipforge-desktop.exe");
        configure_at(&folder, &desktop, config, data, enable)
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
    unicode::save(&draft, desktop, &args).map_err(|e| e.to_string())?;
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
        let desktop = tmp.path().join("ShipForge 開発 🚀.exe");
        fs::write(&desktop, []).unwrap();
        let config = tmp.path().join("machine 测试.json");
        let data = tmp.path().join("private QA 🧪");
        configure_at(tmp.path(), &desktop, &config, &data, true).unwrap();
        let link = tmp
            .path()
            .join(format!("ShipForge Worker {}.lnk", &identity(&config)[..16]));
        let (target, text) = unicode::read(&link).unwrap();
        assert_eq!(
            target.canonicalize().unwrap(),
            desktop.canonicalize().unwrap()
        );
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
