use crate::error::*;
use crate::icon::ShellIcon;
use guid_create::GUID;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use wchar::*;
use widestring::*;
use winreg::enums::*;
use winreg::transaction::Transaction;
use winreg::RegKey;

const HANDLER_PREFIX: &str = "wslscript";
const CLASSES_SUBKEY: &str = r"Software\Classes";
const LXSS_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";

#[derive(Clone)]
pub struct ExtConfig {
    // filetype extension without leading dot
    pub extension: String,
    pub icon: Option<ShellIcon>,
    pub hold_mode: HoldMode,
    pub distro: Option<DistroGUID>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum HoldMode {
    Never,  // always close terminal window on exit
    Always, // always wait for keypress on exit
    Error,  // wait for keypress when exit code != 0
}

impl HoldMode {
    const WCSTR_NEVER: &'static [WideChar] = wch_c!("never");
    const WCSTR_ALWAYS: &'static [WideChar] = wch_c!("always");
    const WCSTR_ERROR: &'static [WideChar] = wch_c!("error");

    /// Create from nul terminated wide string
    pub fn from_wcstr(s: &WideCStr) -> Option<Self> {
        match s.as_slice_with_nul() {
            Self::WCSTR_NEVER => Some(Self::Never),
            Self::WCSTR_ALWAYS => Some(Self::Always),
            Self::WCSTR_ERROR => Some(Self::Error),
            _ => None,
        }
    }

    /// Create from &str
    pub fn from_str(s: &str) -> Option<Self> {
        WideCString::from_str(s)
            .ok()
            .and_then(|s| Self::from_wcstr(&s))
    }

    /// Get mode string as a nul terminated wide string
    pub fn as_wcstr(self) -> &'static WideCStr {
        match self {
            Self::Never => unsafe { WideCStr::from_slice_with_nul_unchecked(Self::WCSTR_NEVER) },
            Self::Always => unsafe { WideCStr::from_slice_with_nul_unchecked(Self::WCSTR_ALWAYS) },
            Self::Error => unsafe { WideCStr::from_slice_with_nul_unchecked(Self::WCSTR_ERROR) },
        }
    }

    /// Get mode as a utf-8 string
    pub fn as_string(self) -> String {
        self.as_wcstr().to_string_lossy()
    }
}

impl Default for HoldMode {
    fn default() -> Self {
        Self::Error
    }
}

/// GUID of the WSL distribution.
#[derive(Clone, Eq)]
pub struct DistroGUID {
    guid: GUID,
    /// Pinned wide c-string of the GUID for win32 usage. Enclosed in `{`...`}`.
    wcs: Pin<WideCString>,
}
impl DistroGUID {
    /// Get reference to the pinned wide c-string of the GUID.
    pub fn as_wcstr(&self) -> &WideCStr {
        &self.wcs
    }
}
impl std::ops::Deref for DistroGUID {
    type Target = GUID;
    fn deref(&self) -> &Self::Target {
        &self.guid
    }
}
impl std::fmt::Display for DistroGUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.wcs.to_string().map_err(|_| std::fmt::Error)?;
        f.write_str(&s)
    }
}
impl FromStr for DistroGUID {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let guid = GUID::parse(s.trim_start_matches('{').trim_end_matches('}')).map_err(|_| ())?;
        let s = format!("{{{}}}", guid.to_string().to_ascii_lowercase());
        let wcs = unsafe { WideCString::from_str_unchecked(s) };
        Ok(Self {
            guid,
            wcs: Pin::new(wcs),
        })
    }
}
impl std::cmp::PartialEq for DistroGUID {
    fn eq(&self, other: &Self) -> bool {
        self.guid.eq(&other.guid)
    }
}
impl std::hash::Hash for DistroGUID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data1().hash(state);
        self.data2().hash(state);
        self.data3().hash(state);
        self.data4().hash(state);
    }
}

pub struct Distros {
    pub list: HashMap<DistroGUID, String>,
    pub default: Option<DistroGUID>,
}
impl Default for Distros {
    fn default() -> Self {
        Self {
            list: HashMap::new(),
            default: None,
        }
    }
}
impl Distros {
    pub fn sorted_pairs(&self) -> Vec<(&DistroGUID, &str)> {
        let mut pairs = self
            .list
            .iter()
            .map(|(k, v)| (k, v.as_str()))
            .collect::<Vec<_>>();
        pairs.sort_by(|&a, &b| {
            use std::cmp::Ordering::*;
            if let Some(default) = self.default.as_ref() {
                if a.0 == default {
                    return Less;
                }
                if b.0 == default {
                    return Greater;
                }
            }
            a.1.cmp(b.1)
        });
        pairs
    }
}

/// Registers WSL Script as a handler for given file extension.
///
/// See https://docs.microsoft.com/en-us/windows/win32/shell/fa-file-types
/// See https://docs.microsoft.com/en-us/windows/win32/shell/fa-progids
/// See https://docs.microsoft.com/en-us/windows/win32/shell/fa-perceivedtypes
///
pub fn register_extension(config: &ExtConfig) -> Result<(), Error> {
    let ext = config.extension.as_str();
    if ext.is_empty() {
        return Err(Error::from(ErrorKind::LogicError { s: "No extension." }));
    }
    let tx = Transaction::new().map_err(|e| ErrorKind::RegistryError { e })?;
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let base = root
        .open_subkey_transacted_with_flags(CLASSES_SUBKEY, &tx, KEY_ALL_ACCESS)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let handler_name = format!("{}.{}", HANDLER_PREFIX, ext);
    // delete previous handler key in a transaction
    // see https://docs.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regdeletekeytransactedw#remarks
    if let Ok(key) = base.open_subkey_transacted_with_flags(&handler_name, &tx, KEY_ALL_ACCESS) {
        key.delete_subkey_all("")
            .map_err(|e| ErrorKind::RegistryError { e })?;
    }
    // command argument to select distribution
    let distro_arg = config
        .distro
        .as_ref()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::Other))
        .and_then(|distro| {
            RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_transacted(LXSS_SUBKEY, &tx)
                .and_then(|k| k.open_subkey_transacted(distro.to_string(), &tx))
        })
        .and_then(|k| k.get_value::<String, _>("DistributionName"))
        .map(|name| format!(r#" -d "{}""#, name))
        .unwrap_or_default();
    let hold_mode = config.hold_mode.as_string();
    let exe_os = std::env::current_exe()?.canonicalize()?;
    // shell handler doesn't recognize UNC format
    let executable = exe_os
        .to_str()
        .ok_or_else(|| ErrorKind::StringToPathUTF8Error)?
        .trim_start_matches(r"\\?\");
    let cmd = format!(
        r#""{}"{} -h {} -E "%0" %*"#,
        executable, distro_arg, hold_mode
    );
    let icon: Option<OsString> = config
        .icon
        .as_ref()
        .and_then(|icon| Some(icon.shell_path().to_os_string()));
    let handler_desc = format!("WSL Shell Script (.{})", ext);
    // Software\Classes\wslscript.ext
    set_value(&tx, &base, &handler_name, "", &handler_desc)?;
    set_value(&tx, &base, &handler_name, "EditFlags", &0x30u32)?;
    set_value(&tx, &base, &handler_name, "FriendlyTypeName", &handler_desc)?;
    set_value(&tx, &base, &handler_name, "HoldMode", &hold_mode)?;
    if let Some(distro) = &config.distro {
        set_value(
            &tx,
            &base,
            &handler_name,
            "Distribution",
            &distro.to_string(),
        )?;
    }
    // Software\Classes\wslscript.ext\DefaultIcon
    if let Some(s) = &icon {
        let path = format!(r"{}\DefaultIcon", handler_name);
        set_value(&tx, &base, &path, "", &s.as_os_str())?;
    }
    // Software\Classes\wslscript.ext\shell
    let path = format!(r"{}\shell", handler_name);
    set_value(&tx, &base, &path, "", &"open")?;
    // Software\Classes\wslscript.ext\shell\open - Open command
    let path = format!(r"{}\shell\open", handler_name);
    set_value(&tx, &base, &path, "", &"Run in WSL")?;
    if let Some(s) = &icon {
        set_value(&tx, &base, &path, "Icon", &s.as_os_str())?;
    }
    // Software\Classes\wslscript.ext\shell\open\command
    let path = format!(r"{}\shell\open\command", handler_name);
    set_value(&tx, &base, &path, "", &cmd)?;
    // Software\Classes\wslscript.ext\shell\runas - Run as administrator
    let path = format!(r"{}\shell\runas", handler_name);
    set_value(&tx, &base, &path, "Extended", &"")?;
    if let Some(s) = &icon {
        set_value(&tx, &base, &path, "Icon", &s.as_os_str())?;
    }
    // Software\Classes\wslscript.ext\shell\runas\command
    let path = format!(r"{}\shell\runas\command", handler_name);
    set_value(&tx, &base, &path, "", &cmd)?;
    // Software\Classes\wslscript.ext\shellex\DropHandler - Drop handler
    let path = format!(r"{}\shellex\DropHandler", handler_name);
    let value = "{86C86720-42A0-1069-A2E8-08002B30309D}";
    set_value(&tx, &base, &path, "", &value)?;
    // Software\Classes\.ext - Register handler for extension
    let path = &format!(".{}", ext);
    set_value(&tx, &base, &path, "", &handler_name)?;
    set_value(&tx, &base, &path, "PerceivedType", &"application")?;
    // Software\Classes\.ext\OpenWithProgIds - Add extension to open with list
    let path = &format!(r".{}\OpenWithProgIds", ext);
    set_value(&tx, &base, &path, &handler_name, &"")?;
    tx.commit().map_err(|e| ErrorKind::RegistryError { e })?;
    Ok(())
}

pub fn unregister_extension(ext: &str) -> Result<(), Error> {
    let tx = Transaction::new().map_err(|e| ErrorKind::RegistryError { e })?;
    let base = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_transacted_with_flags(CLASSES_SUBKEY, &tx, KEY_ALL_ACCESS)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let handler_name = format!("{}.{}", HANDLER_PREFIX, ext);
    // delete handler
    if let Ok(key) = base.open_subkey_transacted_with_flags(&handler_name, &tx, KEY_ALL_ACCESS) {
        key.delete_subkey_all("")
            .map_err(|e| ErrorKind::RegistryError { e })?;
        base.delete_subkey_transacted(&handler_name, &tx)
            .map_err(|e| ErrorKind::RegistryError { e })?;
    }
    let ext_name = format!(".{}", ext);
    if let Ok(ext_key) = base.open_subkey_transacted_with_flags(&ext_name, &tx, KEY_ALL_ACCESS) {
        // if extension has handler as a default
        if let Ok(val) = ext_key.get_value::<String, _>("") {
            if val == handler_name {
                // set default handler to unset
                ext_key
                    .delete_value("")
                    .map_err(|e| ErrorKind::RegistryError { e })?;
            }
        }
        // cleanup OpenWithProgids
        let open_with_name = "OpenWithProgIds";
        if let Ok(open_with_key) =
            ext_key.open_subkey_transacted_with_flags(open_with_name, &tx, KEY_ALL_ACCESS)
        {
            // remove handler
            if let Some(progid) = open_with_key.enum_values().find_map(|item| {
                item.ok()
                    .filter(|(name, _)| *name == handler_name)
                    .map(|(name, _)| name)
            }) {
                open_with_key
                    .delete_value(progid)
                    .map_err(|e| ErrorKind::RegistryError { e })?;
            }
            // if OpenWithProgids was left empty
            if let Ok(info) = open_with_key.query_info() {
                if info.sub_keys == 0 && info.values == 0 {
                    ext_key
                        .delete_subkey_transacted(open_with_name, &tx)
                        .map_err(|e| ErrorKind::RegistryError { e })?;
                }
            }
        }
        // if default handler is unset
        if ext_key.get_value::<String, _>(&"").is_err() {
            // ... and extension has no subkeys
            if let Ok(info) = ext_key.query_info() {
                if info.sub_keys == 0 {
                    // ... remove extension key altogether
                    base.delete_subkey_transacted(&ext_name, &tx)
                        .map_err(|e| ErrorKind::RegistryError { e })?;
                }
            }
        }
    }
    tx.commit().map_err(|e| ErrorKind::RegistryError { e })?;
    Ok(())
}

fn set_value<T: winreg::types::ToRegValue>(
    tx: &Transaction,
    base: &RegKey,
    path: &str,
    name: &str,
    value: &T,
) -> Result<(), Error> {
    let key = base
        .create_subkey_transacted(path, &tx)
        .map_err(|e| ErrorKind::RegistryError { e })?
        .0;
    key.set_value(name, value)
        .map_err(|e| Error::from(ErrorKind::RegistryError { e }))
}

pub fn query_registered_extensions() -> Result<Vec<String>, Error> {
    let base = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(CLASSES_SUBKEY)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let extensions: Vec<String> = base
        .enum_keys()
        .filter_map(Result::ok)
        .filter(|k| k.starts_with(HANDLER_PREFIX))
        .map(|k| {
            k.trim_start_matches(HANDLER_PREFIX)
                .trim_start_matches('.')
                .to_string()
        })
        .filter(|ext| is_extension_registered_for_wsl(ext).unwrap_or(false))
        .collect();
    Ok(extensions)
}

/// Query installed WSL distributions.
pub fn query_distros() -> Result<Distros, Error> {
    let base = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(LXSS_SUBKEY)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let mut distros = Distros::default();
    base.enum_keys().filter_map(Result::ok).for_each(|s| {
        if let Ok(name) = base
            .open_subkey(&s)
            .and_then(|k| k.get_value::<String, _>("DistributionName"))
        {
            if let Ok(guid) = DistroGUID::from_str(&s) {
                distros.list.insert(guid, name);
            }
        }
    });
    if let Ok(s) = base.get_value::<String, _>("DefaultDistribution") {
        if let Ok(guid) = DistroGUID::from_str(&s) {
            distros.default = Some(guid);
        }
    }
    Ok(distros)
}

pub fn get_extension_config(ext: &str) -> Result<ExtConfig, Error> {
    let handler_key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(CLASSES_SUBKEY)
        .map_err(|e| ErrorKind::RegistryError { e })?
        .open_subkey(format!("{}.{}", HANDLER_PREFIX, ext))
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let mut icon: Option<ShellIcon> = None;
    if let Ok(key) = handler_key.open_subkey("DefaultIcon") {
        if let Ok(s) = key.get_value::<String, _>("") {
            icon = s.parse::<ShellIcon>().ok();
        }
    }
    let hold_mode = handler_key
        .get_value::<String, _>("HoldMode")
        .ok()
        .and_then(|s| HoldMode::from_str(&s))
        .unwrap_or_default();
    let distro = handler_key
        .get_value::<String, _>("Distribution")
        .ok()
        .and_then(|s| DistroGUID::from_str(&s).ok());
    Ok(ExtConfig {
        extension: ext.to_owned(),
        icon,
        hold_mode,
        distro,
    })
}

pub fn is_extension_registered_for_wsl(ext: &str) -> Result<bool, Error> {
    let base = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(CLASSES_SUBKEY)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    if let Ok(key) = base.open_subkey(format!(".{}", ext)) {
        if let Ok(handler) = key.get_value::<String, _>("") {
            if handler == format!("{}.{}", HANDLER_PREFIX, ext) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn is_registered_for_other(ext: &str) -> Result<bool, Error> {
    let base = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(CLASSES_SUBKEY)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    if let Ok(key) = base.open_subkey(format!(".{}", ext)) {
        if let Ok(handler) = key.get_value::<String, _>("") {
            if handler != format!("{}.{}", HANDLER_PREFIX, ext) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn get_handler_executable_path(ext: &str) -> Result<PathBuf, Error> {
    let base = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(CLASSES_SUBKEY)
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let handler_name = format!("{}.{}", HANDLER_PREFIX, ext);
    let key = base
        .open_subkey(format!(r"{}\shell\open\command", handler_name))
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let cmd = key
        .get_value::<String, _>("")
        .map_err(|e| ErrorKind::RegistryError { e })?;
    // remove quotes
    if let Some(exe) = cmd.trim_start_matches('"').split_terminator('"').next() {
        return Ok(PathBuf::from(exe));
    }
    Err(Error::from(ErrorKind::InvalidPathError))
}

/// Whether extension is registered for current wslscript executable.
///
/// Returns an error if extension is not registered for WSLScript, or some
/// error occurs.
pub fn is_registered_for_current_executable(ext: &str) -> Result<bool, Error> {
    let registered_exe = get_handler_executable_path(ext)?;
    let registered_exe = registered_exe.canonicalize().unwrap_or(registered_exe);
    let current_exe = std::env::current_exe()?;
    let current_exe = current_exe.canonicalize().unwrap_or(current_exe);
    if current_exe == registered_exe {
        return Ok(true);
    }
    Ok(false)
}
