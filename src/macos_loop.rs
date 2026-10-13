use crate::daemon::Daemon;
use crate::hotkey::{default_combo, parse_combo};
use crate::platform::Platform;
use crate::providers::Provider;
use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

const RUNLOOP_PUMP_INTERVAL: Duration = Duration::from_millis(50);
const HOTKEY_QUEUE_CAPACITY: usize = 16;
const INPUT_MONITORING_HINT: &str =
    "event tap creation failed (NULL). Grant Input Monitoring to the proofd binary \
     (Settings > Privacy & Security > Input Monitoring), then restart the agent.";

// Must run on the main thread: only the main CFRunLoop dispatches the tap's Mach source.
pub async fn run_macos<P, R>(daemon: Daemon<P, R>, hotkey_str: &str) -> Result<(), String>
where
    P: Platform + 'static,
    R: Provider + 'static,
{
    let combo = parse_combo(hotkey_str).unwrap_or_else(|e| {
        tracing::warn!("{e}; falling back to Ctrl-Alt-P");
        default_combo()
    });

    let (hotkey_tx, hotkey_rx) = std::sync::mpsc::sync_channel::<()>(HOTKEY_QUEUE_CAPACITY);
    let combo_in_tap = combo.clone();
    let tap_disabled = Arc::new(AtomicBool::new(false));
    let tap_disabled_in_tap = tap_disabled.clone();
    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::KeyDown],
        move |_proxy, event_type, event| {
            match event_type {
                CGEventType::KeyDown => {
                    let autorepeat =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT);
                    let keycode =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                    if autorepeat == 0 && combo_in_tap.matches(event.get_flags(), keycode) {
                        let _ = hotkey_tx.try_send(());
                    }
                }
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                    tracing::warn!("event tap disabled by system ({event_type:?}); re-enabling");
                    tap_disabled_in_tap.store(true, Ordering::SeqCst);
                }
                _ => {}
            }
            CallbackResult::Keep
        },
    )
    .map_err(|()| INPUT_MONITORING_HINT.to_string())?;

    let source = tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|_| "event tap runloop source creation failed".to_string())?;
    CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    tracing::info!("hotkey armed (listen-only tap): {combo}");

    let shutdown = shutdown_on_ctrl_c();
    loop {
        pump_runloop_once(RUNLOOP_PUMP_INTERVAL);
        if shutdown.load(Ordering::SeqCst) {
            tracing::info!("shutdown");
            return Ok(());
        }
        if tap_disabled.swap(false, Ordering::SeqCst) {
            tap.enable();
        }
        if drain_presses(&hotkey_rx) {
            dispatch_polish(&daemon);
        }
        tokio::task::yield_now().await;
    }
}

fn shutdown_on_ctrl_c() -> Arc<AtomicBool> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        flag.store(true, Ordering::SeqCst);
    });
    shutdown
}

fn drain_presses(rx: &Receiver<()>) -> bool {
    let mut pressed = false;
    while rx.try_recv().is_ok() {
        pressed = true;
    }
    pressed
}

fn dispatch_polish<P, R>(daemon: &Daemon<P, R>)
where
    P: Platform + 'static,
    R: Provider + 'static,
{
    tracing::info!("hotkey pressed; running polish");
    if daemon.is_busy() {
        tracing::debug!("hotkey ignored while busy");
        return;
    }
    let daemon = daemon.clone();
    tokio::spawn(async move {
        daemon.handle_and_notify().await;
    });
}

// Runs the default mode: `kCFRunLoopCommonModes` is a set for adding sources, not a runnable mode.
fn pump_runloop_once(timeout: Duration) {
    // SAFETY: `kCFRunLoopDefaultMode` is a static CFString; `run_in_mode`
    // only runs the current thread's runloop for `timeout`.
    unsafe {
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, timeout, false);
    }
}
