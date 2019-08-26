use crate::error::*;
use std::path::PathBuf;
use winreg::enums::*;
use winreg::transaction::Transaction;
use winreg::RegKey;

const HANDLER_PREFIX: &str = "wslscript";
const CLASSES_SUBKEY: &str = "Software\\Classes";

/// Registers WSL Script as a handler for given file extension.
///
/// See https://docs.microsoft.com/en-us/windows/win32/shell/fa-file-types
/// See https://docs.microsoft.com/en-us/windows/win32/shell/fa-progids
/// See https://docs.microsoft.com/en-us/windows/win32/shell/fa-perceivedtypes
///
/// `ext` is an extension without the leading dot.
pub fn register_extension(ext: &str) -> Result<(), Error> {
    if ext.is_empty() {
        Err(ErrorKind::LogicError { s: "No extension." })?;
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
    let exe_os = std::env::current_exe().unwrap().canonicalize().unwrap();
    // shell handler doesn't recognize UNC format
    let executable = exe_os.to_str().unwrap().trim_start_matches("\\\\?\\");
    let cmd = format!("\"{}\" -E \"%0\" %*", executable);
    let icon = format!("{},0", executable);
    let handler_desc = format!("WSL Shell Script (.{})", ext);
    // Software\Classes\wslscript.ext
    set_value(&tx, &base, &handler_name, "", &handler_desc)?;
    set_value(&tx, &base, &handler_name, "EditFlags", &0x30u32)?;
    set_value(&tx, &base, &handler_name, "FriendlyTypeName", &handler_desc)?;
    // Software\Classes\wslscript.ext\DefaultIcon
    let path = format!("{}\\DefaultIcon", handler_name);
    set_value(&tx, &base, &path, "", &icon)?;
    // Software\Classes\wslscript.ext\shell
    let path = format!("{}\\shell", handler_name);
    set_value(&tx, &base, &path, "", &"open")?;
    // Software\Classes\wslscript.ext\shell\open - Open command
    let path = format!("{}\\shell\\open", handler_name);
    set_value(&tx, &base, &path, "", &"Run in WSL")?;
    set_value(&tx, &base, &path, "Icon", &icon)?;
    // Software\Classes\wslscript.ext\shell\open\command
    let path = format!("{}\\shell\\open\\command", handler_name);
    set_value(&tx, &base, &path, "", &cmd)?;
    // Software\Classes\wslscript.ext\shell\runas - Run as administrator
    let path = format!("{}\\shell\\runas", handler_name);
    set_value(&tx, &base, &path, "Icon", &icon)?;
    set_value(&tx, &base, &path, "Extended", &"")?;
    // Software\Classes\wslscript.ext\shell\runas\command
    let path = format!("{}\\shell\\runas\\command", handler_name);
    set_value(&tx, &base, &path, "", &cmd)?;
    // Software\Classes\wslscript.ext\shellex\DropHandler - Drop handler
    let path = format!("{}\\shellex\\DropHandler", handler_name);
    let value = "{86C86720-42A0-1069-A2E8-08002B30309D}";
    set_value(&tx, &base, &path, "", &value)?;
    // Software\Classes\.ext - Register handler for extension
    let path = &format!(".{}", ext);
    set_value(&tx, &base, &path, "", &handler_name)?;
    set_value(&tx, &base, &path, "PerceivedType", &"application")?;
    // Software\Classes\.ext\OpenWithProgIds - Add extension to open with list
    let path = &format!(".{}\\OpenWithProgIds", ext);
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
        .collect();
    Ok(extensions)
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
        .open_subkey(format!("{}\\shell\\open\\command", handler_name))
        .map_err(|e| ErrorKind::RegistryError { e })?;
    let cmd = key
        .get_value::<String, _>("")
        .map_err(|e| ErrorKind::RegistryError { e })?;
    // remove quotes
    if let Some(exe) = cmd.trim_start_matches('"').split_terminator('"').next() {
        return Ok(PathBuf::from(exe));
    }
    Err(ErrorKind::InvalidPathError)?
}

pub fn is_registered_for_current_executable(ext: &str) -> Result<bool, Error> {
    let registered_exe = get_handler_executable_path(ext)?
        .canonicalize()
        .map_err(|_| ErrorKind::InvalidPathError)?;
    let current_exe = std::env::current_exe()
        .map_err(|_| ErrorKind::InvalidPathError)?
        .canonicalize()
        .map_err(|_| ErrorKind::InvalidPathError)?;
    if current_exe == registered_exe {
        return Ok(true);
    }
    Ok(false)
}
