//! HKCU Run-key autostart management.
//!
//! The registration is the single source of truth: the GUI reflects the
//! actual registry state rather than a config file value.

use std::io;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("escape-proxy");

pub fn command_for_current_exe() -> io::Result<String> {
    let exe = std::env::current_exe()?;
    Ok(format!("\"{}\" --background", exe.display()))
}

/// True if the HKCU Run entry exists.
pub fn is_enabled() -> io::Result<bool> {
    unsafe {
        let mut buf = [0u16; 1024];
        let mut cb = (buf.len() * 2) as u32;
        let rc = RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut cb),
        );
        if rc == ERROR_FILE_NOT_FOUND || rc == ERROR_PATH_NOT_FOUND {
            return Ok(false);
        }
        if rc.is_err() {
            return Err(io::Error::other(format!("registry read failed: {}", rc.0)));
        }
        Ok(true)
    }
}

/// Registers or removes autostart for the current executable.
pub fn set_enabled(enable: bool) -> io::Result<()> {
    unsafe {
        let mut key = HKEY::default();
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if rc.is_err() {
            return Err(io::Error::other(format!("registry open failed: {}", rc.0)));
        }
        let result = (|| {
            if enable {
                let cmd = command_for_current_exe()?;
                let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let raw: &[u8] =
                    std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
                let rc = RegSetValueExW(key, VALUE_NAME, 0, REG_SZ, Some(raw));
                if rc.is_err() {
                    return Err(io::Error::other(format!("registry write failed: {}", rc.0)));
                }
            } else {
                let rc = RegDeleteValueW(key, VALUE_NAME);
                if rc.is_err() && rc != ERROR_FILE_NOT_FOUND {
                    return Err(io::Error::other(format!("registry delete failed: {}", rc.0)));
                }
            }
            Ok(())
        })();
        let _ = RegCloseKey(key);
        result
    }
}
