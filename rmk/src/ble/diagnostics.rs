//! Opt-in sensor packets; no key events, settings writes, or flash access.
use crate::RawMutex;
use core::sync::atomic::{AtomicU8, Ordering};
use embassy_sync::channel::Channel;
use trouble_host::prelude::*;

pub const SERVICE_UUID: &str = "ef558a00-6fa0-4c67-aa6e-06b1cdf63192";
pub static PACKETS: Channel<RawMutex, [u8; 20], 32> = Channel::new();
static MODE: AtomicU8 = AtomicU8::new(0);

#[gatt_service(uuid = "ef558a00-6fa0-4c67-aa6e-06b1cdf63192")]
pub(crate) struct DiagnosticService {
    #[characteristic(uuid = "ef558a01-6fa0-4c67-aa6e-06b1cdf63192", write)]
    pub(crate) command: [u8; 4],
    #[characteristic(uuid = "ef558a02-6fa0-4c67-aa6e-06b1cdf63192", notify)]
    pub(crate) data: [u8; 20],
}

pub fn mode() -> u8 {
    MODE.load(Ordering::Relaxed)
}
pub fn finish_capture() {
    let _ = MODE.compare_exchange(2, 1, Ordering::Relaxed, Ordering::Relaxed);
}

pub(crate) fn command(bytes: &[u8]) -> Result<(), AttErrorCode> {
    if bytes.len() != 4 || bytes[..3] != *b"RT\x01" || bytes[3] > 2 {
        return Err(AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH);
    }
    if bytes[3] == 0 {
        PACKETS.clear();
    }
    MODE.store(bytes[3], Ordering::Relaxed);
    Ok(())
}

pub(crate) struct Connected;
pub(crate) fn connected() -> Connected {
    MODE.store(0, Ordering::Relaxed);
    PACKETS.clear();
    Connected
}
impl Drop for Connected {
    fn drop(&mut self) {
        MODE.store(0, Ordering::Relaxed);
        PACKETS.clear();
    }
}

pub(crate) async fn run<P: PacketPool>(data: Characteristic<[u8; 20]>, conn: &GattConnection<'_, '_, P>) {
    loop {
        let packet = PACKETS.receive().await;
        if mode() != 0 && data.notify(conn, &packet, true).await.is_err() {
            MODE.store(0, Ordering::Relaxed);
            PACKETS.clear();
        }
    }
}
