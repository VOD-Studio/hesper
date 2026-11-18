use hesper_riscv_linux::{
    BootError, BootImages, DRAM_BASE, DataDisk, FDT_ADDRESS, HartIsa, INITRAMFS_ADDRESS,
    KERNEL_ADDRESS, MAX_INITRAMFS_BYTES, Machine, PrivilegeMode,
};
use std::ops::Range;

const FIRMWARE_END: u64 = DRAM_BASE + 0x1000;
const STALE_ADDRESS: u64 = KERNEL_ADDRESS + 0x100;
const STALE_BYTES: &[u8] = b"old guest RAM";
const LOCKED_INITRAMFS_LEN: usize = 512;
const LOCKED_INITRD_END: u64 = INITRAMFS_ADDRESS + LOCKED_INITRAMFS_LEN as u64;

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
    test_dtb(initrd, firmware_end, false)
}

fn test_dtb(initrd: Range<u64>, firmware_end: u64, duplicate_initrd_end: bool) -> Vec<u8> {
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
    // The root property is a decoy; only the matching /chosen property may change.
    property(
        &mut tree,
        "linux,initrd-end",
        &LOCKED_INITRD_END.to_be_bytes(),
    );
    node(&mut tree, "chosen");
    property(&mut tree, "bootargs", b"console=ttyS0 rdinit=/init\0");
    property(&mut tree, "linux,initrd-start", &initrd.start.to_be_bytes());
    property(&mut tree, "linux,initrd-end", &initrd.end.to_be_bytes());
    if duplicate_initrd_end {
        property(&mut tree, "linux,initrd-end", &initrd.end.to_be_bytes());
    }
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

fn newc_entry(archive: &mut Vec<u8>, name: &[u8], data: &[u8]) {
    archive.extend_from_slice(b"070701");
    for value in [
        0,
        if name.starts_with(b"TRAILER!!!\0") {
            0
        } else {
            0o100755
        },
        0,
        0,
        1,
        0,
        data.len(),
        0,
        0,
        0,
        0,
        name.len(),
        0,
    ] {
        archive.extend_from_slice(format!("{value:08x}").as_bytes());
    }
    archive.extend_from_slice(name);
    archive.resize(archive.len().next_multiple_of(4), 0);
    archive.extend_from_slice(data);
    archive.resize(archive.len().next_multiple_of(4), 0);
}

fn newc_archive(data: &[u8]) -> Vec<u8> {
    let mut archive = Vec::new();
    newc_entry(&mut archive, b"init\0\0\0", data);
    newc_entry(&mut archive, b"TRAILER!!!\0\0", b"");
    archive
}

#[test]
fn imafdc_cold_boot_with_disk_exposes_f_and_d_and_owns_the_disk() {
    let mut fixture = Fixture::new(7, FIRMWARE_END);
    fixture.firmware = 0x3010_22f3u32.to_le_bytes(); // csrr x5, misa
    let contents = [0xab; 512];
    let mut machine = Machine::boot_with_isa_and_disk(
        fixture.images(),
        HartIsa::Imafdc,
        DataDisk::new(contents.to_vec()).unwrap(),
    )
    .unwrap();
    let fp_bits = (1 << (b'F' - b'A')) | (1 << (b'D' - b'A'));
    assert_eq!(machine.run(1).retired, 1);
    assert_eq!(machine.debug_state().x[5] & fp_bits, fp_bits);
    assert_eq!(machine.data_disk(), Some(contents.as_slice()));
}

#[test]
fn replacement_derives_only_the_guest_initrd_end_and_preserves_the_source_tree() {
    let base = Fixture::new(7, FIRMWARE_END);
    let locked_dtb = dtb(INITRAMFS_ADDRESS..LOCKED_INITRD_END, base.firmware_end);
    let mut locked_archive = base.initramfs.to_vec();
    locked_archive.resize(LOCKED_INITRAMFS_LEN, 0);
    let mut equal = newc_archive(b"different init");
    equal.resize(LOCKED_INITRAMFS_LEN, 0);
    assert_ne!(equal, locked_archive);
    let short = newc_archive(b"shorter");
    let long = newc_archive(&[0xa5; LOCKED_INITRAMFS_LEN]);
    assert!(short.len() < LOCKED_INITRAMFS_LEN && long.len() > LOCKED_INITRAMFS_LEN);

    let short_images = BootImages {
        initramfs: &short,
        fdt: &locked_dtb,
        ..base.images()
    };
    assert_eq!(
        Machine::boot(short_images).err(),
        Some(BootError::Format("DTB"))
    );

    let disk = DataDisk::new(vec![0xab; 512]).unwrap();
    let mut machine = Machine::boot_with_initramfs(
        BootImages {
            initramfs: &short,
            fdt: &locked_dtb,
            ..base.images()
        },
        LOCKED_INITRAMFS_LEN,
        Some(disk),
    )
    .unwrap();
    for (index, archive) in [&short, &equal, &long].into_iter().enumerate() {
        if index != 0 {
            let limit = INITRAMFS_ADDRESS + archive.len() as u64;
            machine.load(limit - 1, b"last byte and beyond").unwrap();
            machine
                .reboot_with_initramfs(
                    BootImages {
                        initramfs: archive,
                        fdt: &locked_dtb,
                        ..base.images()
                    },
                    LOCKED_INITRAMFS_LEN,
                )
                .unwrap();
        }
        let end = INITRAMFS_ADDRESS + archive.len() as u64;
        let expected_dtb = dtb(INITRAMFS_ADDRESS..end, base.firmware_end);
        assert_eq!(
            machine.inspect(FDT_ADDRESS, locked_dtb.len()),
            Some(expected_dtb.as_slice())
        );
        assert_eq!(
            machine.inspect(INITRAMFS_ADDRESS, archive.len()),
            Some(archive.as_slice())
        );
        assert_eq!(machine.inspect(end, 1), Some(b"\0".as_slice()));
        assert_eq!(machine.data_disk(), Some([0xab; 512].as_slice()));
        assert_entry(&mut machine, 7);
    }
    assert_eq!(
        locked_dtb,
        dtb(INITRAMFS_ADDRESS..LOCKED_INITRD_END, base.firmware_end)
    );

    machine
        .reboot(BootImages {
            initramfs: &locked_archive,
            fdt: &locked_dtb,
            ..base.images()
        })
        .unwrap();
    assert_eq!(
        machine.inspect(FDT_ADDRESS, locked_dtb.len()),
        Some(locked_dtb.as_slice())
    );
    assert_eq!(
        machine.inspect(INITRAMFS_ADDRESS, LOCKED_INITRAMFS_LEN),
        Some(locked_archive.as_slice())
    );
}

#[test]
fn replacement_rejects_invalid_archives_without_changing_the_running_board() {
    let base = Fixture::new(7, FIRMWARE_END);
    let mut machine =
        Machine::boot_with_disk(base.images(), DataDisk::new(vec![0x35; 512]).unwrap()).unwrap();
    assert_entry(&mut machine, 7);
    assert_eq!(machine.serial_input(b"queued input"), 12);
    machine.load(STALE_ADDRESS, STALE_BYTES).unwrap();
    let state = machine.diagnostics();
    let next = Fixture::new(9, FIRMWARE_END + 0x1000);
    let mut reject = |case: &str, initramfs: &[u8], fdt: &[u8], locked_len, error| {
        let images = BootImages {
            initramfs,
            fdt,
            ..next.images()
        };
        assert_eq!(
            machine.reboot_with_initramfs(images, locked_len),
            Err(error),
            "{case}"
        );
        assert_eq!(machine.diagnostics(), state, "{case}");
        base.assert_loaded(&machine);
        assert_eq!(
            machine.inspect(STALE_ADDRESS, STALE_BYTES.len()),
            Some(STALE_BYTES),
            "{case}"
        );
        assert_eq!(machine.data_disk(), Some([0x35; 512].as_slice()), "{case}");
    };

    let mut entry = Vec::new();
    let data = b"new init!";
    newc_entry(&mut entry, b"init\0\0\0", data);
    let archive = newc_archive(data);
    let mut bad_magic = archive.clone();
    bad_magic[5] = b'2';
    let mut bad_hex = archive.clone();
    bad_hex[7] = b'x'; // unneeded c_ino is still a hex field.
    let mut bad_checksum = archive.clone();
    bad_checksum[109] = b'1';
    let mut overflowing_data = archive.clone();
    overflowing_data[54..62].copy_from_slice(b"ffffffff");
    let mut bad_name = archive.clone();
    bad_name[115] = b'x'; // nonzero after the first NUL.
    let mut unterminated_name = archive.clone();
    unterminated_name[114..117].fill(b'x');
    let mut zero_namesize = archive.clone();
    zero_namesize[94..102].copy_from_slice(b"00000000");
    let mut bad_name_padding = archive.clone();
    bad_name_padding[117] = 1;
    let mut bad_data_padding = archive.clone();
    bad_data_padding[120 + data.len()] = 1;
    let mut trailer_data = entry.clone();
    newc_entry(&mut trailer_data, b"TRAILER!!!\0", b"not empty");
    let mut bad_tail = archive.clone();
    bad_tail.extend_from_slice(&[0, 1]);
    let mut concatenated = archive.clone();
    concatenated.extend_from_slice(&archive);
    for (case, malformed) in [
        ("zero bytes", [].as_slice()),
        ("gzip", b"\x1f\x8b\x08"),
        ("crc magic", &bad_magic),
        ("truncated header", &archive[..109]),
        ("nonhex field", &bad_hex),
        ("newc checksum", &bad_checksum),
        ("truncated name", &archive[..115]),
        ("zero namesize", &zero_namesize),
        ("NUL followed by nonzero", &bad_name),
        ("missing NUL", &unterminated_name),
        ("nonzero name padding", &bad_name_padding),
        ("truncated payload", &archive[..120 + data.len() - 1]),
        ("oversized payload field", &overflowing_data),
        ("nonzero payload padding", &bad_data_padding),
        ("missing trailer", &entry),
        ("truncated trailer padding", &archive[..archive.len() - 1]),
        ("nonempty trailer payload", &trailer_data),
        ("nonzero tail", &bad_tail),
        ("concatenated archives", &concatenated),
    ] {
        reject(
            case,
            malformed,
            &next.fdt,
            next.initramfs.len(),
            BootError::Format("custom initramfs"),
        );
    }

    let locked_len = next.initramfs.len();
    let wrong_end = INITRAMFS_ADDRESS + archive.len() as u64;
    let wrong_dtb = dtb(INITRAMFS_ADDRESS..wrong_end, next.firmware_end);
    let duplicate_dtb = test_dtb(
        INITRAMFS_ADDRESS..INITRAMFS_ADDRESS + locked_len as u64,
        next.firmware_end,
        true,
    );
    for (case, fdt, len) in [
        ("DTB already describes replacement", &wrong_dtb, locked_len),
        ("incorrect locked length", &next.fdt, locked_len + 1),
        ("duplicate chosen end", &duplicate_dtb, locked_len),
    ] {
        reject(case, &archive, fdt, len, BootError::Format("DTB"));
    }
}

#[test]
fn replacement_size_limit_includes_zero_padding() {
    assert_eq!(MAX_INITRAMFS_BYTES, 16 * 1024 * 1024);
    let base = Fixture::new(7, FIRMWARE_END);
    // A trailer-only archive is loadable even though it has no /init.
    let mut archive = Vec::with_capacity(MAX_INITRAMFS_BYTES + 1);
    archive.extend_from_slice(&base.initramfs);
    archive.resize(MAX_INITRAMFS_BYTES, 0);
    let mut machine = Machine::boot_with_initramfs(
        BootImages {
            initramfs: &archive,
            ..base.images()
        },
        base.initramfs.len(),
        None,
    )
    .unwrap();
    let end = INITRAMFS_ADDRESS + MAX_INITRAMFS_BYTES as u64;
    let expected_dtb = dtb(INITRAMFS_ADDRESS..end, base.firmware_end);
    assert_eq!(
        machine.inspect(FDT_ADDRESS, base.fdt.len()),
        Some(expected_dtb.as_slice())
    );
    assert_eq!(machine.inspect(end - 1, 2), Some(b"\0\0".as_slice()));
    assert_entry(&mut machine, 7);
    archive.push(0);
    let state = machine.debug_state();
    assert_eq!(
        machine.reboot_with_initramfs(
            BootImages {
                initramfs: &archive,
                ..base.images()
            },
            base.initramfs.len(),
        ),
        Err(BootError::InitramfsTooLarge)
    );
    assert_eq!(machine.debug_state(), state);
    assert_eq!(
        machine.inspect(FDT_ADDRESS, base.fdt.len()),
        Some(expected_dtb.as_slice())
    );
}
