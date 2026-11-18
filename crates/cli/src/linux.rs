//! Native byte-stream host for the pinned RISC-V Linux guest.

use std::{
    error::Error,
    fs::{self, File},
    io::{self, IsTerminal, Read, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use hesper_riscv_linux::{
    BootError, BootImages, DRAM_BASE, DataDisk, FDT_ADDRESS, INITRAMFS_ADDRESS, KERNEL_ADDRESS,
    Machine,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::terminal::{TerminalGuard, TerminalMode};

const IMAGE_LOCK: &str = include_str!("../../../tools/riscv-linux/images.lock.json");
const BLK_IMAGE_LOCK: &str = include_str!("../../../tools/riscv-linux/images-blk.lock.json");
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
const DEFAULT_MAX_FUEL: u64 = 450_000_000;
const BATCH: u32 = 16_384;
const STALLED_TRAP_LIMIT: u32 = 32;
const IO_BYTES: usize = 2048;
const IDLE_WAIT: Duration = Duration::from_millis(10);
const QUIT: u8 = 0x1d;

#[derive(Debug, thiserror::Error)]
enum LinuxError {
    #[error("{0}; use 'hesper linux --help'")]
    Usage(String),
    #[error("invalid embedded Linux image lock: {0}")]
    ImageLock(String),
    #[error("{}: invalid manifest: {source}", .path.display())]
    ManifestJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{}: manifest differs from the embedded image lock", .0.display())]
    ManifestMismatch(PathBuf),
    #[error("{operation} {}: {source}", .path.display())]
    File {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("{}: not a regular file", .0.display())]
    NotRegular(PathBuf),
    #[error("{}: output already exists", .0.display())]
    OutputExists(PathBuf),
    #[error("{}: data disk length must be a positive multiple of 512 up to 16 MiB (found {bytes})", .path.display())]
    DiskSize { path: PathBuf, bytes: u64 },
    #[error("Linux data disk: {0}")]
    DiskFault(#[from] hesper_riscv_linux::DiskFault),
    #[error("{}: size mismatch (expected {expected}, found {actual})", .path.display())]
    SizeMismatch {
        path: PathBuf,
        expected: String,
        actual: u64,
    },
    #[error("{}: SHA-256 mismatch (expected {expected}, found {actual})", .path.display())]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("loading Linux guest: {0}")]
    Boot(#[from] BootError),
    #[error("{operation}: {source}")]
    HostIo {
        operation: &'static str,
        source: io::Error,
    },
    #[error("Linux guest fuel exhausted (limit {0} transitions)")]
    FuelExhausted(u64),
    #[error("Linux guest trap loop ({0} consecutive traps without retirement)")]
    TrapLoop(u32),
    #[error("Linux guest is idle with serial bytes still pending")]
    IdleWithPendingSerial,
}

struct Options {
    images: PathBuf,
    disk: Option<(PathBuf, PathBuf)>,
    max_fuel: u64,
    diagnostics: bool,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Self>, LinuxError> {
        let mut images = None;
        let (mut disk_in, mut disk_out) = (None, None);
        let mut max_fuel = DEFAULT_MAX_FUEL;
        let mut diagnostics = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--images" => {
                    let dir = args
                        .next()
                        .ok_or_else(|| LinuxError::Usage("--images requires a directory".into()))?;
                    images = Some(PathBuf::from(dir));
                }
                "--disk-in" | "--disk-out" => {
                    let path = args
                        .next()
                        .ok_or_else(|| LinuxError::Usage(format!("{arg} requires a file")))?;
                    let option = if arg == "--disk-in" {
                        &mut disk_in
                    } else {
                        &mut disk_out
                    };
                    if option.replace(PathBuf::from(path)).is_some() {
                        return Err(LinuxError::Usage(format!("{arg} specified more than once")));
                    }
                }
                "--max-fuel" => {
                    max_fuel = args
                        .next()
                        .and_then(|n| n.parse().ok())
                        .filter(|&n| n > 0)
                        .ok_or_else(|| {
                            LinuxError::Usage("--max-fuel requires a positive integer".into())
                        })?;
                }
                "--diagnostics" => diagnostics = true,
                "--help" | "-h" => return Ok(None),
                _ => return Err(LinuxError::Usage(format!("unknown linux argument: {arg}"))),
            }
        }
        let disk = match (disk_in, disk_out) {
            (None, None) => None,
            (Some(input), Some(output)) => Some((input, output)),
            _ => {
                return Err(LinuxError::Usage(
                    "--disk-in and --disk-out must be used together".into(),
                ));
            }
        };
        Ok(Some(Self {
            images: images.ok_or_else(|| LinuxError::Usage("--images is required".into()))?,
            disk,
            max_fuel,
            diagnostics,
        }))
    }
}

#[derive(Deserialize)]
struct ImageLock {
    images: [Image; 4],
}

#[derive(Deserialize)]
struct Image {
    path: String,
    bytes: u64,
    sha256: String,
    address: String,
    end: String,
}

fn boot(directory: &Path, disk: Option<DataDisk>) -> Result<Machine, LinuxError> {
    let path = directory.join("manifest.json");
    let actual = serde_json::from_slice::<Value>(&read_bounded(&path, MAX_MANIFEST_BYTES, false)?)
        .map_err(|source| LinuxError::ManifestJson {
            path: path.clone(),
            source,
        })?;
    let lock = if disk.is_some() {
        BLK_IMAGE_LOCK
    } else {
        IMAGE_LOCK
    };
    let pinned: Value =
        serde_json::from_str(lock).map_err(|err| LinuxError::ImageLock(err.to_string()))?;
    if actual != pinned {
        return Err(LinuxError::ManifestMismatch(path));
    }
    let pinned: ImageLock =
        serde_json::from_value(pinned).map_err(|err| LinuxError::ImageLock(err.to_string()))?;
    let [firmware, kernel, initramfs, fdt] = pinned.images;
    for (image, name, address) in [
        (&firmware, "fw_jump.bin", DRAM_BASE),
        (&kernel, "Image", KERNEL_ADDRESS),
        (&initramfs, "initramfs.cpio", INITRAMFS_ADDRESS),
        (&fdt, "board.dtb", FDT_ADDRESS),
    ] {
        if image.path != name || hex_address(&image.address) != Some(address) {
            return Err(LinuxError::ImageLock(format!(
                "expected {name} at {address:#x}"
            )));
        }
    }
    let firmware_end = hex_address(&firmware.end)
        .ok_or_else(|| LinuxError::ImageLock("invalid fw_jump.bin end address".into()))?;
    let images = BootImages {
        firmware: &verified_image(directory, firmware)?,
        firmware_end,
        kernel: &verified_image(directory, kernel)?,
        initramfs: &verified_image(directory, initramfs)?,
        fdt: &verified_image(directory, fdt)?,
    };
    Ok(match disk {
        Some(disk) => Machine::boot_with_disk(images, disk)?,
        None => Machine::boot(images)?,
    })
}

fn hex_address(text: &str) -> Option<u64> {
    u64::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

fn verified_image(directory: &Path, image: Image) -> Result<Vec<u8>, LinuxError> {
    let path = directory.join(image.path);
    let bytes = read_bounded(&path, image.bytes, true)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let actual = Sha256::digest(&bytes);
    let matches = actual
        .iter()
        .flat_map(|&byte| [HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]])
        .eq(image.sha256.bytes());
    if !matches {
        return Err(LinuxError::HashMismatch {
            path,
            expected: image.sha256,
            actual: format!("{actual:x}"),
        });
    }
    Ok(bytes)
}

fn open_image(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        // Reject non-regular files even if opening one (e.g. a FIFO) would block.
        use rustix::fs::{Mode, OFlags, open};
        Ok(open(
            path,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?
        .into())
    }
    #[cfg(not(unix))]
    File::open(path)
}

fn read_bounded(path: &Path, limit: u64, exact: bool) -> Result<Vec<u8>, LinuxError> {
    read_file(path, limit, |actual| {
        (actual > limit || (exact && actual != limit)).then(|| LinuxError::SizeMismatch {
            path: path.into(),
            expected: if exact {
                format!("{limit} bytes")
            } else {
                format!("at most {limit} bytes")
            },
            actual,
        })
    })
}

fn read_file(
    path: &Path,
    limit: u64,
    size_error: impl Fn(u64) -> Option<LinuxError>,
) -> Result<Vec<u8>, LinuxError> {
    let file = open_image(path).map_err(|source| path_error("opening", path, source))?;
    let metadata = file
        .metadata()
        .map_err(|source| path_error("checking", path, source))?;
    if !metadata.is_file() {
        return Err(LinuxError::NotRegular(path.into()));
    }
    let size = metadata.len();
    if let Some(error) = size_error(size) {
        return Err(error);
    }
    // Metadata is not a read limit: the file can change after the check.
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| path_error("reading", path, source))?;
    if let Some(error) = size_error(bytes.len() as u64) {
        return Err(error);
    }
    Ok(bytes)
}

fn path_error(operation: &'static str, path: &Path, source: io::Error) -> LinuxError {
    LinuxError::File {
        operation,
        path: path.into(),
        source,
    }
}

fn load_disk(input: &Path, output: &Path) -> Result<DataDisk, LinuxError> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err(LinuxError::OutputExists(output.into())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(path_error("checking data disk output", output, source)),
    }
    let invalid_size = |bytes| LinuxError::DiskSize {
        path: input.into(),
        bytes,
    };
    let bytes = read_file(input, DataDisk::MAX_BYTES as u64, |bytes| {
        (!usize::try_from(bytes).is_ok_and(DataDisk::valid_size)).then(|| invalid_size(bytes))
    })?;
    let size = bytes.len() as u64;
    DataDisk::new(bytes).map_err(|_| invalid_size(size))
}

fn save_disk(output: &Path, bytes: &[u8]) -> Result<(), LinuxError> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let save = || -> io::Result<()> {
        let mut temp = tempfile::Builder::new()
            .prefix(".hesper-disk-")
            .tempfile_in(parent)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        // Hard-link publication is atomic and fails if any name now occupies
        // the destination. The temporary file is removed on both paths.
        fs::hard_link(temp.path(), output)
    };
    save().map_err(|source| path_error("saving data disk to", output, source))
}

type InputBuffer = [u8; IO_BYTES];

// Poll the byte stream directly, rather than decoding terminal keys or lines.
#[cfg(target_os = "linux")]
struct Input(io::Stdin);

#[cfg(target_os = "linux")]
impl Input {
    fn start() -> io::Result<Self> {
        Ok(Self(io::stdin()))
    }

    fn read(&self, bytes: &mut InputBuffer, wait: bool) -> io::Result<Option<usize>> {
        use rustix::{
            event::{PollFd, PollFlags, Timespec, poll},
            io::{Errno, read},
        };
        let mut fds = [PollFd::new(&self.0, PollFlags::IN)];
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: if wait {
                IDLE_WAIT.subsec_nanos().into()
            } else {
                0
            },
        };
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(Errno::INTR) => Ok(None),
            Ok(_) => match read(&self.0, &mut bytes[..]) {
                Ok(count) => Ok(Some(count)),
                Err(Errno::INTR | Errno::AGAIN) => Ok(None),
                Err(err) => Err(err.into()),
            },
            Err(err) => Err(err.into()),
        }
    }
}

// A process-stdio worker for platforms without the fd polling path above.
#[cfg(not(target_os = "linux"))]
struct Input(std::sync::mpsc::Receiver<io::Result<(InputBuffer, usize)>>);

#[cfg(not(target_os = "linux"))]
impl Input {
    fn start() -> io::Result<Self> {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("hesper-linux-stdin".into())
            .spawn(move || {
                let mut stdin = io::stdin().lock();
                let mut bytes = [0; IO_BYTES];
                loop {
                    let result = match stdin.read(&mut bytes) {
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        result => result,
                    };
                    let finished = !matches!(result, Ok(1..));
                    if send.send(result.map(|count| (bytes, count))).is_err() || finished {
                        break;
                    }
                }
            })?;
        Ok(Self(receive))
    }

    fn read(&self, bytes: &mut InputBuffer, wait: bool) -> io::Result<Option<usize>> {
        use std::sync::mpsc::RecvTimeoutError;
        let timeout = if wait { IDLE_WAIT } else { Duration::ZERO };
        match self.0.recv_timeout(timeout) {
            Ok(Ok((chunk, count))) => {
                bytes[..count].copy_from_slice(&chunk[..count]);
                Ok(Some(count))
            }
            Ok(Err(err)) => Err(err),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::other("stdin reader stopped")),
        }
    }
}

struct SerialTail {
    bytes: [u8; IO_BYTES],
    len: usize,
    next: usize,
}

impl SerialTail {
    fn new() -> Self {
        Self {
            bytes: [0; IO_BYTES],
            len: 0,
            next: 0,
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        let first = bytes.len().min(IO_BYTES - self.next);
        self.bytes[self.next..self.next + first].copy_from_slice(&bytes[..first]);
        self.bytes[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        self.len = (self.len + bytes.len()).min(IO_BYTES);
        self.next = (self.next + bytes.len()) % IO_BYTES;
    }

    fn report(&self) {
        let (first, second) = if self.len == IO_BYTES {
            (&self.bytes[self.next..], &self.bytes[..self.next])
        } else {
            (&self.bytes[..self.len], &[][..])
        };
        eprintln!(
            "linux: serial tail ({} bytes): \"{}{}\"",
            self.len,
            first.escape_ascii(),
            second.escape_ascii()
        );
    }
}

#[derive(Clone, Copy)]
enum Exit {
    Eof,
    Quit,
    Signal,
}

impl Exit {
    fn name(self) -> &'static str {
        match self {
            Self::Eof => "eof",
            Self::Quit => "quit",
            Self::Signal => "signal",
        }
    }
}

struct Session {
    machine: Machine,
    spent: u64,
    tail: SerialTail,
}

impl Session {
    fn run(&mut self, max_fuel: u64) -> Result<Exit, LinuxError> {
        let terminated = io::stdin()
            .is_terminal()
            .then(|| Arc::new(AtomicBool::new(false)));
        let _guard = terminated
            .as_ref()
            .map(|flag| TerminalGuard::enter(TerminalMode::RawStream, flag))
            .transpose()
            .map_err(|source| LinuxError::HostIo {
                operation: "entering raw terminal mode",
                source,
            })?;
        let input = Input::start().map_err(|source| LinuxError::HostIo {
            operation: "starting serial input",
            source,
        })?;
        let mut stdout = io::stdout().lock();
        let mut input_bytes = [0; IO_BYTES];
        let mut pending: Range<usize> = 0..0;
        let mut input_end = None;
        let mut idle = false;
        loop {
            if terminated
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                return Ok(Exit::Signal);
            }
            if pending.is_empty() && input_end.is_none() {
                let current =
                    input
                        .read(&mut input_bytes, idle)
                        .map_err(|source| LinuxError::HostIo {
                            operation: "reading serial stdin",
                            source,
                        })?;
                match current {
                    Some(0) => input_end = Some(Exit::Eof),
                    Some(count) => {
                        let quit = input_bytes[..count].iter().position(|&byte| byte == QUIT);
                        if quit.is_some() {
                            input_end = Some(Exit::Quit);
                        }
                        pending = 0..quit.unwrap_or(count);
                    }
                    None => {}
                }
            }
            if !pending.is_empty() {
                pending.start += self.machine.serial_input(&input_bytes[pending.clone()]);
            }
            if pending.is_empty() && matches!(input_end, Some(Exit::Quit)) {
                return Ok(Exit::Quit);
            }

            let fuel = (max_fuel - self.spent).min(u64::from(BATCH)) as u32;
            let progress = self.machine.run(fuel);
            self.spent += u64::from(progress.spent);
            let output = self
                .drain(&mut stdout)
                .map_err(|source| LinuxError::HostIo {
                    operation: "writing serial stdout",
                    source,
                })?;
            let diagnostics = self.machine.diagnostics();
            if diagnostics.stalled_traps >= STALLED_TRAP_LIMIT {
                return Err(LinuxError::TrapLoop(diagnostics.stalled_traps));
            }
            if let Some(fault) = self.machine.disk_fault() {
                return Err(fault.into());
            }
            idle = progress.spent == 0 && !output;
            if matches!(input_end, Some(Exit::Eof))
                && pending.is_empty()
                && (idle || diagnostics.waiting)
                && !output
                && diagnostics.uart.input_queued == 0
                && diagnostics.uart.output_queued == 0
            {
                return Ok(Exit::Eof);
            }
            if self.spent == max_fuel {
                return Err(LinuxError::FuelExhausted(max_fuel));
            }
            if idle && (input_end.is_some() || !pending.is_empty()) {
                // No device event can make progress, and there is no new host
                // input to wait for while an old chunk is blocked or at EOF.
                // Preserve the finite budget/error contract rather than spin.
                return Err(LinuxError::IdleWithPendingSerial);
            }
        }
    }

    fn drain(&mut self, stdout: &mut impl Write) -> io::Result<bool> {
        let mut output = [0; IO_BYTES];
        let mut wrote = false;
        loop {
            let count = self.machine.serial_output(&mut output);
            if count == 0 {
                break;
            }
            let bytes = &output[..count];
            self.tail.append(bytes);
            stdout.write_all(bytes)?;
            wrote = true;
        }
        if wrote {
            stdout.flush()?;
        }
        Ok(wrote)
    }

    fn report(&self, exit: &str, max_fuel: u64) {
        let diagnostics = self.machine.diagnostics();
        eprintln!(
            "linux: exit={exit} fuel={}/{} retired={} time={}",
            self.spent, max_fuel, diagnostics.cpu.retired, diagnostics.cpu.time
        );
        eprintln!("linux: snapshot={diagnostics:#x?}");
        self.tail.report();
    }
}

/// Run the `hesper linux` command against the process's standard streams.
pub fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let Some(options) = Options::parse(args)? else {
        println!(
            "Usage: hesper linux --images <dir> [--disk-in <raw> --disk-out <raw>] [--max-fuel N] [--diagnostics]\n\nRun the pinned guest over raw serial stdin/stdout. With paired disk options, use the blk images and unmount the data disk before Ctrl-] exports it. Without a disk, Ctrl-] just exits.\nDefault fuel: {DEFAULT_MAX_FUEL} guest transitions."
        );
        return Ok(());
    };
    let disk = options
        .disk
        .as_ref()
        .map(|(input, output)| load_disk(input, output))
        .transpose()?;
    let mut session = Session {
        machine: boot(&options.images, disk)?,
        spent: 0,
        tail: SerialTail::new(),
    };
    let result = session.run(options.max_fuel);
    if let (Ok(Exit::Quit), Some((_, output))) = (&result, &options.disk) {
        let bytes = session
            .machine
            .data_disk()
            .expect("booted with a data disk");
        if let Err(error) = save_disk(output, bytes) {
            session.report("export-error", options.max_fuel);
            return Err(error.into());
        }
    }
    match &result {
        Ok(exit) if options.diagnostics => session.report(exit.name(), options.max_fuel),
        Err(LinuxError::FuelExhausted(_)) => session.report("fuel", options.max_fuel),
        Err(LinuxError::TrapLoop(_)) => session.report("trap-loop", options.max_fuel),
        Err(_) => session.report("error", options.max_fuel),
        Ok(_) => {}
    }
    result?;
    Ok(())
}
