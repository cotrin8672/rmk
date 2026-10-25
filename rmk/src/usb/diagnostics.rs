//! Sensor-only HID endpoint. Three 20-byte records per 64-byte response.
use super::diagnostic_protocol::Buffer;
use crate::RawMutex;
use core::cell::RefCell;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Instant, Timer};
use embassy_usb::{
    Builder,
    class::hid::{Config, HidReaderWriter, State},
    driver::Driver,
};
use static_cell::StaticCell;

static BUFFER: Mutex<RawMutex, RefCell<Buffer>> = Mutex::new(RefCell::new(Buffer::new()));
fn now() -> u32 {
    Instant::now().as_millis() as u32
}
pub fn mode() -> u8 {
    BUFFER.lock(|b| b.borrow_mut().mode(now()))
}
pub fn try_send(packet: [u8; 20]) -> bool {
    BUFFER.lock(|b| b.borrow_mut().push(packet, now()))
}
pub fn finish_capture() {
    BUFFER.lock(|b| b.borrow_mut().finish_capture());
}
pub async fn send(packet: [u8; 20]) -> bool {
    let start = Instant::now();
    loop {
        if mode() == 0 {
            return false;
        }
        if BUFFER.lock(|b| {
            let mut b = b.borrow_mut();
            !b.full() && b.push(packet, now())
        }) {
            return true;
        }
        if start.elapsed().as_millis() > 250 {
            return false;
        }
        Timer::after_millis(1).await;
    }
}

pub(crate) fn add<D: Driver<'static>>(builder: &mut Builder<'static, D>) -> HidReaderWriter<'static, D, 64, 64> {
    static STATE: StaticCell<State> = StaticCell::new();
    let config = Config {
        report_descriptor: &[
            0x06, 0x62, 0xff, 0x09, 0x61, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x40, 0x15, 0x00, 0x26, 0xff, 0x00, 0x09, 0x62,
            0x81, 0x02, 0x95, 0x40, 0x09, 0x63, 0x91, 0x02, 0xc0,
        ],
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
        hid_subclass: embassy_usb::class::hid::HidSubclass::No,
        hid_boot_protocol: embassy_usb::class::hid::HidBootProtocol::None,
    };
    HidReaderWriter::new(builder, STATE.init(State::new()), config)
}

pub(crate) async fn run<D: Driver<'static>>(rw: &mut HidReaderWriter<'static, D, 64, 64>) -> ! {
    let mut command = [0u8; 64];
    loop {
        let length = match rw.read(&mut command).await {
            Ok(n) => n,
            Err(_) => {
                BUFFER.lock(|b| b.borrow_mut().stop());
                Timer::after_millis(100).await;
                continue;
            }
        };
        let reply = BUFFER.lock(|b| b.borrow_mut().command(&command[..length], now()));
        if let Some(reply) = reply {
            if embassy_time::with_timeout(embassy_time::Duration::from_millis(500), rw.write(&reply))
                .await
                .map_or(true, |r| r.is_err())
            {
                BUFFER.lock(|b| b.borrow_mut().stop());
            }
        }
    }
}
