//! A flat, byte-addressable, little-endian RAM mapped at a base address.

use std::fmt;
use std::ops::Range;

/// An access that falls (even partially) outside the mapped region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessFault {
    pub addr: u32,
    pub size: u32,
}

impl fmt::Display for AccessFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-byte access at {:#010x} is outside memory",
            self.size, self.addr
        )
    }
}

impl std::error::Error for AccessFault {}

pub struct Memory {
    base: u32,
    bytes: Vec<u8>,
}

impl Memory {
    pub fn new(base: u32, size: usize) -> Self {
        Self {
            base,
            bytes: vec![0; size],
        }
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    /// One past the last mapped address, as a u64 so a region ending at 4 GiB still fits.
    pub fn end(&self) -> u64 {
        u64::from(self.base) + self.bytes.len() as u64
    }

    /// Translate `[addr, addr + size)` into an index range, or fault.
    ///
    /// Every step is checked: the C++ simulator tested `addr + size > MEMORY_SIZE`
    /// in u32 arithmetic, which wraps for addresses near 0xFFFFFFFF and lets the
    /// access through.
    fn range(&self, addr: u32, size: u32) -> Result<Range<usize>, AccessFault> {
        let fault = AccessFault { addr, size };
        let start = addr.checked_sub(self.base).ok_or(fault)? as usize;
        let end = start.checked_add(size as usize).ok_or(fault)?;
        if end > self.bytes.len() {
            return Err(fault);
        }
        Ok(start..end)
    }

    /// Read 1, 2 or 4 bytes, zero-extended to 32 bits.
    pub fn read(&self, addr: u32, size: u32) -> Result<u32, AccessFault> {
        let range = self.range(addr, size)?;
        let mut buf = [0u8; 4];
        buf[..range.len()].copy_from_slice(&self.bytes[range]);
        Ok(u32::from_le_bytes(buf))
    }

    /// Write the low 1, 2 or 4 bytes of `value`.
    pub fn write(&mut self, addr: u32, size: u32, value: u32) -> Result<(), AccessFault> {
        let range = self.range(addr, size)?;
        let n = range.len();
        self.bytes[range].copy_from_slice(&value.to_le_bytes()[..n]);
        Ok(())
    }

    /// Copy a block (e.g. an ELF segment) into memory.
    pub fn load(&mut self, addr: u32, data: &[u8]) -> Result<(), AccessFault> {
        let size = u32::try_from(data.len()).map_err(|_| AccessFault {
            addr,
            size: u32::MAX,
        })?;
        let range = self.range(addr, size)?;
        self.bytes[range].copy_from_slice(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_little_endian() {
        let mut mem = Memory::new(0x8000_0000, 16);
        mem.write(0x8000_0000, 4, 0xdead_beef).unwrap();
        assert_eq!(mem.read(0x8000_0000, 1).unwrap(), 0xef);
        assert_eq!(mem.read(0x8000_0002, 2).unwrap(), 0xdead);
        assert_eq!(mem.read(0x8000_0000, 4).unwrap(), 0xdead_beef);
    }

    #[test]
    fn faults_instead_of_wrapping() {
        let mem = Memory::new(0, 1024);
        // In wrapping u32 arithmetic 0xFFFF_FFFE + 4 == 2, which is "in bounds".
        assert!(mem.read(0xffff_fffe, 4).is_err());
        assert!(mem.read(1022, 4).is_err());
        assert!(mem.read(1020, 4).is_ok());
    }

    #[test]
    fn faults_below_base() {
        let mem = Memory::new(0x8000_0000, 1024);
        assert_eq!(
            mem.read(0x7fff_fffc, 4),
            Err(AccessFault {
                addr: 0x7fff_fffc,
                size: 4
            })
        );
    }
}
