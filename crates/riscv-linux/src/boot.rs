use core::{fmt, ops::Range};

use crate::{DRAM_BASE, DRAM_SIZE, cpio, fdt};

/// Physical entry address of the board's RV64 Linux Image.
pub const KERNEL_ADDRESS: u64 = 0x8020_0000;
/// Physical start of the board's uncompressed initramfs.
pub const INITRAMFS_ADDRESS: u64 = 0x8400_0000;
/// Physical start of the firmware's in-place device tree.
pub const FDT_ADDRESS: u64 = 0x8780_0000;
/// Maximum byte length of an explicitly supplied initramfs, including padding.
pub const MAX_INITRAMFS_BYTES: usize = 16 * 1024 * 1024;
const DRAM_END: u64 = DRAM_BASE + DRAM_SIZE as u64;

/// Board images. The host authenticates the locked images and the OpenSBI
/// ELF-derived resident end against its trust source before loading.
pub struct BootImages<'a> {
    pub firmware: &'a [u8],
    pub firmware_end: u64,
    pub kernel: &'a [u8],
    pub initramfs: &'a [u8],
    pub fdt: &'a [u8],
}

impl BootImages<'_> {
    /// RAM after the device tree reserved for in-place firmware updates.
    pub const FDT_GROWTH: u64 = 0x1_0000;
}

/// Failure to validate the board images or allocate RAM for a cold boot.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BootError {
    Allocation,
    Format(&'static str),
    InitramfsTooLarge,
    Overlap(&'static str, &'static str),
    Range(&'static str),
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation => write!(f, "could not allocate {DRAM_SIZE} bytes of board DRAM"),
            Self::Format(image) => write!(f, "unsupported or malformed {image}"),
            Self::InitramfsTooLarge => {
                write!(f, "custom initramfs exceeds {MAX_INITRAMFS_BYTES} bytes")
            }
            Self::Overlap(first, second) => write!(f, "{first} and {second} load ranges overlap"),
            Self::Range(image) => write!(f, "{image} load range is outside board DRAM"),
        }
    }
}

impl std::error::Error for BootError {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootProfile {
    Standard,
    SystemDisk,
}

pub(crate) struct Validated<'a> {
    images: BootImages<'a>,
    initrd_end_patch: Option<(usize, [u8; 8])>,
    pub(crate) profile: BootProfile,
}

impl<'a> Validated<'a> {
    pub(crate) fn new(
        images: BootImages<'a>,
        locked_initramfs_len: Option<usize>,
        profile: BootProfile,
    ) -> Result<Self, BootError> {
        const FW: &str = "OpenSBI";
        const KERNEL: &str = "Linux Image";
        const INITRD: &str = "initramfs";
        const DTB: &str = "DTB";

        if images.firmware.is_empty() || images.firmware.get(..4) == Some(b"\x7fELF") {
            return Err(BootError::Format(FW));
        }
        let fw_bytes = span(FW, DRAM_BASE, images.firmware.len() as u64)?;
        if images.firmware_end < fw_bytes.end || images.firmware_end > DRAM_END {
            return Err(BootError::Range(FW));
        }
        let kernel_bytes = span(KERNEL, KERNEL_ADDRESS, kernel_extent(images.kernel)?)?;
        if locked_initramfs_len.is_some() {
            if images.initramfs.len() > MAX_INITRAMFS_BYTES {
                return Err(BootError::InitramfsTooLarge);
            }
            cpio::check(images.initramfs).ok_or(BootError::Format("custom initramfs"))?;
        } else if !cpio::has_newc_magic(images.initramfs) {
            return Err(BootError::Format(INITRD));
        }
        let initrd = span(INITRD, INITRAMFS_ADDRESS, images.initramfs.len() as u64)?;
        let tree_bytes = (images.fdt.len() as u64)
            .checked_add(BootImages::FDT_GROWTH)
            .ok_or(BootError::Range(DTB))?;
        let tree = span(DTB, FDT_ADDRESS, tree_bytes)?;
        let regions = [
            (FW, DRAM_BASE..images.firmware_end),
            (KERNEL, kernel_bytes),
            (INITRD, initrd.clone()),
            (DTB, tree),
        ];
        for adjacent in regions.windows(2) {
            let [(left, first), (right, second)] = adjacent else {
                unreachable!("adjacent regions")
            };
            if first.end > second.start {
                return Err(BootError::Overlap(left, right));
            }
        }
        let expected_end = match locked_initramfs_len {
            Some(len) => {
                let locked = span(INITRD, INITRAMFS_ADDRESS, len as u64)?;
                if locked.end > FDT_ADDRESS {
                    return Err(BootError::Overlap(INITRD, DTB));
                }
                locked.end
            }
            None => initrd.end,
        };
        let bootargs: &[u8] = match profile {
            BootProfile::Standard => b"console=ttyS0 rdinit=/init\0",
            BootProfile::SystemDisk => b"console=ttyS0 rdinit=/init panic=-1\0",
        };
        let offset = fdt::check(images.fdt, initrd.start, expected_end, bootargs)
            .ok_or(BootError::Format(DTB))?;
        // The validated property has exactly eight bytes. Changing only those
        // bytes preserves every FDT check except the expected end value, which
        // now describes the already validated replacement range.
        let initrd_end_patch =
            (expected_end != initrd.end).then_some((offset, initrd.end.to_be_bytes()));
        Ok(Self {
            images,
            initrd_end_patch,
            profile,
        })
    }

    pub(crate) fn install(self, ram: &mut [u8]) {
        let images = self.images;
        for (address, bytes) in [
            (DRAM_BASE, images.firmware),
            (KERNEL_ADDRESS, images.kernel),
            (INITRAMFS_ADDRESS, images.initramfs),
            (FDT_ADDRESS, images.fdt),
        ] {
            let start = (address - DRAM_BASE) as usize;
            ram[start..start + bytes.len()].copy_from_slice(bytes);
        }
        if let Some((offset, value)) = self.initrd_end_patch {
            let start = (FDT_ADDRESS - DRAM_BASE) as usize + offset;
            ram[start..start + value.len()].copy_from_slice(&value);
        }
    }
}

fn span(name: &'static str, base: u64, bytes: u64) -> Result<Range<u64>, BootError> {
    let end = base.checked_add(bytes).ok_or(BootError::Range(name))?;
    if end > DRAM_END {
        return Err(BootError::Range(name));
    }
    Ok(base..end)
}

fn kernel_extent(image: &[u8]) -> Result<u64, BootError> {
    let header = image.get(..64).ok_or(BootError::Format("Linux Image"))?;
    let word = |offset| u64::from_le_bytes(header[offset..offset + 8].try_into().unwrap());
    let size = word(16);
    let version = u32::from_le_bytes(header[32..36].try_into().unwrap());
    if &header[56..60] != b"RSC\x05"
        || version != 2
        || word(8) != KERNEL_ADDRESS - DRAM_BASE
        || size == 0
        || word(24) != 0
    {
        return Err(BootError::Format("Linux Image"));
    }
    Ok(size.max(image.len() as u64))
}
