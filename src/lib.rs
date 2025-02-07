//! WebUSB on the web 🕸️ — Access USB devices from the web browser.
//!
//! The **WebUSB API** provides a way to expose non-standard Universal Serial Bus (USB)
//! compatible devices services to the web, to make USB safer and easier to use.
//!
//! This crate provides Rust support for using WebUSB when targeting WebAssembly.
//!
//! MDN provides a [WebUSB overview] while detailed information is available in the
//! [WebUSB specification].
//!
//! [WebUSB overview]: https://developer.mozilla.org/en-US/docs/Web/API/WebUSB_API
//! [WebUSB specification]: https://wicg.github.io/webusb/
//!
//! ### Building
//! This crate depends on unstable features of the `web_sys` crate.
//! Therefore you must add `--cfg=web_sys_unstable_apis` to the Rust
//! compiler flags (`RUSTFLAGS`).
//! 
//! ### Usage
//! Call [`Usb::new()`] to obtain an interface to the WebUSB API.
//! You must call [`Usb::request_device()`] to ask the user for permission before
//! any USB device can be used through this API.
//!

#![warn(missing_docs)]

use std::{
    fmt,
    marker::PhantomData,
    pin::Pin,
    task::{ready, Context, Poll},
};

use futures_core::Stream;
use futures_util::StreamExt;
use js_sys::{Reflect, Uint8Array};
use tokio::sync::broadcast;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// WebUSB error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    msg: String,
}

impl Error {
    /// Error kind.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Error message.
    pub fn msg(&self) -> &str {
        &self.msg
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, &self.msg)
    }
}

impl std::error::Error for Error {}

/// WebUSB error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// WebUSB is unsupported by this browser.
    Unsupported,
    /// The USB device has already been opened.
    AlreadyOpen,
    /// The USB device has been disconnected.
    Disconnected,
    /// Access denied.
    Security,
    /// The USB device stalled the transfer to indicate an error.
    ///
    /// This condition can be reset by calling [`OpenUsbDevice::clear_halt`].
    Stall,
    /// The USB device sent too much data.
    Babble,
    /// USB transfer failed.
    Transfer,
    /// Invalid access.
    InvalidAccess,
    /// Other error.
    Other,
}

impl Error {
    fn new(kind: ErrorKind, msg: impl AsRef<str>) -> Self {
        Self { kind, msg: msg.as_ref().to_string() }
    }
}

impl From<JsValue> for Error {
    fn from(value: JsValue) -> Self {
        if let Some(js_error) = value.dyn_ref::<js_sys::Error>() {
            let msg = js_error.message().as_string().unwrap();
            let kind = match js_error.name().as_string().unwrap().as_str() {
                "NotFoundError" => ErrorKind::Disconnected,
                "SecurityError" => ErrorKind::Security,
                "InvalidAccessError" => ErrorKind::InvalidAccess,
                "NetworkError" => ErrorKind::Transfer,
                _ => ErrorKind::Other,
            };
            return Error::new(kind, msg);
        }

        let msg = value.as_string().unwrap_or_else(|| "unknown error".into());
        Error::new(ErrorKind::Other, msg)
    }
}

/// WebUSB result.
pub type Result<T> = std::result::Result<T, Error>;

/// A configuration belonging to a USB device.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UsbConfiguration {
    /// The numeric value identifying this configuration.
    pub configuration_value: u8,
    /// Optional name describing this configuration.
    pub configuration_name: Option<String>,
    /// The interfaces available under this configuration.
    pub interfaces: Vec<UsbInterface>,
}

impl From<&web_sys::UsbConfiguration> for UsbConfiguration {
    fn from(conf: &web_sys::UsbConfiguration) -> Self {
        let iface_list = conf.interfaces();
        let mut interfaces = Vec::new();
        for i in 0..iface_list.length() {
            if let Some(iface) = iface_list.get(i).dyn_ref::<web_sys::UsbInterface>() {
                interfaces.push(UsbInterface::from(iface));
            }
        }
        Self {
            configuration_value: conf.configuration_value(),
            configuration_name: conf.configuration_name(),
            interfaces,
        }
    }
}

/// A USB interface grouping one or more alternate settings.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UsbInterface {
    /// The interface number.
    pub interface_number: u8,
    /// The alternate settings for this interface.
    pub alternates: Vec<UsbAlternateSetting>,
}

impl From<&web_sys::UsbInterface> for UsbInterface {
    fn from(iface: &web_sys::UsbInterface) -> Self {
        let alt_list = iface.alternates();
        let mut alternates = Vec::new();
        for i in 0..alt_list.length() {
            if let Some(alt) = alt_list.get(i).dyn_ref::<web_sys::UsbAlternateInterface>() {
                alternates.push(UsbAlternateSetting::from(alt));
            }
        }
        Self { interface_number: iface.interface_number(), alternates }
    }
}

/// An alternate setting containing detailed interface information.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UsbAlternateSetting {
    /// The alternate setting value.
    pub alternate_setting: u8,
    /// The interface class code.
    pub interface_class: u8,
    /// The interface subclass code.
    pub interface_subclass: u8,
    /// The interface protocol code.
    pub interface_protocol: u8,
    /// Optional name for this interface alternate.
    pub interface_name: Option<String>,
    /// The endpoints belonging to this alternate setting.
    pub endpoints: Vec<UsbEndpoint>,
}

impl From<&web_sys::UsbAlternateInterface> for UsbAlternateSetting {
    fn from(alt: &web_sys::UsbAlternateInterface) -> Self {
        let ep_list = alt.endpoints();
        let mut endpoints = Vec::new();
        for i in 0..ep_list.length() {
            if let Some(ep) = ep_list.get(i).dyn_ref::<web_sys::UsbEndpoint>() {
                endpoints.push(UsbEndpoint::from(ep));
            }
        }
        Self {
            alternate_setting: alt.alternate_setting(),
            interface_class: alt.interface_class(),
            interface_subclass: alt.interface_subclass(),
            interface_protocol: alt.interface_protocol(),
            interface_name: alt.interface_name(),
            endpoints,
        }
    }
}

/// A USB endpoint used for communication with a device.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UsbEndpoint {
    /// The endpoint number.
    pub endpoint_number: u8,
    /// The direction of transfer (e.g. "in" or "out").
    pub direction: UsbDirection,
    /// The transfer type (e.g. "bulk", "interrupt", or "isochronous").
    pub endpoint_type: UsbEndpointType,
    /// The maximum packet size for this endpoint.
    pub packet_size: u32,
}

impl From<&web_sys::UsbEndpoint> for UsbEndpoint {
    fn from(ep: &web_sys::UsbEndpoint) -> Self {
        Self {
            endpoint_number: ep.endpoint_number(),
            direction: ep.direction().into(),
            endpoint_type: ep.type_().into(),
            packet_size: ep.packet_size(),
        }
    }
}

/// USB endpoint type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsbEndpointType {
    /// Provides reliable data transfer for large payloads.
    ///
    /// Data sent through a bulk endpoint is guaranteed to be delivered
    /// or generate an error but may be preempted by other data traffic.
    Bulk,
    /// Provides reliable data transfer for small payloads.
    ///
    /// Data sent through an interrupt endpoint is guaranteed to be
    /// delivered or generate an error and is also given dedicated bus time
    /// for transmission.
    Interrupt,
    /// Provides unreliable data transfer for payloads that must be delivered
    /// periodically.
    ///
    /// They are given dedicated bus time but if a deadline is missed the data is dropped.
    Isochronous,
}

impl From<web_sys::UsbEndpointType> for UsbEndpointType {
    fn from(value: web_sys::UsbEndpointType) -> Self {
        match value {
            web_sys::UsbEndpointType::Bulk => Self::Bulk,
            web_sys::UsbEndpointType::Interrupt => Self::Interrupt,
            web_sys::UsbEndpointType::Isochronous => Self::Isochronous,
            other => unreachable!("unsupported UsbEndpointType: {other:?}"),
        }
    }
}

/// A USB device.
#[derive(Clone)]
pub struct UsbDevice {
    device: web_sys::UsbDevice,
}

impl UsbDevice {
    /// Manufacturer-provided vendor identifier.
    pub fn vendor_id(&self) -> u16 {
        self.device.vendor_id()
    }

    /// Manufacturer-provided product identifier.
    pub fn product_id(&self) -> u16 {
        self.device.product_id()
    }

    /// Device class code.
    pub fn device_class(&self) -> u8 {
        self.device.device_class()
    }

    /// Device subclass code.
    pub fn device_subclass(&self) -> u8 {
        self.device.device_subclass()
    }

    /// Device protocol code.
    pub fn device_protocol(&self) -> u8 {
        self.device.device_protocol()
    }

    /// Major version of the device.
    pub fn device_version_major(&self) -> u8 {
        self.device.device_version_major()
    }

    /// Minor version of the device.
    pub fn device_version_minor(&self) -> u8 {
        self.device.device_version_minor()
    }

    /// Subminor version of the device.
    pub fn device_version_subminor(&self) -> u8 {
        self.device.device_version_subminor()
    }

    /// Optional manufacturer name.
    pub fn manufacturer_name(&self) -> Option<String> {
        self.device.manufacturer_name()
    }

    /// Optional product name.
    pub fn product_name(&self) -> Option<String> {
        self.device.product_name()
    }

    /// Optional serial number of the device.
    pub fn serial_number(&self) -> Option<String> {
        self.device.serial_number()
    }

    /// Indicates if the device is currently opened.
    pub fn opened(&self) -> bool {
        self.device.opened()
    }

    /// Active configuration value if any.
    pub fn configuration(&self) -> Option<UsbConfiguration> {
        self.device.configuration().map(|cfg| (&cfg).into())
    }

    /// All available configurations for this device.
    pub fn configurations(&self) -> Vec<UsbConfiguration> {
        let cfg_list = self.device.configurations();
        let mut configurations = Vec::new();
        for i in 0..cfg_list.length() {
            if let Some(conf) = cfg_list.get(i).dyn_ref::<web_sys::UsbConfiguration>() {
                configurations.push(UsbConfiguration::from(conf));
            }
        }
        configurations
    }

    /// End the device session and relinquish all obtained permissions to
    /// access the USB device.
    pub async fn forget(&self) {
        JsFuture::from(self.device.forget()).await.unwrap();
    }

    /// Open the USB device to allow USB transfers.
    ///
    /// A device can only be open once.
    pub async fn open(&self) -> Result<OpenUsbDevice> {
        if self.opened() {
            return Err(Error::new(ErrorKind::AlreadyOpen, "USB device is already open"));
        }

        JsFuture::from(self.device.open()).await?;
        Ok(OpenUsbDevice { device: self.clone(), closed: false })
    }
}

impl std::fmt::Debug for UsbDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("UsbDevice")
            .field("vendor_id", &self.vendor_id())
            .field("product_id", &self.product_id())
            .field("device_class", &self.device_class())
            .field("device_subclass", &self.device_subclass())
            .field("device_protocol", &self.device_protocol())
            .field("device_version_major", &self.device_version_major())
            .field("device_version_minor", &self.device_version_minor())
            .field("device_version_subminor", &self.device_version_subminor())
            .field("manufacturer_name", &self.manufacturer_name())
            .field("product_name", &self.product_name())
            .field("serial_number", &self.serial_number())
            .field("opened", &self.opened())
            .field("configuration", &self.configuration())
            .field("configurations", &self.configurations())
            .finish()
    }
}

impl From<web_sys::UsbDevice> for UsbDevice {
    fn from(device: web_sys::UsbDevice) -> Self {
        Self { device }
    }
}

/// USB transfer direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsbDirection {
    /// Data is transferred from device to host.
    In,
    /// Data is transferred from host to device.
    Out,
}

impl From<web_sys::UsbDirection> for UsbDirection {
    fn from(value: web_sys::UsbDirection) -> Self {
        match value {
            web_sys::UsbDirection::In => Self::In,
            web_sys::UsbDirection::Out => Self::Out,
            other => unreachable!("unsupported UsbDirection {other:?}"),
        }
    }
}

impl From<UsbDirection> for web_sys::UsbDirection {
    fn from(direction: UsbDirection) -> Self {
        match direction {
            UsbDirection::In => web_sys::UsbDirection::In,
            UsbDirection::Out => web_sys::UsbDirection::Out,
        }
    }
}

/// A filter used to match specific USB devices by various criteria.
///
/// Fields left as `None` will match any value in that field.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct UsbDeviceFilter {
    /// Optional USB vendor ID.
    pub vendor_id: Option<u16>,
    /// Optional USB product ID.
    pub product_id: Option<u16>,
    /// Optional USB device class code.
    pub class_code: Option<u8>,
    /// Optional USB device subclass code.
    pub subclass_code: Option<u8>,
    /// Optional USB device protocol code.
    pub protocol_code: Option<u8>,
    /// Optional USB device serial number.
    pub serial_number: Option<String>,
}

impl UsbDeviceFilter {
    /// Creates a new, empty USB device filter.
    pub fn new() -> Self {
        Self::default()
    }
}

impl From<&UsbDeviceFilter> for web_sys::UsbDeviceFilter {
    fn from(value: &UsbDeviceFilter) -> Self {
        let filter = web_sys::UsbDeviceFilter::new();
        if let Some(x) = value.vendor_id {
            filter.set_vendor_id(x);
        }
        if let Some(x) = value.product_id {
            filter.set_product_id(x);
        }
        if let Some(x) = value.class_code {
            filter.set_class_code(x);
        }
        if let Some(x) = value.subclass_code {
            filter.set_subclass_code(x);
        }
        if let Some(x) = value.protocol_code {
            filter.set_protocol_code(x);
        }
        if let Some(x) = &value.serial_number {
            filter.set_serial_number(x);
        }
        filter
    }
}

/// The recipient of a USB control transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsbRecipient {
    /// The request is intended for the USB device as a whole.
    Device,
    /// The request is intended for a specific interface on the USB device.
    Interface,
    /// The request is intended for a specific endpoint on the USB device.
    Endpoint,
    /// The request is intended for some other recipient.
    Other,
}

impl From<UsbRecipient> for web_sys::UsbRecipient {
    fn from(recipient: UsbRecipient) -> Self {
        match recipient {
            UsbRecipient::Device => Self::Device,
            UsbRecipient::Interface => Self::Interface,
            UsbRecipient::Endpoint => Self::Endpoint,
            UsbRecipient::Other => Self::Other,
        }
    }
}

/// The type of USB control request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsbRequestType {
    /// A standard request defined by the USB specification.
    Standard,
    /// A class-specific request.
    Class,
    /// A vendor-specific request.
    Vendor,
}

impl From<UsbRequestType> for web_sys::UsbRequestType {
    fn from(req_type: UsbRequestType) -> Self {
        match req_type {
            UsbRequestType::Standard => Self::Standard,
            UsbRequestType::Class => Self::Class,
            UsbRequestType::Vendor => Self::Vendor,
        }
    }
}

/// USB device request options.
#[derive(Clone, Debug)]
struct UsbDeviceRequestOptions {
    /// An array of filter objects for possible devices you would like to pair.
    pub filters: Vec<UsbDeviceFilter>,
}

impl UsbDeviceRequestOptions {
    /// Creates new USB device request options with the specified device filter.
    pub fn new(filters: impl IntoIterator<Item = UsbDeviceFilter>) -> Self {
        Self { filters: filters.into_iter().collect() }
    }
}

impl From<&UsbDeviceRequestOptions> for web_sys::UsbDeviceRequestOptions {
    fn from(value: &UsbDeviceRequestOptions) -> Self {
        let filters = js_sys::Array::new();
        for filter in &value.filters {
            filters.push(&web_sys::UsbDeviceFilter::from(filter));
        }

        web_sys::UsbDeviceRequestOptions::new(&filters)
    }
}

/// WebUSB event.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum UsbEvent {
    /// USB device was connected.
    Connected(UsbDevice),
    /// USB device was disconnected.
    Disconnected(UsbDevice),
}

/// Wrapper for making any type [Send].
#[derive(Debug, Clone)]
struct SendWrapper<T>(pub T);
unsafe impl<T> Send for SendWrapper<T> {}

/// WebUSB event stream.
///
/// Provides device change events for paired devices.
pub struct UsbEvents {
    // We wrap UsbEvent in SendWrapper to allow the use of
    // BroadcastStream. However, we need to ensure that UsbEvents
    // is !Send.
    rx: BroadcastStream<SendWrapper<UsbEvent>>,
    _marker: PhantomData<*const ()>,
}

impl fmt::Debug for UsbEvents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("UsbEvents").finish()
    }
}

impl Stream for UsbEvents {
    type Item = UsbEvent;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            match ready!(self.rx.poll_next_unpin(cx)) {
                Some(Ok(event)) => break Poll::Ready(Some(event.0)),
                Some(Err(BroadcastStreamRecvError::Lagged(_))) => (),
                None => break Poll::Ready(None),
            }
        }
    }
}

/// WebUSB device enumeration and connection.
pub struct Usb {
    usb: web_sys::Usb,
    event_rx: broadcast::Receiver<SendWrapper<UsbEvent>>,
}

impl fmt::Debug for Usb {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Usb").finish()
    }
}

impl Usb {
    /// Checks that WebUSB is available and obtains access to the WebUSB API.
    pub fn new() -> Result<Self> {
        let usb = Self::browser_usb()?;

        let (event_tx, event_rx) = broadcast::channel(1024);

        let on_connect = {
            let event_tx = event_tx.clone();
            Closure::wrap(Box::new(move |event: web_sys::UsbConnectionEvent| {
                let _ = event_tx.send(SendWrapper(UsbEvent::Connected(event.device().into())));
            }) as Box<dyn Fn(_)>)
        };
        usb.set_onconnect(Some(on_connect.into_js_value().unchecked_ref()));

        let on_disconnect = {
            let event_tx = event_tx.clone();
            Closure::wrap(Box::new(move |event: web_sys::UsbConnectionEvent| {
                let _ = event_tx.send(SendWrapper(UsbEvent::Disconnected(event.device().into())));
            }) as Box<dyn Fn(_)>)
        };
        usb.set_ondisconnect(Some(on_disconnect.into_js_value().unchecked_ref()));

        Ok(Self { usb, event_rx })
    }

    fn browser_usb() -> Result<web_sys::Usb> {
        let global = js_sys::global();

        if let Some(window) = global.dyn_ref::<web_sys::Window>() {
            let navigator = window.navigator();
            match Reflect::get(&navigator, &JsValue::from_str("usb")) {
                Ok(usb) if !usb.is_null() && !usb.is_undefined() => return Ok(navigator.usb()),
                _ => (),
            }
        }

        if let Some(worker) = global.dyn_ref::<web_sys::WorkerGlobalScope>() {
            let navigator = worker.navigator();
            match Reflect::get(&navigator, &JsValue::from_str("usb")) {
                Ok(usb) if !usb.is_null() && !usb.is_undefined() => return Ok(navigator.usb()),
                _ => (),
            }
        }

        Err(Error::new(ErrorKind::Unsupported, "browser does not support WebUSB"))
    }

    /// Subscribe to a stream of [`UsbEvent`]s notifying of USB device changes.
    ///
    /// Only events for paired devices will be provided.
    pub fn events(&self) -> UsbEvents {
        UsbEvents { rx: self.event_rx.resubscribe().into(), _marker: PhantomData }
    }

    /// List of paired attached devices.
    ///
    /// For information on pairing devices, see [`request_device`](Self::request_device).
    pub async fn devices(&self) -> Vec<UsbDevice> {
        let list = JsFuture::from(self.usb.get_devices()).await.unwrap();
        js_sys::Array::from(&list)
            .iter()
            .map(|dev| UsbDevice::from(dev.dyn_into::<web_sys::UsbDevice>().unwrap()))
            .collect()
    }

    /// Pairs a USB device with the specified filter criteria.
    ///
    /// Calling this function triggers the user agent's pairing flow.
    pub async fn request_device(&self, filters: impl IntoIterator<Item = UsbDeviceFilter>) -> Result<UsbDevice> {
        let opts = &UsbDeviceRequestOptions::new(filters);
        let dev = JsFuture::from(self.usb.request_device(&opts.into())).await?;
        Ok(dev.dyn_into::<web_sys::UsbDevice>().unwrap().into())
    }
}

impl Drop for Usb {
    fn drop(&mut self) {
        self.usb.set_onconnect(None);
        self.usb.set_ondisconnect(None);
    }
}

/// An opened USB device.
///
/// Dropping this causes the USB device to be closed.
pub struct OpenUsbDevice {
    device: UsbDevice,
    closed: bool,
}

impl fmt::Debug for OpenUsbDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("OpenUsbDevice").field("device", &self.device).finish()
    }
}

impl AsRef<UsbDevice> for OpenUsbDevice {
    fn as_ref(&self) -> &UsbDevice {
        &self.device
    }
}

impl OpenUsbDevice {
    fn dev(&self) -> &web_sys::UsbDevice {
        &self.device.device
    }

    /// The USB device.
    pub fn device(&self) -> &UsbDevice {
        &self.device
    }

    /// Releases all open interfaces and ends the device session.
    ///
    /// It is not necessary to call this method, since dropping
    /// [OpenUsbDevice] will also close the USB device.
    pub async fn close(mut self) -> Result<()> {
        self.closed = true;
        JsFuture::from(self.dev().close()).await?;
        Ok(())
    }

    /// Resets the device and cancels all pending operations.
    pub async fn reset(&self) -> Result<()> {
        JsFuture::from(self.dev().reset()).await?;
        Ok(())
    }

    /// Selects the USB device configuration with the specified index.
    pub async fn select_configuration(&self, configuration: u8) -> Result<()> {
        JsFuture::from(self.dev().select_configuration(configuration)).await?;
        Ok(())
    }

    /// Claim specified interface for exclusive access.
    pub async fn claim_interface(&self, interface: u8) -> Result<()> {
        JsFuture::from(self.dev().claim_interface(interface)).await?;
        Ok(())
    }

    /// Release specified interface from exclusive access.
    pub async fn release_interface(&self, interface: u8) -> Result<()> {
        JsFuture::from(self.dev().release_interface(interface)).await?;
        Ok(())
    }

    /// Selects the alternate setting with the specified index for an interface.
    pub async fn select_alternate_interface(&self, interface: u8, alternate: u8) -> Result<()> {
        JsFuture::from(self.dev().select_alternate_interface(interface, alternate)).await?;
        Ok(())
    }

    /// Clears a halt condition.
    ///
    /// A halt condition is when a data transfer to or from the device has a status of 'stall',
    /// which requires the web page (the host system, in USB terminology) to clear that condition.
    pub async fn clear_halt(&self, direction: UsbDirection, endpoint: u8) -> Result<()> {
        JsFuture::from(self.dev().clear_halt(direction.into(), endpoint)).await?;
        Ok(())
    }

    /// Check transfer status.
    fn check_status(status: web_sys::UsbTransferStatus) -> Result<()> {
        match status {
            web_sys::UsbTransferStatus::Ok => Ok(()),
            web_sys::UsbTransferStatus::Stall => Err(Error::new(ErrorKind::Stall, "stall condition")),
            web_sys::UsbTransferStatus::Babble => Err(Error::new(ErrorKind::Babble, "device babbled")),
            other => unreachable!("unsupported UsbTransferStatus {other:?}"),
        }
    }

    /// Perform a control transfer from device to host.
    pub async fn control_transfer_in(
        &self, recipient: UsbRecipient, request_type: UsbRequestType, request: u8, value: u16, index: u16,
        len: u16,
    ) -> Result<Vec<u8>> {
        let setup = web_sys::UsbControlTransferParameters::new(
            index,
            recipient.into(),
            request,
            request_type.into(),
            value,
        );

        let res = JsFuture::from(self.dev().control_transfer_in(&setup, len)).await?;
        let res = res.dyn_into::<web_sys::UsbInTransferResult>().unwrap();

        Self::check_status(res.status())?;

        let data = Uint8Array::new(&res.data().unwrap().buffer()).to_vec();
        Ok(data)
    }

    /// Perform a control transfer from host to device.
    pub async fn control_transfer_out(
        &self, recipient: UsbRecipient, request_type: UsbRequestType, request: u8, value: u16, index: u16,
        data: &[u8],
    ) -> Result<u32> {
        let setup = web_sys::UsbControlTransferParameters::new(
            index,
            recipient.into(),
            request,
            request_type.into(),
            value,
        );

        let data = Uint8Array::from(data);
        let res = JsFuture::from(self.dev().control_transfer_out_with_u8_array(&setup, &data)?).await?;
        let res = res.dyn_into::<web_sys::UsbOutTransferResult>().unwrap();

        Self::check_status(res.status())?;
        Ok(res.bytes_written())
    }

    /// Transmits time sensitive information from the device.
    pub async fn isochronous_transfer_in(
        &self, endpoint: u8, packet_lens: impl IntoIterator<Item = u32>,
    ) -> Result<Vec<Result<Vec<u8>>>> {
        let array: js_sys::Array = packet_lens.into_iter().map(|len| JsValue::from_f64(len as _)).collect();

        let res = JsFuture::from(self.dev().isochronous_transfer_in(endpoint, &array)).await?;
        let res = res.dyn_into::<web_sys::UsbIsochronousInTransferResult>().unwrap();

        let mut results = Vec::new();
        for packet in res.packets() {
            let packet = packet.dyn_into::<web_sys::UsbIsochronousInTransferPacket>().unwrap();
            let result = match Self::check_status(packet.status()) {
                Ok(()) => Ok(Uint8Array::new(&res.data().unwrap().buffer()).to_vec()),
                Err(err) => Err(err),
            };
            results.push(result);
        }

        Ok(results)
    }

    /// Transmits time sensitive information to the device.
    ///
    /// Returns the number of bytes sent of each packet.
    pub async fn isochronous_transfer_out(
        &self, endpoint: u8, packets: impl IntoIterator<Item = &[u8]>,
    ) -> Result<Vec<Result<u32>>> {
        let mut data = Vec::new();
        let mut lens = Vec::new();

        for packet in packets {
            data.extend_from_slice(packet);
            lens.push(data.len());
        }

        let data = Uint8Array::from(&data[..]);
        let lens: js_sys::Array = lens.into_iter().map(|len| JsValue::from_f64(len as _)).collect();

        let res =
            JsFuture::from(self.dev().isochronous_transfer_out_with_u8_array(endpoint, &data, &lens)?).await?;
        let res = res.dyn_into::<web_sys::UsbIsochronousOutTransferResult>().unwrap();

        let mut results = Vec::new();
        for packet in res.packets() {
            let packet = packet.dyn_into::<web_sys::UsbIsochronousOutTransferPacket>().unwrap();
            let result = match Self::check_status(packet.status()) {
                Ok(()) => Ok(packet.bytes_written()),
                Err(err) => Err(err),
            };
            results.push(result);
        }

        Ok(results)
    }

    /// Performs a bulk or interrupt transfer from specified endpoint of the device.
    pub async fn transfer_in(&self, endpoint: u8, len: u32) -> Result<Vec<u8>> {
        let res = JsFuture::from(self.dev().transfer_in(endpoint, len)).await?;
        let res = res.dyn_into::<web_sys::UsbInTransferResult>().unwrap();

        Self::check_status(res.status())?;

        let data = Uint8Array::new(&res.data().unwrap().buffer()).to_vec();
        Ok(data)
    }

    /// Performs a bulk or interrupt transfer to the specified endpoint of the device.
    ///
    /// Returns the number of bytes sent.
    pub async fn transfer_out(&self, endpoint: u8, data: &[u8]) -> Result<u32> {
        let data = Uint8Array::from(data);
        let res = JsFuture::from(self.dev().transfer_out_with_u8_array(endpoint, &data)?).await?;
        let res = res.dyn_into::<web_sys::UsbOutTransferResult>().unwrap();

        Self::check_status(res.status())?;

        Ok(res.bytes_written())
    }
}

impl Drop for OpenUsbDevice {
    fn drop(&mut self) {
        if !self.closed {
            let device = self.dev().clone();
            let fut = JsFuture::from(device.close());
            spawn_local(async move {
                let _ = fut.await;
            });
        }
    }
}
