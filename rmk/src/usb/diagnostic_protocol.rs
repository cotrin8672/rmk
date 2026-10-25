//! Pure USB diagnostics queue/protocol; runnable with rustc --test.
pub const CAPACITY: usize = 128;
pub struct Buffer {
    packets: [[u8; 20]; CAPACITY],
    head: usize,
    len: usize,
    mode: u8,
    last_host: u32,
    overflow: bool,
}
impl Buffer {
    pub const fn new() -> Self {
        Self {
            packets: [[0; 20]; CAPACITY],
            head: 0,
            len: 0,
            mode: 0,
            last_host: 0,
            overflow: false,
        }
    }
    pub fn stop(&mut self) {
        self.mode = 0;
        self.head = 0;
        self.len = 0;
        self.overflow = false;
    }
    pub fn mode(&mut self, now: u32) -> u8 {
        if now.wrapping_sub(self.last_host) > 2000 {
            self.stop();
        }
        self.mode
    }
    pub fn full(&self) -> bool {
        self.len == CAPACITY
    }
    pub fn finish_capture(&mut self) {
        if self.mode == 2 {
            self.mode = 1;
        }
    }
    pub fn push(&mut self, packet: [u8; 20], now: u32) -> bool {
        if self.mode(now) == 0 {
            return false;
        }
        if self.len == CAPACITY {
            self.overflow = true;
            return false;
        }
        self.packets[(self.head + self.len) % CAPACITY] = packet;
        self.len += 1;
        true
    }
    pub fn command(&mut self, bytes: &[u8], now: u32) -> Option<[u8; 64]> {
        if bytes.len() != 64 || bytes[..3] != *b"RT\x02" || bytes[3] > 3 || bytes[5..].iter().any(|b| *b != 0) {
            return None;
        }
        self.mode(now);
        self.last_host = now;
        match bytes[3] {
            0 => {
                self.stop();
                self.mode = 1;
            }
            1 => {
                if self.mode == 0 {
                    self.mode = 1;
                }
            }
            2 => {
                if self.mode != 2 {
                    self.head = 0;
                    self.len = 0;
                    self.overflow = false;
                    self.mode = 2;
                }
            }
            3 => self.stop(),
            _ => unreachable!(),
        }
        let mut reply = [0u8; 64];
        reply[..4].copy_from_slice(&[0xd2, bytes[4], 3 | if self.overflow { 0x80 } else { 0 }, 0]);
        self.overflow = false;
        if bytes[3] == 1 {
            let count = self.len.min(3);
            reply[3] = count as u8;
            for i in 0..count {
                reply[4 + i * 20..24 + i * 20].copy_from_slice(&self.packets[self.head]);
                self.head = (self.head + 1) % CAPACITY;
                self.len -= 1;
            }
        }
        Some(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(op: u8, tag: u8) -> [u8; 64] {
        let mut b = [0u8; 64];
        b[..5].copy_from_slice(&[82, 84, 2, op, tag]);
        b
    }
    #[test]
    fn drains_three_complete_samples_and_detects_only_real_overflow() {
        let mut b = Buffer::new();
        assert!(!b.push([1; 20], 0));
        assert_eq!(b.command(&request(0, 7), 0).unwrap()[..4], [0xd2, 7, 3, 0]);
        for i in 0..128 {
            assert!(b.push([i as u8; 20], 1));
        }
        assert!(!b.push([255; 20], 1));
        let first = b.command(&request(1, 8), 2).unwrap();
        assert_eq!(&first[..4], &[0xd2, 8, 0x83, 3]);
        assert_eq!(&first[4..24], &[0; 20]);
        assert_eq!(&first[24..44], &[1; 20]);
        assert_eq!(&first[44..64], &[2; 20]);
        for expected in (3..128).step_by(3) {
            let reply = b.command(&request(1, 9), 3).unwrap();
            assert_eq!(reply[2], 3);
            assert_eq!(reply[4], expected as u8);
        }
        assert_eq!(b.command(&request(1, 10), 4).unwrap()[3], 0);
        for i in 0..3 {
            assert!(b.push([i; 20], 5));
        }
        assert_eq!(b.command(&request(1, 11), 6).unwrap()[3], 3);
    }
    #[test]
    fn polling_does_not_cancel_capture_and_disconnect_never_restarts_it() {
        let mut b = Buffer::new();
        b.command(&request(2, 1), 0);
        assert_eq!(b.mode(1), 2);
        b.command(&request(1, 2), 10);
        assert_eq!(b.mode(11), 2);
        b.finish_capture();
        assert_eq!(b.mode(12), 1);
        b.command(&request(3, 3), 13);
        b.finish_capture();
        assert_eq!(b.mode(14), 0);
        b.command(&request(0, 4), u32::MAX - 500);
        assert_eq!(b.mode(500), 1);
        assert_eq!(b.mode(2000), 0);
        b.command(&request(1, 5), 2001);
        assert_eq!(b.mode(2002), 1);
        let mut bad = request(2, 6);
        bad[63] = 1;
        assert!(b.command(&bad, 2003).is_none());
        assert_eq!(b.mode(2004), 1);
        assert!(b.command(&[0; 4], 2005).is_none());
    }
    #[test]
    fn foreground_polling_keeps_all_250hz_samples_without_log_truncation() {
        let mut b = Buffer::new();
        b.command(&request(0, 1), 0);
        let mut received = 0usize;
        for ms in 1..=10000u32 {
            if ms % 4 == 0 {
                assert!(b.push([(ms / 4 % 256) as u8; 20], ms));
            }
            if ms % 8 == 0 {
                let reply = b.command(&request(1, 2), ms).unwrap();
                assert_eq!(reply[2], 3);
                for i in 0..usize::from(reply[3]) {
                    received += 1;
                    assert_eq!(&reply[4 + i * 20..24 + i * 20], &[(received % 256) as u8; 20]);
                }
            }
        }
        assert_eq!(received, 2500);
    }
}
