use futures_util::StreamExt;
use tokio::sync::oneshot;
use wasm_bindgen_futures::spawn_local;
use wasm_bindgen_test::wasm_bindgen_test;

mod util;
use util::{wait_for_interaction, ResultExt};

use webusb_web::*;

#[wasm_bindgen_test]
async fn test() {
    log!("Getting WebUSB API");
    let usb = Usb::new().expect_log("cannot get WebUSB API");
    log!("Obtained WebUSB API");

    let mut filter = UsbDeviceFilter::new();
    filter.vendor_id = Some(0x06);
    filter.product_id = Some(0x11);

    log!("Starting event stream");
    let mut events = usb.events();
    let (disconnected_tx, disconnected_rx) = oneshot::channel();
    spawn_local(async move {
        let mut disconnected_tx = Some(disconnected_tx);
        while let Some(event) = events.next().await {
            log!("WebUSB event: {event:?}");

            match &event {
                UsbEvent::Disconnected(dev)
                    if dev.vendor_id() == filter.vendor_id.unwrap_log()
                        && dev.product_id() == filter.product_id.unwrap_log() =>
                {
                    if let Some(disconnected_tx) = disconnected_tx.take() {
                        disconnected_tx.send(()).unwrap();
                    }
                }
                _ => (),
            }
        }
    });
    log!("Event stream started");

    log!("Enumerating devices. This is expected to be empty when no devices are paried.");
    let devices = usb.devices().await;
    for device in devices {
        log!("Enumerated USB device: {device:?}");
    }
    log!("Enumeration complete");

    wait_for_interaction(
        "\
        Please connect a Linux device supporting USB gadget mode and run the \
        <pre>custom_interface_device</pre> example from the \
        usb-gadget repository at <pre>https://github.com/surban/usb-gadget</pre>\
        <br>\
        Then click here to enable permission requests and continue.\
        Then select the USB device from the popup shown by your browser.\
    ",
    )
    .await;

    log!("Requesting device with filter {filter:?}");
    let res = usb.request_device([filter.clone()]).await;
    log!("Request result: {res:?}");

    let Ok(dev) = res else {
        log!("No device obtained, exiting.");
        panic!("No device obtained");
    };
    assert!(
        dev.vendor_id() == filter.vendor_id.unwrap_log() && dev.product_id() == filter.product_id.unwrap_log()
    );

    log!("Enumerating devices. The device selected should now be visible.");
    let devices = usb.devices().await;
    let mut found = false;
    for device in devices {
        log!("Enumerated USB device: {device:?}");
        if device.vendor_id() == filter.vendor_id.unwrap_log()
            && device.product_id() == filter.product_id.unwrap_log()
        {
            found = true;
            log!("Device found!");
        }
    }
    assert!(found, "device not enumerated after paired");
    log!("Enumeration complete");

    let cfg = dev.configuration().expect_log("device has no active configuration");
    log!("Active device configuration: {cfg:?}");

    let iface = cfg.interfaces.first().unwrap_log();
    let alt = &iface.alternate;
    assert_eq!(alt.alternate_setting, 0);
    assert_eq!(alt.interface_class, 255);
    assert_eq!(alt.interface_subclass, 1);
    assert_eq!(alt.interface_protocol, 2);

    let mut in_packet_size = None;
    let mut in_ep = None;
    let mut out_packet_size = None;
    let mut out_ep = None;
    for ep in &alt.endpoints {
        match ep.direction {
            UsbDirection::In => {
                log!("Found in endpoint: {ep:?}");
                in_ep = Some(ep.endpoint_number);
                in_packet_size = Some(ep.packet_size);
            }
            UsbDirection::Out => {
                log!("Found out endpoint: {ep:?}");
                out_ep = Some(ep.endpoint_number);
                out_packet_size = Some(ep.packet_size);
            }
        }
    }
    let in_packet_size = in_packet_size.unwrap_log();
    let in_ep = in_ep.unwrap_log();
    let out_packet_size = out_packet_size.unwrap_log();
    let out_ep = out_ep.unwrap_log();

    log!("Opening device");
    let open = dev.open().await.expect_log("device failed to open");

    log!("Claiming interface");
    open.claim_interface(0).await.expect_log("failed to claim interface");
    log!("Interface claimed");

    let send_task = async {
        let mut b = 0;
        for _ in 0..1024 {
            log!("Sending {out_packet_size} bytes of {b:x}");
            let data = vec![b; out_packet_size as usize];
            let n = open.transfer_out(out_ep, &data).await.unwrap_log();
            assert_eq!(n, out_packet_size);

            b = b.wrapping_add(1);
        }
    };

    let mut recved = 0;
    let recv_task = async {
        let mut b = None;
        loop {
            let data = open.transfer_in(in_ep, in_packet_size).await.unwrap_log();
            log!("Received {} bytes: {:x?}...", data.len(), &data[..16]);
            assert_eq!(data.len(), in_packet_size as usize);

            let b = b.get_or_insert(data[0]);
            assert!(data.iter().all(|x| x == b));
            *b = b.wrapping_add(1);

            recved += 1;
        }
    };

    let ctrl_task = async {
        for i in 0x00..0xff {
            let data = vec![i; i as usize];
            let control = UsbControlRequest::new(
                UsbRequestType::Class,
                UsbRecipient::Interface,
                i,
                0xfffe,
                iface.interface_number.into(),
            );
            let n = open.control_transfer_out(&control, &data).await.unwrap_log();
            assert_eq!(n as usize, data.len());
            log!("Sent control request {i:x} of size {} bytes", data.len());

            let back = open.control_transfer_in(&control, i as _).await.unwrap_log();
            log!("Received data from control request {i:x}");

            assert_eq!(back, data);
            log!("Received data matches sent data");
        }
    };

    tokio::select! {
        () = recv_task => (),
        _ = async { tokio::join!(send_task, ctrl_task) } => (),
    }

    assert!(recved >= 100, "not enough packets received");

    log!("Clearing halt");
    open.clear_halt(UsbDirection::In, in_ep).await.unwrap_log();
    open.clear_halt(UsbDirection::Out, out_ep).await.unwrap_log();

    log!("Closing device");
    open.close().await.unwrap_log();

    log!("Reopening device");
    let open = dev.open().await.expect_log("device failed to open");

    log!("Reclaiming interface");
    open.claim_interface(0).await.expect_log("failed to claim interface");
    log!("Interface claimed");

    log!("Terminating USB gadget");
    let control = UsbControlRequest::new(
        UsbRequestType::Class,
        UsbRecipient::Interface,
        0xff,
        0,
        iface.interface_number.into(),
    );
    open.control_transfer_out(&control, &[]).await.unwrap_log();

    log!("Waiting for disconnect event");
    disconnected_rx.await.unwrap_log();

    log!("Device disconnected");
    dev.forget().await;
}
