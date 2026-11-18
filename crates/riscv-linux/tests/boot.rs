use hesper_riscv_linux::{
    BootError, BootImages, DRAM_BASE, FDT_ADDRESS, INITRAMFS_ADDRESS, KERNEL_ADDRESS, Machine,
    PrivilegeMode,
};
use std::ops::Range;

const FIRMWARE_END: u64 = DRAM_BASE + 0x1000;
const STALE_ADDRESS: u64 = KERNEL_ADDRESS + 0x100;
const STALE_BYTES: &[u8] = b"old guest RAM";

fn image(first_instruction: u32) -> [u8; 64] {
    let mut header = [0; 64];
    header[..4].copy_from_slice(&first_instruction.to_le_bytes());
    header[4..8].copy_from_slice(&0x0000_0013u32.to_le_bytes()); // nop
    header[8..16].copy_from_slice(&(KERNEL_ADDRESS - DRAM_BASE).to_le_bytes());
    header[16..24].copy_from_slice(&0x1000u64.to_le_bytes()); // effective image size
    header[32..36].copy_from_slice(&2u32.to_le_bytes()); // version 0.2
    header[48..56].copy_from_slice(b"RISCV\0\0\0");
    header[56..60].copy_from_slice(b"RSC\x05");
    header
}

fn dtb(initrd: Range<u64>, firmware_end: u64) -> Vec<u8> {
    const STRINGS: &str = concat!(
        "#address-cells\0#size-cells\0bootargs\0",
        "linux,initrd-start\0linux,initrd-end\0ranges\0reg\0no-map\0",
    );
    const END_NODE: u32 = 2;
    const END: u32 = 9;

    fn word(tree: &mut Vec<u8>, value: u32) {
        tree.extend_from_slice(&value.to_be_bytes());
    }

    fn node(tree: &mut Vec<u8>, name: &str) {
        word(tree, 1); // FDT_BEGIN_NODE
        tree.extend_from_slice(name.as_bytes());
        tree.resize((tree.len() + 1).next_multiple_of(4), 0);
    }

    fn property(tree: &mut Vec<u8>, name: &str, value: &[u8]) {
        word(tree, 3); // FDT_PROP
        word(tree, value.len() as u32);
        word(tree, STRINGS.find(name).unwrap() as u32);
        tree.extend_from_slice(value);
        tree.resize(tree.len().next_multiple_of(4), 0);
    }

    let mut tree = Vec::new();
    node(&mut tree, "");
    property(&mut tree, "#address-cells", &2u32.to_be_bytes());
    property(&mut tree, "#size-cells", &2u32.to_be_bytes());
    node(&mut tree, "chosen");
    property(&mut tree, "bootargs", b"console=ttyS0 rdinit=/init\0");
    property(&mut tree, "linux,initrd-start", &initrd.start.to_be_bytes());
    property(&mut tree, "linux,initrd-end", &initrd.end.to_be_bytes());
    word(&mut tree, END_NODE);
    node(&mut tree, "reserved-memory");
    property(&mut tree, "#address-cells", &2u32.to_be_bytes());
    property(&mut tree, "#size-cells", &2u32.to_be_bytes());
    property(&mut tree, "ranges", &[]);
    node(&mut tree, "opensbi@80000000");
    let reg = [
        DRAM_BASE.to_be_bytes(),
        (firmware_end - DRAM_BASE).to_be_bytes(),
    ];
    property(&mut tree, "reg", reg.as_flattened());
    property(&mut tree, "no-map", &[]);
    word(&mut tree, END_NODE);
    word(&mut tree, END_NODE);
    word(&mut tree, END_NODE);
    word(&mut tree, END);

    let structure_offset = 56;
    let strings_offset = structure_offset + tree.len() as u32;
    let total_size = strings_offset + STRINGS.len() as u32 + 4096;
    let header = [
        0xd00d_feed,
        total_size,
        structure_offset,
        strings_offset,
        40, // reservation map offset, followed by a 16-byte terminator
        17, // version
        16, // last compatible version
        0,  // boot hart
        STRINGS.len() as u32,
        tree.len() as u32,
    ];
    let mut bytes = Vec::with_capacity(total_size as usize);
    bytes.extend_from_slice(header.map(u32::to_be_bytes).as_flattened());
    bytes.resize(structure_offset as usize, 0);
    bytes.extend_from_slice(&tree);
    bytes.extend_from_slice(STRINGS.as_bytes());
    bytes.resize(total_size as usize, 0);
    bytes
}

struct Fixture {
    firmware: [u8; 4],
    firmware_end: u64,
    kernel: [u8; 64],
    initramfs: [u8; 124],
    fdt: Vec<u8>,
}

impl Fixture {
    fn new(value: u8, firmware_end: u64) -> Self {
        let instruction = (u32::from(value) << 20) | 0x293; // addi x5, x0, value
        // Empty newc archive: a 110-byte header, trailer name and 4-byte alignment.
        let mut initramfs = [b'0'; 124];
        initramfs[..6].copy_from_slice(b"070701");
        initramfs[6 + 4 * 8 + 7] = b'1'; // nlink
        initramfs[6 + 5 * 8 + 7] = b'0' + value; // mtime distinguishes the fixtures
        initramfs[6 + 11 * 8 + 7] = b'b'; // namesize, including NUL
        initramfs[110..].copy_from_slice(b"TRAILER!!!\0\0\0\0");
        Self {
            firmware: instruction.to_le_bytes(),
            firmware_end,
            kernel: image(instruction),
            initramfs,
            fdt: dtb(
                INITRAMFS_ADDRESS..INITRAMFS_ADDRESS + initramfs.len() as u64,
                firmware_end,
            ),
        }
    }

    fn images(&self) -> BootImages<'_> {
        BootImages {
            firmware: &self.firmware,
            firmware_end: self.firmware_end,
            kernel: &self.kernel,
            initramfs: &self.initramfs,
            fdt: &self.fdt,
        }
    }

    fn assert_loaded(&self, machine: &Machine) {
        for (address, bytes) in [
            (DRAM_BASE, self.firmware.as_slice()),
            (KERNEL_ADDRESS, self.kernel.as_slice()),
            (INITRAMFS_ADDRESS, self.initramfs.as_slice()),
            (FDT_ADDRESS, self.fdt.as_slice()),
        ] {
            assert_eq!(
                machine.inspect(address, bytes.len()),
                Some(bytes),
                "{address:#x}"
            );
        }
    }
}

fn assert_entry(machine: &mut Machine, value: u64) {
    let entry = machine.debug_state();
    let mut x = [0; 32];
    x[11] = FDT_ADDRESS; // a0 is hart ID 0; a1 is the DTB pointer.
    assert_eq!(entry.x, x);
    assert_eq!(entry.pc, DRAM_BASE);
    assert_eq!(entry.mode, PrivilegeMode::Machine);
    assert_eq!((entry.retired, entry.time, entry.last_trap), (0, 0, None));

    let progress = machine.run(1);
    assert_eq!((progress.spent, progress.retired, progress.time), (1, 1, 1));
    let state = machine.debug_state();
    assert_eq!(state.pc, DRAM_BASE + 4);
    assert_eq!(state.x[5], value);
    assert_eq!(state.last_trap, None);
}

#[test]
fn boot_and_reboot_install_images_and_enter_firmware_in_machine_mode() {
    let original = Fixture::new(7, FIRMWARE_END);
    let mut machine = Machine::boot(original.images()).unwrap();
    original.assert_loaded(&machine);
    assert_entry(&mut machine, 7);

    machine.load(STALE_ADDRESS, STALE_BYTES).unwrap();
    let next = Fixture::new(9, FIRMWARE_END + 0x1000);
    machine.reboot(next.images()).unwrap();
    next.assert_loaded(&machine);
    assert_eq!(
        machine.inspect(STALE_ADDRESS, STALE_BYTES.len()),
        Some(&[0; STALE_BYTES.len()][..])
    );
    assert_entry(&mut machine, 9);
}

#[test]
fn rejected_reboots_preserve_existing_ram_and_execution_state() {
    let original = Fixture::new(7, FIRMWARE_END);
    let mut machine = Machine::boot(original.images()).unwrap();
    assert_entry(&mut machine, 7);
    machine.load(STALE_ADDRESS, STALE_BYTES).unwrap();
    let state = machine.debug_state();
    let mut reject = |case: &str, images: BootImages<'_>, error| {
        assert_eq!(machine.reboot(images), Err(error), "{case}");
        assert_eq!(machine.debug_state(), state, "{case}");
        original.assert_loaded(&machine);
        assert_eq!(
            machine.inspect(STALE_ADDRESS, STALE_BYTES.len()),
            Some(STALE_BYTES),
            "{case}"
        );
    };

    let overlapping = Fixture::new(9, KERNEL_ADDRESS + 0x1000);
    reject(
        "firmware resident extent",
        overlapping.images(),
        BootError::Overlap("OpenSBI", "Linux Image"),
    );

    let next = Fixture::new(9, FIRMWARE_END + 0x1000);
    let mut kernel = next.kernel;
    let extent = INITRAMFS_ADDRESS - KERNEL_ADDRESS + 1;
    kernel[16..24].copy_from_slice(&extent.to_le_bytes());
    reject(
        "effective Image extent",
        BootImages {
            kernel: &kernel,
            ..next.images()
        },
        BootError::Overlap("Linux Image", "initramfs"),
    );

    {
        let mut initramfs = vec![0; (FDT_ADDRESS - INITRAMFS_ADDRESS) as usize + 1];
        initramfs[..6].copy_from_slice(b"070701");
        reject(
            "initramfs extends into DTB",
            BootImages {
                initramfs: &initramfs,
                ..next.images()
            },
            BootError::Overlap("initramfs", "DTB"),
        );
    }

    for size in [next.fdt.len() as u32 - 1, next.fdt.len() as u32 + 1] {
        let mut fdt = next.fdt.clone();
        fdt[4..8].copy_from_slice(&size.to_be_bytes());
        reject(
            "DTB totalsize",
            BootImages {
                fdt: &fdt,
                ..next.images()
            },
            BootError::Format("DTB"),
        );
    }
    let initrd_end = INITRAMFS_ADDRESS + next.initramfs.len() as u64;
    for initrd in [
        INITRAMFS_ADDRESS + 4..initrd_end,
        INITRAMFS_ADDRESS..initrd_end - 1,
    ] {
        let fdt = dtb(initrd, next.firmware_end);
        reject(
            "DTB initrd addresses",
            BootImages {
                fdt: &fdt,
                ..next.images()
            },
            BootError::Format("DTB"),
        );
    }

    let format_error = BootError::Format("Linux Image");
    reject(
        "truncated Image header",
        BootImages {
            kernel: &next.kernel[..63],
            ..next.images()
        },
        format_error,
    );
    for (case, field, bytes) in [
        ("Image magic2", 56..60, b"bad!".as_slice()),
        ("Image version", 32..36, &0x0001_0000u32.to_le_bytes()),
        ("Image flags (big-endian)", 24..32, &1u64.to_le_bytes()),
        ("Image text offset", 8..16, &0u64.to_le_bytes()),
        ("Image size", 16..24, &0u64.to_le_bytes()),
    ] {
        let mut kernel = next.kernel;
        kernel[field].copy_from_slice(bytes);
        reject(
            case,
            BootImages {
                kernel: &kernel,
                ..next.images()
            },
            format_error,
        );
    }
}
