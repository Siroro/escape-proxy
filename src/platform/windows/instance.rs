//! Single-instance mutex and second-launch activation.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, EVENT_MODIFY_STATE,
};

const MUTEX_NAME: PCWSTR = w!("Local\\escape-proxy-single-instance");
const ACTIVATE_EVENT_NAME: PCWSTR = w!("Local\\escape-proxy-activate");
const EXIT_EVENT_NAME: PCWSTR = w!("Local\\escape-proxy-exit");

/// What a second launch asks the running instance to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondLaunch {
    /// Bring the settings window up (the default when the app is opened
    /// again).
    ShowSettings,
    /// Shut down (`escape-proxy --exit`).
    Exit,
}

/// A raw handle wrapped for transfer to another thread. Safe because the
/// Windows object APIs used here are thread-agnostic.
#[derive(Clone, Copy)]
pub struct SendHandle(pub HANDLE);

// SAFETY: HANDLE is an opaque kernel object pointer; all uses in this crate
// call WaitForSingleObject-family/CloseHandle APIs that are safe from any
// thread.
unsafe impl Send for SendHandle {}

impl SendHandle {
    pub fn close(&self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Owns the single-instance mutex for the process lifetime.
pub struct SingleInstance {
    mutex: SendHandle,
}

impl SingleInstance {
    /// Attempts to become the single instance for this user session.
    /// Returns `Ok(Some(guard))` for the first instance. For a second
    /// instance, signals the first as requested by `second_launch` and
    /// returns `Ok(None)`.
    pub fn try_acquire(
        second_launch: SecondLaunch,
    ) -> windows::core::Result<Option<SingleInstance>> {
        unsafe {
            let mutex = CreateMutexW(None, false, MUTEX_NAME)?;
            if GetLastError() == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(mutex);
                let event = match second_launch {
                    SecondLaunch::ShowSettings => ACTIVATE_EVENT_NAME,
                    SecondLaunch::Exit => EXIT_EVENT_NAME,
                };
                signal_first_instance(event)?;
                return Ok(None);
            }
            Ok(Some(SingleInstance {
                mutex: SendHandle(mutex),
            }))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        self.mutex.close();
    }
}

/// Signals the running instance to bring its settings window up.
pub fn activate_first_instance() -> windows::core::Result<()> {
    signal_first_instance(ACTIVATE_EVENT_NAME)
}

/// Signals the running instance to shut down (`escape-proxy --exit`).
pub fn exit_first_instance() -> windows::core::Result<()> {
    signal_first_instance(EXIT_EVENT_NAME)
}

fn signal_first_instance(event: PCWSTR) -> windows::core::Result<()> {
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, false, event)?;
        SetEvent(event)?;
        let _ = CloseHandle(event);
        Ok(())
    }
}

/// Creates the named activation event the tray message loop waits on.
pub fn create_activation_event() -> windows::core::Result<SendHandle> {
    unsafe { Ok(SendHandle(CreateEventW(None, false, false, ACTIVATE_EVENT_NAME)?)) }
}

/// Creates the named exit event the tray message loop waits on.
pub fn create_exit_event() -> windows::core::Result<SendHandle> {
    unsafe { Ok(SendHandle(CreateEventW(None, false, false, EXIT_EVENT_NAME)?)) }
}
