# webusb-web — WebUSB on the web 🕸️

This crate provides WebUSB support in a JavaScript runtime environment, usually a web browser.
It allows you to communicate with connected USB devices from a web browser.

The **WebUSB API** provides a way to expose non-standard Universal Serial Bus (USB)
compatible devices services to the web, to make USB safer and easier to use.

This crate provides Rust support for using WebUSB when targeting WebAssembly.

MDN provides a [WebUSB overview] while detailed information is available in the
[WebUSB specification].

[WebUSB overview]: https://developer.mozilla.org/en-US/docs/Web/API/WebUSB_API
[WebUSB specification]: https://wicg.github.io/webusb/

[![crates.io page](https://img.shields.io/crates/v/webusb-web)](https://crates.io/crates/webusb-web)
[![docs.rs page](https://docs.rs/webusb-web/badge.svg)](https://docs.rs/webusb-web)
[![Apache 2 license](https://img.shields.io/crates/l/webusb-web)](https://raw.githubusercontent.com/surban/webusb-web/master/LICENSE)

## Building

This crate depends on unstable features of the `web_sys` crate.
Therefore you must add `--cfg=web_sys_unstable_apis` to the Rust
compiler flags.

One way of doing this is to create the file `.cargo/config.toml` in your
project with the following contents:

```toml
[build]
target = "wasm32-unknown-unknown"
rustflags = ["--cfg=web_sys_unstable_apis"]
```

## License

webusb-web is licensed under the [Apache 2.0 license].

[Apache 2.0 license]: https://github.com/surban/webusb-web/blob/master/LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in webusb-web by you, shall be licensed as Apache 2.0, without any
additional terms or conditions.
