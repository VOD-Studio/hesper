use core::{fmt, ops::Range};

use crate::{board::read_le, dram_range};

const SECTOR: usize = 512;
const QUEUE_MAX: u16 = 128;
const VERSION_1: u32 = 1; // Feature bit 32, page 1.
const ACKNOWLEDGE: u32 = 1;
const DRIVER: u32 = 2;
const DRIVER_OK: u32 = 4;
const FEATURES_OK: u32 = 8;
const NEEDS_RESET: u32 = 64;
const FAILED: u32 = 128;
const USED_IRQ: u32 = 1;
const CONFIG_IRQ: u32 = 2;
const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;
const NO_INTERRUPT: u16 = 1;
const ID: &[u8; 20] = b"hesper-data\0\0\0\0\0\0\0\0\0";

/// Owned raw sector bytes. The machine does not interpret their filesystem.
pub struct DataDisk {
    bytes: Vec<u8>,
    dirty: bool,
}

impl DataDisk {
    pub const MAX_BYTES: usize = 16 * 1024 * 1024;

    pub const fn valid_size(bytes: usize) -> bool {
        bytes > 0 && bytes <= Self::MAX_BYTES && bytes.is_multiple_of(SECTOR)
    }

    pub fn new(bytes: Vec<u8>) -> Result<Self, DiskSizeError> {
        if !Self::valid_size(bytes.len()) {
            return Err(DiskSizeError(bytes.len()));
        }
        Ok(Self {
            bytes,
            dirty: false,
        })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn dirty(&self) -> bool {
        self.dirty
    }

    pub(crate) fn mark_exported(&mut self) {
        self.dirty = false;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskSizeError(pub usize);

impl fmt::Display for DiskSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "disk length {} must be a positive multiple of {SECTOR} up to {} bytes",
            self.0,
            DataDisk::MAX_BYTES
        )
    }
}

impl std::error::Error for DiskSizeError {}

/// A transport fault, latched until a device reset. Request-level IOERR and
/// UNSUPP completions do not set a transport fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskFault {
    QueueLayout,
    AvailableCount,
    DescriptorChain,
    DescriptorDma,
}

impl fmt::Display for DiskFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::QueueLayout => "invalid virtio split queue size, alignment or DRAM range",
            Self::AvailableCount => "virtio available index advances beyond the split queue",
            Self::DescriptorChain => "invalid virtio block descriptor chain",
            Self::DescriptorDma => "virtio block descriptor DMA is outside data DRAM",
        })?;
        f.write_str("; device reset required")
    }
}

impl std::error::Error for DiskFault {}

pub(crate) struct VirtioBlk {
    pub(crate) disk: DataDisk,
    transport: Transport,
    // One scratch table per device. Reused for each chain; only occupied entries
    // are touched. All descriptors are checked before the transfer starts.
    segments: [Segment; QUEUE_MAX as usize],
}

#[derive(Default)]
struct Transport {
    device_features_sel: u32,
    driver_features_sel: u32,
    driver_features: [u32; 2],
    queue_sel: u32,
    queue_num: u32,
    addresses: [u64; 3],
    ring: Option<Ring>,
    status: u32,
    interrupt: u32,
    fault: Option<DiskFault>,
}

impl VirtioBlk {
    pub(crate) fn new(disk: DataDisk) -> Self {
        Self {
            disk,
            transport: Transport::default(),
            segments: [Segment { start: 0, len: 0 }; QUEUE_MAX as usize],
        }
    }

    pub(crate) fn reset(&mut self) {
        self.transport = Transport::default();
    }

    pub(crate) fn fault(&self) -> Option<DiskFault> {
        self.transport.fault
    }

    pub(crate) fn interrupt(&self) -> bool {
        self.transport.interrupt != 0
    }

    pub(crate) fn read(&self, offset: u64, width: usize) -> Option<u64> {
        if offset >= 0x100 {
            let capacity = (self.disk.bytes.len() as u64 / SECTOR as u64).to_le_bytes();
            return match width {
                1 | 2 | 4 if offset.is_multiple_of(width as u64) => Some(
                    capacity
                        .get((offset - 0x100) as usize..(offset - 0x100) as usize + width)
                        .map_or(0, read_le),
                ),
                _ => None,
            };
        }
        if width != 4 || !offset.is_multiple_of(4) {
            return None;
        }
        let t = &self.transport;
        Some(u64::from(match offset {
            0x000 => 0x7472_6976, // "virt"
            0x004 => 2,           // Modern MMIO transport.
            0x008 => 2,           // Block device.
            0x00c => u32::from_le_bytes(*b"HSPR"),
            0x010 if t.device_features_sel == 1 => VERSION_1,
            0x034 if t.queue_sel == 0 => u32::from(QUEUE_MAX),
            0x044 if t.queue_sel == 0 => u32::from(t.ring.is_some()),
            0x060 => t.interrupt,
            0x070 => t.status,
            0x0fc => 0, // Capacity never changes; no config generation changes.
            _ => 0,
        }))
    }

    /// Returns whether a completion wrote guest DRAM.
    pub(crate) fn write(
        &mut self,
        offset: u64,
        width: usize,
        value: u32,
        ram: &mut [u8],
    ) -> Option<bool> {
        if offset >= 0x100 {
            // The device-specific configuration is read-only.
            return matches!(width, 1 | 2 | 4).then_some(false);
        }
        if width != 4 || !offset.is_multiple_of(4) {
            return None;
        }
        let t = &mut self.transport;
        match offset {
            0x014 => t.device_features_sel = value,
            0x024 => t.driver_features_sel = value,
            0x020 if t.status & FEATURES_OK == 0 => {
                if let Some(page) = t.driver_features.get_mut(t.driver_features_sel as usize) {
                    *page = value;
                }
            }
            0x030 => t.queue_sel = value,
            0x038 if t.queue_sel == 0 && t.ring.is_none() => t.queue_num = value,
            0x044 if t.queue_sel == 0 && t.fault.is_none() => match value {
                0 => t.ring = None,
                1 if t.ring.is_none() && t.status & FEATURES_OK != 0 => {
                    t.ring = Ring::new(t.queue_num, t.addresses);
                    if t.ring.is_none() {
                        t.fail(DiskFault::QueueLayout);
                    }
                }
                _ => {}
            },
            0x050 if value == 0 && t.status & (DRIVER_OK | FAILED | NEEDS_RESET) == DRIVER_OK => {
                let Some(ring) = t.ring.as_mut() else {
                    return Some(false);
                };
                let before = ring.used_index;
                let result = ring.service(&mut self.disk, &mut self.segments, ram);
                let completed = before != ring.used_index;
                if completed && read16(ram, ring.avail.start) & NO_INTERRUPT == 0 {
                    t.interrupt |= USED_IRQ;
                }
                if let Err(fault) = result {
                    t.fail(fault);
                }
                return Some(completed);
            }
            0x064 => t.interrupt &= !(value & (USED_IRQ | CONFIG_IRQ)),
            0x070 if value == 0 => self.reset(),
            0x070 => t.set_status(value),
            0x080 | 0x084 | 0x090 | 0x094 | 0x0a0 | 0x0a4
                if t.queue_sel == 0 && t.ring.is_none() =>
            {
                let address = &mut t.addresses[((offset - 0x080) / 0x10) as usize];
                let shift = ((offset & 4) * 8) as u32;
                *address =
                    (*address & !(u64::from(u32::MAX) << shift)) | (u64::from(value) << shift);
            }
            _ => {}
        }
        Some(false)
    }
}

impl Transport {
    fn set_status(&mut self, value: u32) {
        self.status |= value & (ACKNOWLEDGE | DRIVER | FAILED);
        let has_driver = self.status & (ACKNOWLEDGE | DRIVER) == (ACKNOWLEDGE | DRIVER);
        if has_driver && value & FEATURES_OK != 0 && self.driver_features == [0, VERSION_1] {
            self.status |= FEATURES_OK;
        }
        if self.status & FEATURES_OK != 0 && value & DRIVER_OK != 0 {
            self.status |= DRIVER_OK;
        }
    }

    fn fail(&mut self, fault: DiskFault) {
        self.fault = Some(fault);
        self.status |= NEEDS_RESET;
        if self.status & DRIVER_OK != 0 {
            self.interrupt |= CONFIG_IRQ;
        }
    }
}

struct Ring {
    num: u16,
    desc: Range<usize>,
    avail: Range<usize>,
    used: Range<usize>,
    avail_index: u16,
    used_index: u16,
}

impl Ring {
    fn new(num: u32, [desc, avail, used]: [u64; 3]) -> Option<Self> {
        if !num.is_power_of_two()
            || num > u32::from(QUEUE_MAX)
            || !desc.is_multiple_of(16)
            || !avail.is_multiple_of(2)
            || !used.is_multiple_of(4)
        {
            return None;
        }
        // No event index fields: that feature is not advertised.
        let n = num as usize;
        let desc = dram_range(desc, 16 * n)?;
        let avail = dram_range(avail, 4 + 2 * n)?;
        let used = dram_range(used, 4 + 8 * n)?;
        if overlaps(&desc, &avail) || overlaps(&desc, &used) || overlaps(&avail, &used) {
            return None;
        }
        Some(Self {
            num: num as u16,
            desc,
            avail,
            used,
            avail_index: 0,
            used_index: 0,
        })
    }

    fn service(
        &mut self,
        disk: &mut DataDisk,
        segments: &mut [Segment; QUEUE_MAX as usize],
        ram: &mut [u8],
    ) -> Result<(), DiskFault> {
        let pending = read16(ram, self.avail.start + 2).wrapping_sub(self.avail_index);
        if pending > self.num {
            return Err(DiskFault::AvailableCount);
        }
        // Each outstanding chain owns its descriptors. One bitmap bounds the
        // whole notification and rejects cycles or shared descriptors.
        let mut visited = 0u128;
        for _ in 0..pending {
            let slot = usize::from(self.avail_index & (self.num - 1));
            let head = read16(ram, self.avail.start + 4 + 2 * slot);
            let request = self.request(head, &mut visited, ram, segments)?;
            let used = request.complete(disk, ram);
            let slot = usize::from(self.used_index & (self.num - 1));
            let entry = self.used.start + 4 + 8 * slot;
            ram[entry..entry + 4].copy_from_slice(&u32::from(head).to_le_bytes());
            ram[entry + 4..entry + 8].copy_from_slice(&used.to_le_bytes());
            self.avail_index = self.avail_index.wrapping_add(1);
            self.used_index = self.used_index.wrapping_add(1);
            ram[self.used.start + 2..self.used.start + 4]
                .copy_from_slice(&self.used_index.to_le_bytes());
        }
        Ok(())
    }

    fn request<'a>(
        &self,
        mut index: u16,
        visited: &mut u128,
        ram: &[u8],
        segments: &'a mut [Segment],
    ) -> Result<Request<'a>, DiskFault> {
        let mut header = [0u8; 16];
        let mut header_len = 0;
        let mut seen_write = false;
        let (mut count, mut out_count) = (0, 0);
        let (mut out_bytes, mut in_bytes) = (0usize, 0usize);
        loop {
            if index >= self.num || *visited & (1 << index) != 0 {
                return Err(DiskFault::DescriptorChain);
            }
            *visited |= 1 << index;
            let offset = self.desc.start + usize::from(index) * 16;
            let address = read_le(&ram[offset..offset + 8]);
            let len = read_le(&ram[offset + 8..offset + 12]) as usize;
            let flags = read16(ram, offset + 12);
            if flags & !(DESC_NEXT | DESC_WRITE) != 0 {
                return Err(DiskFault::DescriptorChain);
            }
            if len != 0 {
                let range = dram_range(address, len).ok_or(DiskFault::DescriptorDma)?;
                if overlaps(&range, &self.desc)
                    || overlaps(&range, &self.avail)
                    || overlaps(&range, &self.used)
                {
                    return Err(DiskFault::DescriptorDma);
                }
                let (start, len) = if flags & DESC_WRITE != 0 {
                    if header_len != header.len() {
                        return Err(DiskFault::DescriptorChain);
                    }
                    seen_write = true;
                    in_bytes = in_bytes
                        .checked_add(len)
                        .ok_or(DiskFault::DescriptorChain)?;
                    (range.start, len)
                } else {
                    if seen_write {
                        return Err(DiskFault::DescriptorChain);
                    }
                    let take = len.min(header.len() - header_len);
                    header[header_len..header_len + take]
                        .copy_from_slice(&ram[range.start..range.start + take]);
                    header_len += take;
                    out_bytes = out_bytes
                        .checked_add(len - take)
                        .ok_or(DiskFault::DescriptorChain)?;
                    (range.start + take, len - take)
                };
                if len != 0 {
                    segments[count] = Segment { start, len };
                    count += 1;
                    if !seen_write {
                        out_count = count;
                    }
                }
            }
            if flags & DESC_NEXT == 0 {
                break;
            }
            index = read16(ram, offset + 14);
        }
        if !seen_write {
            return Err(DiskFault::DescriptorChain);
        }
        // The status byte follows the data, not necessarily in its own
        // descriptor. Header bytes need not occupy their own descriptor either.
        let last = &mut segments[count - 1];
        last.len -= 1;
        let status = last.start + last.len;
        Ok(Request {
            kind: read_le(&header[..4]) as u32,
            sector: read_le(&header[8..]),
            status,
            segments: &segments[..count],
            out_count,
            out_bytes,
            in_bytes: in_bytes - 1,
        })
    }
}

#[derive(Clone, Copy)]
struct Segment {
    start: usize,
    len: usize,
}

struct Request<'a> {
    kind: u32,
    sector: u64,
    status: usize,
    segments: &'a [Segment],
    out_count: usize,
    out_bytes: usize,
    in_bytes: usize,
}

impl Request<'_> {
    fn complete(&self, disk: &mut DataDisk, ram: &mut [u8]) -> u32 {
        const OK: u8 = 0;
        const IOERR: u8 = 1;
        const UNSUPP: u8 = 2;
        let [input, output] = [
            &self.segments[..self.out_count],
            &self.segments[self.out_count..],
        ];
        let (status, written) = match self.kind {
            0 if self.out_bytes == 0 => {
                if let Some(span) = disk_span(self.sector, self.in_bytes, disk.bytes.len()) {
                    scatter(ram, output, &disk.bytes[span]);
                    (OK, self.in_bytes)
                } else {
                    (IOERR, 0)
                }
            }
            1 if self.in_bytes == 0 => {
                if let Some(span) = disk_span(self.sector, self.out_bytes, disk.bytes.len()) {
                    let mut dest = &mut disk.bytes[span];
                    for &Segment { start, len } in input {
                        let (target, rest) = dest.split_at_mut(len);
                        target.copy_from_slice(&ram[start..start + len]);
                        dest = rest;
                    }
                    disk.dirty |= self.out_bytes != 0;
                    (OK, 0)
                } else {
                    (IOERR, 0)
                }
            }
            8 if self.out_bytes == 0 && self.in_bytes >= ID.len() => {
                scatter(ram, output, ID);
                (OK, ID.len())
            }
            0 | 1 | 8 => (IOERR, 0),
            _ => (UNSUPP, 0),
        };
        ram[self.status] = status;
        (written + 1) as u32
    }
}

fn scatter(ram: &mut [u8], output: &[Segment], mut bytes: &[u8]) {
    for &Segment { start, len } in output {
        let len = len.min(bytes.len());
        ram[start..start + len].copy_from_slice(&bytes[..len]);
        bytes = &bytes[len..];
        if bytes.is_empty() {
            break;
        }
    }
}

fn disk_span(sector: u64, bytes: usize, capacity: usize) -> Option<Range<usize>> {
    let start = usize::try_from(sector).ok()?.checked_mul(SECTOR)?;
    let end = start.checked_add(bytes)?;
    (bytes.is_multiple_of(SECTOR) && end <= capacity).then_some(start..end)
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn read16(ram: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([ram[offset], ram[offset + 1]])
}
