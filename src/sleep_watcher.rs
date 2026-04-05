//! Sleep/wake notification listener for macOS.
//!
//! Uses IOKit's `IORegisterForSystemPower` to receive power events, then
//! forwards them through a tokio channel so the daemon can checkpoint before
//! sleep and trigger reconnection after wake.

#[derive(Debug)]
pub enum PowerEvent {
    /// System is about to sleep. Send `()` on the responder when pre-sleep
    /// work (e.g. checkpoints) is done so the sleep-watcher thread can call
    /// `IOAllowPowerChange`.
    WillSleep {
        done: tokio::sync::oneshot::Sender<()>,
    },
    DidWake,
}

/// Spawn a background listener for system sleep/wake events.
///
/// On macOS this registers an IOKit callback and runs a CFRunLoop on a
/// dedicated thread.  On other platforms the returned receiver never
/// produces events (the sender is simply dropped).
pub fn start() -> tokio::sync::mpsc::Receiver<PowerEvent> {
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    #[cfg(target_os = "macos")]
    {
        std::thread::Builder::new()
            .name("sleep-watcher".into())
            .spawn(move || macos::run_loop(tx))
            .expect("failed to spawn sleep-watcher thread");
    }

    #[cfg(not(target_os = "macos"))]
    drop(tx);

    rx
}

// ---------------------------------------------------------------------------
// macOS IOKit implementation
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod macos {
    use super::PowerEvent;
    use std::ffi::c_void;
    use tokio::sync::mpsc::Sender;

    // IOKit message constants
    const KIO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
    const KIO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
    const KIO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;

    // CFRunLoop helpers from core-foundation-sys (re-exported by core-foundation)
    extern "C" {
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
    }

    // IOKit power management
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            notify_port_ref: *mut *mut c_void,
            callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void),
            notifier: *mut u32,
        ) -> u32;
        fn IOAllowPowerChange(kernel_port: u32, notification_id: isize) -> i32;
        fn IONotificationPortGetRunLoopSource(notify_port: *mut c_void) -> *mut c_void;
    }

    // kCFRunLoopDefaultMode — a well-known CoreFoundation string constant.
    extern "C" {
        static kCFRunLoopDefaultMode: *const c_void;
    }

    /// IOKit callback invoked on the sleep-watcher thread.
    extern "C" fn power_callback(
        refcon: *mut c_void,
        _service: u32,
        message_type: u32,
        message_argument: *mut c_void,
    ) {
        let tx = unsafe { &*(refcon as *const Sender<PowerEvent>) };
        let notification_id = message_argument as isize;

        match message_type {
            KIO_MESSAGE_CAN_SYSTEM_SLEEP => {
                // We don't want to block sleep — just allow it.
                let root_port = ROOT_PORT.load(Ordering::Acquire);
                unsafe { IOAllowPowerChange(root_port, notification_id) };
            }
            KIO_MESSAGE_SYSTEM_WILL_SLEEP => {
                tracing::debug!("IOKit: SystemWillSleep");
                let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                // blocking_send is fine — we're on a std thread, not async.
                let _ = tx.blocking_send(PowerEvent::WillSleep { done: done_tx });
                // Wait for daemon to finish checkpoints before allowing sleep.
                // blocking_recv returns Err if sender is dropped (e.g. daemon panic),
                // which is fine — we allow sleep anyway.
                let _ = done_rx.blocking_recv();
                let root_port = ROOT_PORT.load(Ordering::Acquire);
                unsafe { IOAllowPowerChange(root_port, notification_id) };
            }
            KIO_MESSAGE_SYSTEM_HAS_POWERED_ON => {
                tracing::debug!("IOKit: SystemHasPoweredOn");
                let _ = tx.blocking_send(PowerEvent::DidWake);
            }
            _ => {}
        }
    }

    /// Global root-port handle used by the callback. Set once in `run_loop`
    /// before the CFRunLoop starts.
    use std::sync::atomic::{AtomicU32, Ordering};
    static ROOT_PORT: AtomicU32 = AtomicU32::new(0);

    pub(super) fn run_loop(tx: Sender<PowerEvent>) {
        let tx_box = Box::new(tx);
        let refcon = Box::into_raw(tx_box) as *mut c_void;

        let mut notify_port: *mut c_void = std::ptr::null_mut();
        let mut notifier: u32 = 0;

        let root_port = unsafe {
            IORegisterForSystemPower(refcon, &mut notify_port, power_callback, &mut notifier)
        };

        if root_port == 0 {
            tracing::error!("IORegisterForSystemPower failed — sleep/wake events unavailable");
            // Reclaim the sender so it doesn't leak.
            let _ = unsafe { Box::from_raw(refcon as *mut Sender<PowerEvent>) };
            return;
        }

        ROOT_PORT.store(root_port, Ordering::Release);

        let rl_source = unsafe { IONotificationPortGetRunLoopSource(notify_port) };
        unsafe {
            let rl = CFRunLoopGetCurrent();
            CFRunLoopAddSource(rl, rl_source, kCFRunLoopDefaultMode);
        }

        tracing::info!("Sleep/wake watcher started (IOKit)");

        // Blocks forever — the thread is dedicated to receiving power events.
        unsafe { CFRunLoopRun() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_event_variant_names() {
        // WillSleep carries a oneshot::Sender so we can't derive PartialEq.
        // Just verify both variants construct without issue.
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let _sleep = PowerEvent::WillSleep { done: tx };
        let _wake = PowerEvent::DidWake;
    }

    #[test]
    fn test_start_returns_receiver() {
        // Just verify start() doesn't panic and returns a receiver.
        let _rx = start();
    }
}
