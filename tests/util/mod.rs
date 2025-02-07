use std::fmt;

/// Log to console.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        web_sys::console::log_1(&::wasm_bindgen::JsValue::from_str(&format!($($arg)*)));
    }
}

#[track_caller]
pub fn log_and_panic(msg: &str) -> ! {
    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(msg));
    panic!("{msg}")
}

#[macro_export]
macro_rules! panic_log {
    ($($arg:tt)*) => {
        crate::util::log_and_panic(&format!($($arg)*))
    }
}

pub trait ResultExt<T> {
    #[track_caller]
    fn expect_log(self, msg: &str) -> T;
    #[track_caller]
    fn unwrap_log(self) -> T;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: fmt::Display,
{
    #[track_caller]
    fn expect_log(self, msg: &str) -> T {
        match self {
            Ok(v) => v,
            Err(err) => panic_log!("{msg}: {err}"),
        }
    }

    #[track_caller]
    fn unwrap_log(self) -> T {
        match self {
            Ok(v) => v,
            Err(err) => panic_log!("unwrap failed: {err}"),
        }
    }
}

impl<T> ResultExt<T> for Option<T> {
    #[track_caller]
    fn expect_log(self, msg: &str) -> T {
        match self {
            Some(v) => v,
            None => panic_log!("{msg}"),
        }
    }

    #[track_caller]
    fn unwrap_log(self) -> T {
        match self {
            Some(v) => v,
            None => panic_log!("unwrap of None option"),
        }
    }
}
