use hesper_riscv_linux::{BootImages, FDT_ADDRESS, KERNEL_ADDRESS, Machine, PrivilegeMode};
use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::Path,
};

fn advance(machine: &mut Machine, fuel: u32, serial: &mut impl Write) -> io::Result<u32> {
    let progress = machine.run(fuel);
    let mut buffer = [0; 256];
    let count = machine.serial_output(&mut buffer);
    serial.write_all(&buffer[..count])?;
    Ok(progress.spent)
}

fn main() -> Result<(), Box<dyn Error>> {
    let [dir, end, probe] = env::args().skip(1).collect::<Vec<_>>().try_into().map_err(
        |_| "usage: opensbi_probe <verified image directory> <firmware end hex> <probe binary>",
    )?;
    let dir = Path::new(&dir);
    let firmware = fs::read(dir.join("fw_jump.bin"))?;
    let kernel = fs::read(dir.join("Image"))?;
    let initramfs = fs::read(dir.join("initramfs.cpio"))?;
    let fdt = fs::read(dir.join("board.dtb"))?;
    let mut machine = Machine::boot(BootImages {
        firmware: &firmware,
        firmware_end: u64::from_str_radix(end.trim_start_matches("0x"), 16)?,
        kernel: &kernel,
        initramfs: &initramfs,
        fdt: &fdt,
    })?;
    machine.load(KERNEL_ADDRESS, &fs::read(probe)?)?;
    let mut serial = io::stdout().lock();
    let mut handoff = None;
    for _ in 0..25_000_000 {
        let state = machine.debug_state();
        if state.pc == KERNEL_ADDRESS {
            handoff = Some(state);
            break;
        }
        advance(&mut machine, 1, &mut serial)?;
    }
    let state =
        handoff.unwrap_or_else(|| panic!("no firmware handoff: {:x?}", machine.debug_state()));
    assert_eq!(
        (state.mode, state.satp, state.x[10], state.x[11]),
        (PrivilegeMode::Supervisor, 0, 0, FDT_ADDRESS)
    );
    let mut sent = false;
    let mut reply = Vec::with_capacity(16);
    for _ in 0..256 {
        let spent = advance(&mut machine, 128, &mut reply)?;
        let signature = machine.inspect(0x8300_0000, 64).unwrap();
        let word =
            |index: usize| u64::from_le_bytes(signature[index * 8..][..8].try_into().unwrap());
        assert_eq!(word(7), 0, "guest failure; signature: {signature:02x?}");
        if !sent {
            serial.write_all(&reply)?;
            reply.clear();
        }
        if word(5) == 1 && !sent && spent < 128 {
            assert_eq!(
                (word(0), word(1), word(2), word(3), word(4)),
                (0, FDT_ADDRESS, 0, 0x0200_0000, 1)
            );
            assert_eq!(machine.serial_input(b"K"), 1);
            sent = true;
        } else if word(5) == 2 && !reply.is_empty() {
            assert!(sent);
            assert_eq!(word(6), u64::from(b'L'));
            assert_eq!(reply, b"L");
            let done = machine.debug_state();
            writeln!(
                serial,
                "probe: pc={:#x} mode={:?} satp={} hart={} fdt={:#x} SBI=2.0 timer=1 UART=K->L retired={} time={}",
                state.pc, state.mode, state.satp, state.x[10], state.x[11], done.retired, done.time
            )?;
            return Ok(());
        }
    }
    Err(format!(
        "S-mode probe did not finish (sent={sent}, reply={reply:?}): {:x?}",
        machine.debug_state()
    )
    .into())
}
