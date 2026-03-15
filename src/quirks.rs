//! Workarounds for browser bugs.

use js_sys::Reflect;
use wasm_bindgen::{JsCast, JsValue};

use crate::{Error, ErrorKind};

/// Returns `true` if the browser is running on Windows.
fn is_windows() -> bool {
    let global = js_sys::global();
    // Try navigator.platform (available in both Window and Worker contexts).
    let navigator = if let Some(window) = global.dyn_ref::<web_sys::Window>() {
        Reflect::get(&window.navigator(), &JsValue::from_str("platform")).ok()
    } else if let Some(worker) = global.dyn_ref::<web_sys::WorkerGlobalScope>() {
        Reflect::get(&worker.navigator(), &JsValue::from_str("platform")).ok()
    } else {
        None
    };
    navigator.and_then(|v| v.as_string()).map(|p| p.starts_with("Win")).unwrap_or(false)
}

/// Remap a `NetworkError` promise rejection to [`ErrorKind::Stall`] on Windows.
///
/// Chromium's Windows USB backend (`usb_device_handle_win.cc`) maps all
/// WinUSB errors — including stalls (`ERROR_GEN_FAILURE`) — to
/// `UsbTransferStatus::TRANSFER_ERROR`, which causes the Blink layer to
/// reject the promise with a `NetworkError` instead of resolving it with
/// `status: "stall"` as the WebUSB spec requires.  On Linux the stall is
/// reported correctly via `EPIPE` → `UsbTransferStatus::STALLED`.
///
/// This affects all transfer types (control, bulk, interrupt).
///
/// See Chromium issue <https://issues.chromium.org/issues/40832809>.
pub(crate) fn network_error_as_stall(js_err: JsValue) -> Error {
    if is_windows() {
        if let Some(js_error) = js_err.dyn_ref::<js_sys::Error>() {
            let name = js_error.name().as_string().unwrap_or_default();
            if name == "NetworkError" {
                let msg = js_error.message().as_string().unwrap_or_default();
                web_sys::console::warn_1(
                    &format!("[webusb-web] remapping NetworkError to Stall (Windows): {msg:?}").into(),
                );
                return Error::new(ErrorKind::Stall, msg);
            }
        }
    }
    // Not Windows or not a NetworkError — use the normal conversion.
    js_err.into()
}
