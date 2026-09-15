//! Bounded, ROM-free digital video capture using a real 6502 output program.
//! Usage: cargo run -p hesper-apple1 --release --example video -- OUT [TEXT] [FRAME]
//! OUT must not exist. Optional FRAME starts capture after that natural frame
//! number, even while writing/scrolling; otherwise wait for the text to settle.

use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use hesper_apple1::timing::{CRYSTAL_HZ, HORIZ_MASTER_TICKS};
use hesper_apple1::{Apple1, VideoSample};

const NORMAL_FRAME: u64 = 262 * HORIZ_MASTER_TICKS;
const MAX_CAPTURE: usize = 2048 * HORIZ_MASTER_TICKS as usize;
const DEFAULT_TEXT: &str = "HESPER APPLE-1 DIGITAL VIDEO\r@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_\r !\"#$%&'()*+,-./0123456789:;<=>?\r";

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .ok_or("usage: video OUT [TEXT] [FRAME]; OUT must not exist")?;
    let text = args.next().unwrap_or_else(|| DEFAULT_TEXT.to_owned());
    let capture_frame = args.next().map(|arg| arg.parse::<u64>()).transpose()?;
    if args.next().is_some() || capture_frame.is_some_and(|frame| frame == 0 || frame > 1024) {
        return Err("FRAME must be 1..1024; no extra arguments are accepted".into());
    }
    if text.is_empty()
        || text.len() > 255
        || !text
            .bytes()
            .all(|b| b == b'\r' || (0x20..=0x7e).contains(&b))
    {
        return Err("TEXT must contain 1..255 printable ASCII bytes or CR".into());
    }
    if Path::new(&out).exists() {
        return Err("OUT already exists; choose a new directory".into());
    }

    let mut rom = [0; 256];
    rom[0xfd] = 2;
    let mut machine = Apple1::new(&rom)?;
    // NUL-terminated string at $0300; a genuine CPU/PIA polling program.
    let program = [
        0xa9, 0x7f, 0x8d, 0x12, 0xd0, 0xa9, 0x27, 0x8d, 0x13, 0xd0, 0xa2, 0, 0xbd, 0, 3, 0xf0,
        0x0b, 0x8d, 0x12, 0xd0, 0x2c, 0x12, 0xd0, 0x30, 0xfb, 0xe8, 0xd0, 0xf0, 0x4c, 0x1c, 2,
    ];
    machine.bus_mut().load_ram(0x0200, &program)?;
    machine.bus_mut().load_ram(0x0300, text.as_bytes())?;
    machine.reset()?;

    let budget = (capture_frame.unwrap_or(text.len() as u64) + 10) * NORMAL_FRAME;
    let expected = text.to_ascii_uppercase().into_bytes();
    let mut output = Vec::new();
    let mut idle_frames = 0;
    let mut ready = false;
    for _ in 0..budget {
        let tick = machine.tick()?;
        output.extend(machine.drain_output());
        if tick.frame_completed {
            if output == expected && !machine.io_pending() {
                idle_frames += 1;
            } else {
                idle_frames = 0;
            }
            ready = capture_frame.map_or(idle_frames == 2, |n| machine.video_frames() >= n);
            if ready {
                break;
            }
        }
    }
    if !ready {
        return Err("writer/frame budget exhausted".into());
    }

    // Begin/end at actual horizontal sync edges surrounding a natural frame
    // completion. H18 controls the number of captured scan lines.
    let mut previous_hsync = machine.tick()?.video.hsync;
    let mut samples = Vec::new();
    let mut start = None;
    let mut completed = false;
    let mut ended = false;
    for _ in 0..MAX_CAPTURE {
        let at = machine.master_ticks();
        let tick = machine.tick()?;
        let rising = tick.video.hsync && !previous_hsync;
        previous_hsync = tick.video.hsync;
        if rising {
            if start.is_some() && completed {
                ended = true;
                break;
            }
            start.get_or_insert(at);
        }
        if start.is_some() {
            samples.push(tick.video);
            completed |= tick.frame_completed;
        }
        // Character diagnostics are independently bounded by TEXT length.
        machine.drain_output();
    }
    if !ended || samples.is_empty() || samples.len() % HORIZ_MASTER_TICKS as usize != 0 {
        return Err("capture budget exhausted or incomplete horizontal line".into());
    }

    fs::create_dir(&out)?;
    let directory = Path::new(&out);
    write_raster(directory, &samples)?;
    write_csv(directory, start.ok_or("no sync edge")?, &samples)?;
    write_sync(directory, &samples)?;
    println!(
        "{}: {} master samples, {} scan lines, {:.3} ms; frame.pgm, video.csv, sync.svg",
        directory.display(),
        samples.len(),
        samples.len() / HORIZ_MASTER_TICKS as usize,
        samples.len() as f64 * 1000.0 / CRYSTAL_HZ as f64
    );
    Ok(())
}

fn write_raster(directory: &Path, samples: &[VideoSample]) -> std::io::Result<()> {
    let mut file = BufWriter::new(File::create_new(directory.join("frame.pgm"))?);
    writeln!(
        file,
        "P5\n455 {}\n255",
        samples.len() / HORIZ_MASTER_TICKS as usize
    )?;
    for sample in samples.iter().filter(|sample| sample.dot_edge) {
        file.write_all(&[if sample.luminance && !sample.sync {
            255
        } else {
            0
        }])?;
    }
    file.flush()
}

fn write_csv(directory: &Path, start: u64, samples: &[VideoSample]) -> std::io::Result<()> {
    let mut file = BufWriter::new(File::create_new(directory.join("video.csv"))?);
    writeln!(file, "master_tick,luminance,hsync,vsync,sync,nominal_mv")?;
    for (index, sample) in samples.iter().enumerate() {
        writeln!(
            file,
            "{},{},{},{},{},{}",
            start + index as u64,
            u8::from(sample.luminance),
            u8::from(sample.hsync),
            u8::from(sample.vsync),
            u8::from(sample.sync),
            sample.nominal_millivolts()
        )?;
    }
    file.flush()
}

fn write_sync(directory: &Path, samples: &[VideoSample]) -> std::io::Result<()> {
    let mut file = BufWriter::new(File::create_new(directory.join("sync.svg"))?);
    writeln!(
        file,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1400 320\"><rect width=\"1400\" height=\"320\" fill=\"#10151c\"/><g font-family=\"monospace\" font-size=\"16\" fill=\"#ecf2fa\"><text x=\"20\" y=\"28\">APPLE-1 DIGITAL SYNC / {} master ticks / {:.3} ms</text>",
        samples.len(),
        samples.len() as f64 * 1000.0 / CRYSTAL_HZ as f64
    )?;
    for (name, lane) in [("HSYNC", 0), ("VSYNC", 1), ("COMPOSITE", 2)] {
        let low = 100 + lane * 80;
        writeln!(file, "<text x=\"20\" y=\"{}\">{name}</text>", low - 12)?;
        let value = |sample: &VideoSample| match lane {
            0 => sample.hsync,
            1 => sample.vsync,
            _ => sample.sync,
        };
        let y = |asserted| if asserted { low - 35 } else { low };
        write!(
            file,
            "<path fill=\"none\" stroke=\"#5ed8b0\" stroke-width=\"1\" d=\"M 150 {}",
            y(value(&samples[0]))
        )?;
        let mut previous = value(&samples[0]);
        for (index, sample) in samples.iter().enumerate().skip(1) {
            let next = value(sample);
            if next != previous {
                let x = 150.0 + index as f64 * 1220.0 / samples.len() as f64;
                write!(file, " H {x:.3} V {}", y(next))?;
                previous = next;
            }
        }
        writeln!(file, " H 1370\"/>")?;
    }
    writeln!(
        file,
        "<text x=\"150\" y=\"302\">High = asserted; ideal digital timing, not measured board voltage.</text></g></svg>"
    )?;
    file.flush()
}
