//! JavaScript host for the existing Apple I board-clock machine.

use hesper_apple1::{Apple1 as Machine, Tick};
use hesper_wasm_support::{self as js, RegistersValue};
use wasm_bindgen::prelude::*;

const INPUT_LIMIT: usize = 4096;
const OUTPUT_LIMIT: usize = 4096;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
export interface CursorSnapshot {
    readonly row: number; readonly column: number; readonly visible: boolean;
}
export interface TickSnapshot {
    readonly video: { readonly luminance: boolean; readonly sync: boolean;
        readonly hsync: boolean; readonly vsync: boolean; readonly dotEdge: boolean };
    readonly cpu: CycleSnapshot | null; readonly refresh: boolean; readonly frameCompleted: boolean;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "CursorSnapshot")]
    pub type CursorValue;
    #[wasm_bindgen(typescript_type = "TickSnapshot")]
    pub type TickValue;
}

/// Each instance owns its machine. ROM bytes are supplied by the caller.
#[wasm_bindgen]
pub struct Apple1 {
    machine: Machine,
    output: Vec<u8>,
}

#[wasm_bindgen]
impl Apple1 {
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8], expansion_ram: bool) -> Result<Apple1, JsValue> {
        Ok(Self {
            machine: Machine::with_expansion_ram(rom, expansion_ram)
                .map_err(|e| js::error("InvalidRom", &e.to_string()))?,
            output: Vec::new(),
        })
    }

    /// Load only installed contiguous RAM; failure leaves RAM untouched.
    #[wasm_bindgen(js_name = loadRam)]
    pub fn load_ram(&mut self, address: f64, bytes: &[u8]) -> Result<(), JsValue> {
        self.machine
            .bus_mut()
            .load_ram(js::address(address)?, bytes)
            .map_err(|e| js::error("InvalidLoad", &e.to_string()))
    }

    /// Raw keyboard byte, masked to 7 bits and ASCII-uppercased by the machine.
    #[wasm_bindgen(js_name = typeChar)]
    pub fn type_char(&mut self, byte: f64) -> Result<(), JsValue> {
        let byte = js::integer(byte, 0, 255, "byte")? as u8;
        self.check_input(1)?;
        self.machine.type_char(byte);
        Ok(())
    }

    /// Printable ASCII and line breaks; CRLF/LF normalize to CR. Rejects Unicode.
    /// The entire text is validated before any key is queued.
    #[wasm_bindgen(js_name = typeText)]
    pub fn type_text(&mut self, text: &str) -> Result<(), JsValue> {
        if !text
            .bytes()
            .all(|b| matches!(b, b'\r' | b'\n' | 0x20..=0x7e))
        {
            return Err(js::error(
                "InvalidText",
                "text must be printable ASCII or CR/LF; use typeChar for control bytes",
            ));
        }
        // Bound the input before allocating the normalized copy.
        if text.len() > INPUT_LIMIT * 2 {
            return Err(js::error(
                "InputLimit",
                "keyboard queue limit is 4096 bytes",
            ));
        }
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        self.check_input(text.len())?;
        self.machine.type_str(&text);
        Ok(())
    }

    /// One master tick, including digital video. Output remains available to drainOutput.
    pub fn tick(&mut self) -> Result<TickValue, JsValue> {
        let t = self.advance()?;
        Ok(js::object([
            (
                "video",
                js::object([
                    ("luminance", t.video.luminance.into()),
                    ("sync", t.video.sync.into()),
                    ("hsync", t.video.hsync.into()),
                    ("vsync", t.video.vsync.into()),
                    ("dotEdge", t.video.dot_edge.into()),
                ]),
            ),
            ("cpu", t.cpu.map_or(JsValue::NULL, |c| js::cycle(c).into())),
            ("refresh", t.refresh.into()),
            ("frameCompleted", t.frame_completed.into()),
        ])
        .unchecked_into())
    }

    /// Run 1..=1,000,000 master ticks and drain completed characters on success.
    /// On failure, completed output remains available through drainOutput.
    #[wasm_bindgen(js_name = runTicks)]
    pub fn run_ticks(&mut self, tick_budget: f64) -> Result<Vec<u8>, JsValue> {
        for _ in 0..js::budget(tick_budget)? {
            self.advance()?;
        }
        Ok(self.drain_output())
    }

    #[wasm_bindgen(js_name = drainOutput)]
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Row-major 40 x 24 ASCII host projection, copied out of Wasm memory.
    pub fn screen(&self) -> Vec<u8> {
        self.machine
            .display()
            .screen()
            .iter()
            .flatten()
            .copied()
            .collect()
    }

    pub fn cursor(&self) -> CursorValue {
        let (row, column) = self.machine.display().cursor();
        js::object([
            ("row", (row as u32).into()),
            ("column", (column as u32).into()),
            ("visible", (row < 24 && column < 40).into()),
        ])
        .unchecked_into()
    }

    pub fn registers(&self) -> RegistersValue {
        js::registers(self.machine.cpu().registers())
    }

    /// Physical board RESET, preserving screen, clocks, and unread keyboard input.
    pub fn reset(&mut self) -> Result<(), JsValue> {
        // Drain before a synchronous reset if a caller has filled the output queue.
        // With PIA reset asserted, at most the already-offered character can finish.
        self.check_output()?;
        let result = self.machine.reset();
        self.output.extend(self.machine.drain_output());
        result.map_err(js::cpu_error)
    }

    #[wasm_bindgen(js_name = setResetLine)]
    pub fn set_reset_line(&mut self, asserted: bool) {
        self.machine.set_reset_line(asserted);
    }

    /// Atomic video clear: no CPU cycle, no board clock reset, no keyboard flush.
    #[wasm_bindgen(js_name = clearScreen)]
    pub fn clear_screen(&mut self) {
        self.machine.clear_screen();
    }

    #[wasm_bindgen(js_name = masterTicks)]
    pub fn master_ticks(&self) -> u64 {
        self.machine.master_ticks()
    }
    #[wasm_bindgen(js_name = cpuCycles)]
    pub fn cpu_cycles(&self) -> u64 {
        self.machine.cpu_cycles()
    }
    #[wasm_bindgen(js_name = videoFrames)]
    pub fn video_frames(&self) -> u64 {
        self.machine.video_frames()
    }
    #[wasm_bindgen(js_name = ioPending)]
    pub fn io_pending(&self) -> bool {
        self.machine.io_pending()
    }
}

impl Apple1 {
    fn check_input(&self, additional: usize) -> Result<(), JsValue> {
        if additional > INPUT_LIMIT.saturating_sub(self.machine.keyboard().pending_len()) {
            Err(js::error(
                "InputLimit",
                "keyboard queue limit is 4096 bytes",
            ))
        } else {
            Ok(())
        }
    }

    fn check_output(&self) -> Result<(), JsValue> {
        if self.output.len() >= OUTPUT_LIMIT {
            Err(js::error(
                "OutputLimit",
                "drainOutput before continuing: output queue contains 4096 bytes",
            ))
        } else {
            Ok(())
        }
    }

    fn advance(&mut self) -> Result<Tick, JsValue> {
        self.check_output()?;
        let result = self.machine.tick();
        // Even an unsupported opcode can coincide with a completed video write.
        self.output.extend(self.machine.drain_output());
        result.map_err(js::cpu_error)
    }
}
