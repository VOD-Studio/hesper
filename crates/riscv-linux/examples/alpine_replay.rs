//! Replay locked Alpine images and input at fixed Machine::run boundaries.

use std::{
    env,
    error::Error,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::PathBuf,
};

use hesper_riscv_linux::{BootImages, DataDisk, HartIsa, Machine};

const BATCH: u32 = 16_384;
const INPUT: &[u8] = b"printf 'HESPER_REPLAY:%s\\n' \"$((6 * 7))\"\n";

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [
        directory,
        firmware_end,
        serial,
        boot_batches,
        command_batches,
        disk @ ..,
    ] = args.as_slice()
    else {
        return Err(
            "usage: alpine_replay IMAGES FIRMWARE_END SERIAL BOOT_BATCHES COMMAND_BATCHES [DISK]"
                .into(),
        );
    };
    if disk.len() > 1 {
        return Err("expected at most one disk".into());
    }
    let directory = PathBuf::from(directory);
    let firmware_end = u64::from_str_radix(
        firmware_end
            .to_str()
            .ok_or("expected a hexadecimal address")?
            .trim_start_matches("0x"),
        16,
    )?;
    let boot_batches: u32 = boot_batches
        .to_str()
        .ok_or("expected a boot batch count")?
        .parse()?;
    let command_batches: u32 = command_batches
        .to_str()
        .ok_or("expected a command batch count")?
        .parse()?;
    let count = boot_batches
        .checked_add(command_batches)
        .ok_or("too many batches")?;
    let firmware = fs::read(directory.join("fw_jump.bin"))?;
    let kernel = fs::read(directory.join("Image"))?;
    let initramfs = fs::read(directory.join("initramfs.cpio"))?;
    let fdt = fs::read(directory.join("board.dtb"))?;
    let images = BootImages {
        firmware: &firmware,
        firmware_end,
        kernel: &kernel,
        initramfs: &initramfs,
        fdt: &fdt,
    };
    let mut machine = if let [disk] = disk {
        Machine::boot_with_isa_and_disk(images, HartIsa::Imafdc, DataDisk::new(fs::read(disk)?)?)?
    } else {
        Machine::boot_with_isa(images, HartIsa::Imafdc)?
    };
    let mut serial = BufWriter::new(File::create(serial)?);
    let mut snapshots = BufWriter::new(io::stdout().lock());
    let mut output = [0; 4096];
    for index in 0..count {
        if index == boot_batches && machine.serial_input(INPUT) != INPUT.len() {
            return Err("guest did not accept the fixed input".into());
        }
        let progress = machine.run(BATCH);
        let mut serial_bytes = 0;
        loop {
            let length = machine.serial_output(&mut output);
            if length == 0 {
                break;
            }
            serial.write_all(&output[..length])?;
            serial_bytes += length;
        }
        writeln!(
            snapshots,
            "{index} {progress:?} {:x?} serial={serial_bytes}",
            machine.debug_state()
        )?;
    }
    serial.flush()?;
    snapshots.flush()?;
    Ok(())
}
