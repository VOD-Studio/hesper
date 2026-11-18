use hesper_riscv_linux::dram_range;

#[test]
fn maps_firmware_kernel_and_last_byte_to_the_same_ram() {
    assert_eq!(dram_range(0x8000_0000, 4), Some(0..4));
    assert_eq!(dram_range(0x8020_0000, 4), Some(0x20_0000..0x20_0004));
    assert_eq!(dram_range(0x87ff_ffff, 1), Some(0x07ff_ffff..0x0800_0000));
    assert_eq!(dram_range(0x8800_0000, 0), Some(0x0800_0000..0x0800_0000));
}

#[test]
fn rejects_partial_overlaps_overflow_and_truncated_high_addresses() {
    assert_eq!(dram_range(0x7fff_ffff, 2), None);
    assert_eq!(dram_range(0x87ff_ffff, 2), None);
    assert_eq!(dram_range(0x8800_0000, 1), None);
    assert_eq!(dram_range(0x8800_0001, 0), None);
    assert_eq!(dram_range(0x1_8000_0000, 1), None);
    assert_eq!(dram_range(0x8000_0001, usize::MAX), None);
    assert_eq!(dram_range(u64::MAX, 2), None);
}
