# Changelog

All notable changes to this project will be documented in this file.

## 0.4.2 - Unreleased

### Changed
- Update `web-sys` and `js-sys` to 0.3.91.

## 0.4.1 - 2025-03-03

### Fixed
- Fix events not working after drop of `Usb` object by using `EventTarget` for USB device events.

### Added
- Device equality test.

## 0.4.0 - 2025-02-18

### Added
- Implement `PartialEq`, `Eq` and `Hash` for `UsbDevice`.
- Implement `AsRef<web_sys::UsbDevice>` for `UsbDevice` to provide access to the native `web-sys` object.

### Changed
- `UsbDevice::forget` now takes `self` by value instead of by reference.

## 0.3.1 - 2025-02-18

### Added
- Implement conversion from `Error` to `std::io::Error`.

## 0.3.0 - 2025-02-09

### Changed
- Improve `UsbDeviceFilter` API with const builder methods (`with_vendor_id`, `with_product_id`, `with_class_code`, `with_subclass_code`, `with_protocol_code`, `with_serial_number`).
- Make `UsbDeviceFilter::new` and `UsbControlRequest::new` const.
- Remove `UsbDeviceFilter::by_vendor_and_product_id` in favor of builder pattern.

## 0.2.0 - 2025-02-09

### Changed
- Rename `UsbAlternateSetting` to `UsbAlternateInterface`.
- Add `alternate` and `claimed` fields to `UsbInterface`.
- Improve documentation for USB descriptor types.
- Add missing USB device fields.

## 0.1.4 - 2025-02-07

### Changed
- Improve documentation and docs.rs configuration.

## 0.1.1 - 2025-02-07

### Fixed
- Fix docs.rs build configuration.

## 0.1.0 - 2025-02-07

- Initial release.
