//! Minimal ELF32 loader: just enough to place PT_LOAD segments and find the entry point.
//!
//! Reference: System V gABI, "ELF Header" and "Program Header" chapters.

use std::fmt;

const EM_RISCV: u16 = 243;
const PT_LOAD: u32 = 1;
const EHDR_SIZE: usize = 52;
const PHDR_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    BadMagic,
    Not32Bit,
    NotLittleEndian,
    NotRiscV(u16),
    /// A header or segment points past the end of the file.
    Truncated {
        offset: usize,
    },
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElfError::BadMagic => f.write_str("not an ELF file"),
            ElfError::Not32Bit => f.write_str("not a 32-bit ELF (expected ELFCLASS32)"),
            ElfError::NotLittleEndian => f.write_str("not a little-endian ELF"),
            ElfError::NotRiscV(m) => write!(f, "ELF machine is {m}, expected RISC-V ({EM_RISCV})"),
            ElfError::Truncated { offset } => write!(f, "ELF truncated at byte offset {offset}"),
        }
    }
}

impl std::error::Error for ElfError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Physical load address (p_paddr), as bare-metal loaders such as Spike use.
    pub addr: u32,
    /// File-backed bytes; memory past `data.len()` up to `mem_size` is zero (e.g. .bss).
    pub data: Vec<u8>,
    pub mem_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfImage {
    pub entry: u32,
    pub segments: Vec<Segment>,
}

pub fn is_elf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF")
}

fn slice(bytes: &[u8], offset: usize, len: usize) -> Result<&[u8], ElfError> {
    offset
        .checked_add(len)
        .and_then(|end| bytes.get(offset..end))
        .ok_or(ElfError::Truncated { offset })
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    let b = slice(bytes, offset, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    let b = slice(bytes, offset, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub fn parse(bytes: &[u8]) -> Result<ElfImage, ElfError> {
    if !is_elf(bytes) {
        return Err(ElfError::BadMagic);
    }
    slice(bytes, 0, EHDR_SIZE)?;
    if bytes[4] != 1 {
        return Err(ElfError::Not32Bit);
    }
    if bytes[5] != 1 {
        return Err(ElfError::NotLittleEndian);
    }
    let machine = u16_at(bytes, 18)?;
    if machine != EM_RISCV {
        return Err(ElfError::NotRiscV(machine));
    }

    let entry = u32_at(bytes, 24)?;
    let phoff = u32_at(bytes, 28)? as usize;
    let phentsize = usize::from(u16_at(bytes, 42)?).max(PHDR_SIZE);
    let phnum = usize::from(u16_at(bytes, 44)?);

    let mut segments = Vec::new();
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if u32_at(bytes, ph)? != PT_LOAD {
            continue;
        }
        let offset = u32_at(bytes, ph + 4)? as usize;
        let addr = u32_at(bytes, ph + 12)?;
        let file_size = u32_at(bytes, ph + 16)? as usize;
        let mem_size = u32_at(bytes, ph + 20)?;
        let data = slice(bytes, offset, file_size)?.to_vec();
        segments.push(Segment {
            addr,
            data,
            mem_size,
        });
    }

    Ok(ElfImage { entry, segments })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny ELF32 with one PT_LOAD segment holding `code` at `addr`.
    fn tiny_elf(addr: u32, code: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; EHDR_SIZE + PHDR_SIZE];
        f[..4].copy_from_slice(b"\x7fELF");
        f[4] = 1; // ELFCLASS32
        f[5] = 1; // ELFDATA2LSB
        f[18..20].copy_from_slice(&EM_RISCV.to_le_bytes());
        f[24..28].copy_from_slice(&addr.to_le_bytes()); // e_entry
        f[28..32].copy_from_slice(&(EHDR_SIZE as u32).to_le_bytes()); // e_phoff
        f[42..44].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
        f[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
        let ph = EHDR_SIZE;
        let data_off = (EHDR_SIZE + PHDR_SIZE) as u32;
        let len = code.len() as u32;
        f[ph..ph + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        f[ph + 4..ph + 8].copy_from_slice(&data_off.to_le_bytes());
        f[ph + 12..ph + 16].copy_from_slice(&addr.to_le_bytes());
        f[ph + 16..ph + 20].copy_from_slice(&len.to_le_bytes());
        f[ph + 20..ph + 24].copy_from_slice(&(len + 16).to_le_bytes());
        f.extend_from_slice(code);
        f
    }

    #[test]
    fn parses_a_single_segment() {
        let image = parse(&tiny_elf(0x8000_0000, &[0x73, 0, 0, 0])).unwrap();
        assert_eq!(image.entry, 0x8000_0000);
        assert_eq!(
            image.segments,
            vec![Segment {
                addr: 0x8000_0000,
                data: vec![0x73, 0, 0, 0],
                mem_size: 20
            }]
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse(b"hello"), Err(ElfError::BadMagic));
        let mut elf = tiny_elf(0, &[]);
        elf[18] = 62; // EM_X86_64
        assert_eq!(parse(&elf), Err(ElfError::NotRiscV(62)));
        let elf = tiny_elf(0, &[1, 2, 3, 4]);
        assert!(matches!(
            parse(&elf[..elf.len() - 2]),
            Err(ElfError::Truncated { .. })
        ));
    }
}
