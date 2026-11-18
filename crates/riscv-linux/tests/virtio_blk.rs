use hesper_riscv_linux::{DRAM_BASE, DRAM_SIZE, DataDisk, DiskFault, Machine};

const MMIO: u32 = 0x1000_1000;
const PLIC: u32 = 0x0c00_0000;
const UART: u32 = 0x1000_0000;
const CLAIM: u32 = PLIC + 0x20_1004;
const DESC: u64 = DRAM_BASE + 0x10_0000;
const AVAIL: u64 = DESC + 0x100;
const USED: u64 = DESC + 0x200;
const HEADER: u64 = DESC + 0x400;
const DATA: u64 = DESC + 0x800;
const OUTPUT: u64 = DESC + 0x1000;
const STATUS: u64 = DESC + 0x2000;
const NEXT: u16 = 1;
const WRITE: u16 = 2;

struct Probe {
    machine: Machine,
    pc: u64,
    slice: u32,
}

impl Probe {
    fn new(slice: u32) -> Self {
        Self {
            machine: Machine::try_with_disk(DataDisk::new(vec![0x55; 1024]).unwrap()).unwrap(),
            pc: DRAM_BASE,
            slice,
        }
    }

    fn execute<const N: usize>(&mut self, instructions: [u32; N]) {
        self.put(self.pc, instructions.map(u32::to_le_bytes).as_flattened());
        self.pc += N as u64 * 4;
        let mut remaining = N as u32;
        while remaining != 0 {
            let fuel = remaining.min(self.slice);
            assert_eq!(self.machine.run(fuel).retired, fuel);
            remaining -= fuel;
        }
    }

    fn put(&mut self, addr: u64, bytes: &[u8]) {
        self.machine.load(addr, bytes).unwrap();
    }

    fn write_mmio(&mut self, addr: u32, value: u32, width: u32) {
        let [a, b] = constant(addr, 1);
        let [c, d] = constant(value, 2);
        self.execute([a, b, c, d, 2 << 20 | 1 << 15 | width << 12 | 0x23]);
    }

    fn write(&mut self, reg: u32, value: u32) {
        self.write_mmio(MMIO + reg, value, 2);
    }

    fn read_mmio(&mut self, addr: u32) -> u32 {
        let [a, b] = constant(addr, 1);
        self.execute([a, b, 1 << 15 | 6 << 12 | 3 << 7 | 0x03]); // lwu x3,0(x1)
        self.machine.debug_state().x[3] as u32
    }

    fn read(&mut self, reg: u32) -> u32 {
        self.read_mmio(MMIO + reg)
    }

    fn negotiate(&mut self) {
        self.write(0x70, 0);
        self.write(0x70, 1);
        self.write(0x70, 3);
        self.write(0x24, 1);
        self.write(0x20, 1);
        self.write(0x70, 11);
        assert_eq!(self.read(0x70), 11);
    }

    fn configure(&mut self, used: u64) {
        self.write(0x38, 8);
        self.write(0x80, DESC as u32);
        self.write(0x90, AVAIL as u32);
        self.write(0xa0, used as u32);
        self.write(0x44, 1);
    }

    fn descriptor(&mut self, index: u16, addr: u64, len: u32, flags: u16, next: u16) {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&addr.to_le_bytes());
        bytes[8..12].copy_from_slice(&len.to_le_bytes());
        bytes[12..14].copy_from_slice(&flags.to_le_bytes());
        bytes[14..].copy_from_slice(&next.to_le_bytes());
        self.put(DESC + u64::from(index) * 16, &bytes);
    }

    fn header(&mut self, kind: u32, sector: u64) {
        self.put(HEADER, &kind.to_le_bytes());
        self.put(HEADER + 8, &sector.to_le_bytes());
    }

    fn available(&mut self, index: u16, heads: &[u16]) {
        for (slot, head) in heads.iter().enumerate() {
            self.put(AVAIL + 4 + slot as u64 * 2, &head.to_le_bytes());
        }
        self.put(AVAIL + 2, &index.to_le_bytes());
    }

    fn bytes(&self, addr: u64, size: usize) -> &[u8] {
        self.machine.inspect(addr, size).unwrap()
    }

    fn used_index(&self) -> u16 {
        u16::from_le_bytes(self.bytes(USED + 2, 2).try_into().unwrap())
    }

    fn completion(&self, slot: u64) -> (u32, u32) {
        let bytes = self.bytes(USED + 4 + slot * 8, 8);
        (
            u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            u32::from_le_bytes(bytes[4..].try_into().unwrap()),
        )
    }
}

fn constant(value: u32, rd: u32) -> [u32; 2] {
    let upper = value.wrapping_add(0x800) & 0xffff_f000;
    // The queue addresses (0x80...) are data written as words, not sign-extended MMIO bases.
    let lower = (value & 0xfff) << 20;
    [upper | rd << 7 | 0x37, lower | rd << 15 | rd << 7 | 0x13]
}

fn transfer(slice: u32) -> (Vec<u8>, u64, u64) {
    let mut p = Probe::new(slice);
    assert_eq!(p.read(0x00), 0x74726976);
    assert_eq!(p.read(0x04), 2);
    assert_eq!(p.read(0x08), 2);
    assert_eq!(p.read(0x100), 2);
    assert_eq!(p.read(0x10), 0);
    p.write(0x14, 1);
    assert_eq!(p.read(0x10), 1);
    p.write(0x30, 1);
    assert_eq!(p.read(0x34), 0);
    p.write(0x30, 0);
    assert_eq!(p.read(0x34), 128);

    // No FEATURES_OK for a requested feature this device did not offer.
    p.write(0x70, 3);
    p.write(0x20, 1);
    p.write(0x70, 11);
    assert_eq!(p.read(0x70), 3);
    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    assert_eq!(p.read(0x44), 1);
    p.write_mmio(PLIC + 4, 1, 2);
    p.write_mmio(PLIC + 0x28, 1, 2);
    p.write_mmio(PLIC + 0x2080, 1026, 2);
    p.write_mmio(UART + 1, 2, 0); // THRE, UART source 10.

    // First request writes the last sector. Header is split across two
    // descriptors and shares the second descriptor with the first data span.
    p.header(1, 1);
    p.put(HEADER + 16, &[0x79; 160]);
    p.put(DATA, &[0xc3; 352]);
    p.put(STATUS, &[0xff]);
    p.descriptor(0, HEADER, 8, NEXT, 1);
    p.descriptor(1, HEADER + 8, 168, NEXT, 2);
    p.descriptor(2, DATA, 352, NEXT, 3);
    p.descriptor(3, STATUS, 1, WRITE, 0);
    // Follow it with a read in the same notification; the status is coalesced
    // with the last writable span, not a dedicated descriptor.
    p.put(HEADER + 256 + 8, &1u64.to_le_bytes());
    p.put(OUTPUT, &[0xef; 514]);
    p.descriptor(4, HEADER + 256, 16, NEXT, 5);
    p.descriptor(5, OUTPUT, 307, NEXT | WRITE, 6);
    p.descriptor(6, OUTPUT + 307, 206, WRITE, 0);
    p.available(2, &[0, 4]);
    p.write(0x50, 0);
    assert_eq!(p.used_index(), 2);
    assert_eq!(p.completion(0), (0, 1));
    assert_eq!(p.completion(1), (4, 513));
    assert_eq!(p.bytes(STATUS, 1), &[0]);
    assert_eq!(p.bytes(OUTPUT + 512, 2), &[0, 0xef]);
    assert_eq!(p.bytes(OUTPUT, 160), &[0x79; 160]);
    assert_eq!(p.bytes(OUTPUT + 160, 352), &[0xc3; 352]);
    assert_eq!(p.read(0x60), 1);
    assert_eq!(p.read_mmio(CLAIM), 1); // Equal priorities: lower ID wins.
    assert_eq!(p.machine.diagnostics().mip & 512, 512); // UART unaffected.
    p.write_mmio(CLAIM, 1, 2); // Completing without ACK re-pends the block line.
    assert_eq!(p.read_mmio(CLAIM), 1);
    p.write(0x64, 1);
    p.write_mmio(CLAIM, 1, 2);
    assert_eq!(p.read_mmio(CLAIM), 10);
    p.write_mmio(UART + 1, 0, 0);
    p.write_mmio(CLAIM, 10, 2);
    assert_eq!(p.read_mmio(CLAIM), 0);
    assert_eq!(p.machine.diagnostics().mip & 512, 0);
    assert!(p.machine.disk_dirty());
    let disk = p.machine.data_disk().unwrap().to_vec();
    assert_eq!(disk[..512], [0x55; 512]);
    assert_eq!(&disk[512..], p.bytes(OUTPUT, 512));

    p.write(0x70, 0);
    assert_eq!(p.read(0x44), 0);
    assert_eq!(p.read(0x60), 0);
    p.machine.mark_disk_exported();
    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    p.header(8, 0);
    p.descriptor(0, HEADER, 16, NEXT, 1);
    p.descriptor(1, OUTPUT, 32, NEXT | WRITE, 2);
    p.descriptor(2, STATUS, 1, WRITE, 0);
    p.put(OUTPUT, &[0xef; 32]);
    p.available(1, &[0]);
    p.write(0x50, 0);
    assert_eq!(p.used_index(), 1);
    assert_eq!(p.completion(0), (0, 21));
    assert_eq!(p.bytes(OUTPUT, 11), b"hesper-data");
    assert_eq!(p.bytes(OUTPUT + 11, 9), &[0; 9]);
    assert_eq!(p.bytes(OUTPUT + 20, 12), &[0xef; 12]);
    assert_eq!(p.machine.data_disk().unwrap(), disk);
    assert!(!p.machine.disk_dirty());
    assert_eq!(p.machine.disk_fault(), None);
    let state = p.machine.debug_state();
    (disk, state.retired, state.time)
}

#[test]
fn transfers_split_requests_and_delivers_independent_interrupts_at_any_fuel_slice() {
    assert_eq!(transfer(1), transfer(64));
}

#[test]
fn fails_closed_on_bad_layout_and_malformed_requests_without_changing_sectors() {
    let mut p = Probe::new(32);
    p.negotiate();
    p.write(0x70, 15);
    p.write(0x50, 0); // Not ready.
    assert_eq!(p.used_index(), 0);
    assert_eq!(p.machine.disk_fault(), None);
    p.configure(DRAM_BASE + DRAM_SIZE as u64 - 4);
    assert_eq!(p.read(0x44), 0);
    assert_eq!(p.read(0x70) & 64, 64);
    assert_eq!(p.machine.disk_fault(), Some(DiskFault::QueueLayout));

    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    p.available(9, &[]);
    p.write(0x50, 0);
    assert_eq!(p.machine.disk_fault(), Some(DiskFault::AvailableCount));
    assert_eq!(p.used_index(), 0);

    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    p.header(1, 0);
    p.put(STATUS - 1, &[0x3b, 0xff, 0x4c]);
    p.put(DATA, &[0x9a; 1024]);
    p.descriptor(0, HEADER, 16, NEXT, 1);
    p.descriptor(1, DATA, 512, NEXT, 1); // Cycle after valid write data.
    p.available(1, &[0]);
    p.write(0x50, 0);
    assert_eq!(p.machine.disk_fault(), Some(DiskFault::DescriptorChain));
    assert_eq!(p.used_index(), 0);
    assert_eq!(p.bytes(STATUS - 1, 3), &[0x3b, 0xff, 0x4c]);
    assert_eq!(p.machine.data_disk().unwrap(), &[0x55; 1024]);

    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    p.descriptor(1, DATA, 512, NEXT, 2);
    p.descriptor(2, u64::from(UART), 1, WRITE, 0); // Not DRAM; never touches UART.
    p.write(0x50, 0);
    assert_eq!(p.machine.disk_fault(), Some(DiskFault::DescriptorDma));
    assert_eq!(p.machine.diagnostics().uart.output_queued, 0);
    assert_eq!(p.used_index(), 0);
    assert_eq!(p.machine.data_disk().unwrap(), &[0x55; 1024]);

    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    p.header(1, 1);
    p.descriptor(1, DATA, 1024, NEXT, 2); // Crosses disk end.
    p.descriptor(2, STATUS, 1, WRITE, 0);
    p.write(0x50, 0);
    assert_eq!(p.completion(0), (0, 1));
    assert_eq!(p.bytes(STATUS - 1, 3), &[0x3b, 1, 0x4c]);
    p.header(1, 0);
    p.descriptor(1, DATA, 512, NEXT | WRITE, 2); // OUT in the wrong direction.
    p.available(2, &[0, 0]);
    p.write(0x50, 0);
    assert_eq!(p.completion(1), (0, 1));
    assert_eq!(p.bytes(STATUS - 1, 3), &[0x3b, 1, 0x4c]);
    assert_eq!(p.bytes(DATA, 1024), &[0x9a; 1024]);
    assert_eq!(p.machine.data_disk().unwrap(), &[0x55; 1024]);
    assert!(!p.machine.disk_dirty());
    assert_eq!(p.machine.disk_fault(), None);
}

#[test]
fn indices_wrap_and_suppressed_notifications_do_not_assert_irq() {
    let mut p = Probe::new(32);
    p.negotiate();
    p.configure(USED);
    p.write(0x70, 15);
    p.header(4, 0); // FLUSH was not advertised, so it must return UNSUPP.
    p.put(AVAIL, &1u16.to_le_bytes());
    for i in 0..4u16 {
        p.descriptor(i * 2, HEADER, 16, NEXT, i * 2 + 1);
        p.descriptor(i * 2 + 1, STATUS + u64::from(i), 1, WRITE, 0);
    }
    p.available(0, &[0, 2, 4, 6, 0, 2, 4, 6]);
    let mut index = 0u16;
    for _ in 0..=u16::MAX / 4 {
        index = index.wrapping_add(4);
        p.put(AVAIL + 2, &index.to_le_bytes());
        p.write(0x50, 0);
    }
    assert_eq!(p.used_index(), 0);
    assert_eq!(p.bytes(STATUS, 4), &[2; 4]);
    p.put(STATUS, &[0xff; 4]);
    p.available(4, &[]);
    p.write(0x50, 0);
    assert_eq!(p.used_index(), 4);
    assert_eq!(p.completion(3), (6, 1));
    assert_eq!(p.bytes(STATUS, 4), &[2; 4]);
    assert_eq!(p.read(0x60), 0);
    assert_eq!(p.machine.disk_fault(), None);
}

#[test]
fn serial_board_has_no_block_mmio_mapping() {
    let mut machine = Machine::new();
    let [a, b] = constant(MMIO, 1);
    let code = [a, b, 1 << 15 | 6 << 12 | 3 << 7 | 0x03];
    machine
        .load(DRAM_BASE, code.map(u32::to_le_bytes).as_flattened())
        .unwrap();
    assert_eq!(machine.run(3).retired, 2);
    let trap = machine.debug_state().last_trap.unwrap();
    assert_eq!((trap.cause, trap.value), (5, u64::from(MMIO)));
    assert_eq!(machine.data_disk(), None);
}
