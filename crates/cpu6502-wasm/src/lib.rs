//! A bounded RAM host for the unchanged NMOS CPU, exported to JavaScript.

use hesper_cpu6502::{Cpu, Ram};
use hesper_wasm_support::{self as js, CycleValue, DebugValue, RegistersValue, StepValue};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
export interface RunSummary { readonly cycles: number; readonly completedSteps: number; }
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "RunSummary")]
    pub type RunSummary;
}

/// An independent CPU plus zero-filled 64 KiB RAM. Call reset after loading a vector.
#[wasm_bindgen]
pub struct Cpu6502Ram {
    cpu: Cpu,
    ram: Ram,
}

#[wasm_bindgen]
impl Cpu6502Ram {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            ram: Ram::new(),
        }
    }

    /// Non-wrapping, transactional host load; does not run the CPU.
    pub fn load(&mut self, address: f64, bytes: &[u8]) -> Result<(), JsValue> {
        let address = js::address(address)?;
        self.ram
            .load(address, bytes)
            .map_err(|e| js::error("InvalidLoad", &e.to_string()))
    }

    /// Detached, side-effect-free RAM copy. This host has no mapped devices.
    #[wasm_bindgen(js_name = readMemory)]
    pub fn read_memory(&self, address: f64, length: f64) -> Result<Vec<u8>, JsValue> {
        let start = usize::from(js::address(address)?);
        let length = js::integer(length, 0, 65536, "length")? as usize;
        if length > 65536 - start {
            return Err(js::error("InvalidRange", "memory range exceeds 64 KiB"));
        }
        Ok(self.ram.as_slice()[start..start + length].to_vec())
    }

    pub fn registers(&self) -> RegistersValue {
        js::registers(self.cpu.registers())
    }

    #[wasm_bindgen(js_name = debugState)]
    pub fn debug_state(&self) -> DebugValue {
        js::debug(self.cpu.debug_state())
    }

    #[wasm_bindgen(js_name = halfCycle)]
    pub fn half_cycle(&mut self) -> Result<Option<CycleValue>, JsValue> {
        self.cpu
            .half_cycle(&mut self.ram)
            .map(|c| c.map(js::cycle))
            .map_err(js::cpu_error)
    }

    pub fn cycle(&mut self) -> Result<CycleValue, JsValue> {
        self.cpu
            .cycle(&mut self.ram)
            .map(js::cycle)
            .map_err(js::cpu_error)
    }

    /// Finish an instruction/entry, retaining in-flight state on budget exhaustion.
    pub fn step(&mut self, cycle_budget: f64) -> Result<StepValue, JsValue> {
        self.cpu
            .step_with_cycle_budget(&mut self.ram, js::budget(cycle_budget)?)
            .map(js::step)
            .map_err(js::cpu_error)
    }

    /// Run exactly this many cycle() calls, stopping at the first CPU error.
    /// Normal budget completion is successful, even halfway through an instruction.
    #[wasm_bindgen(js_name = runCycles)]
    pub fn run_cycles(&mut self, cycle_budget: f64) -> Result<RunSummary, JsValue> {
        let budget = js::budget(cycle_budget)?;
        let mut completed = 0u32;
        for _ in 0..budget {
            let cycle = self.cpu.cycle(&mut self.ram).map_err(js::cpu_error)?;
            completed += u32::from(cycle.completed.is_some());
        }
        Ok(js::object([
            ("cycles", (budget as u32).into()),
            ("completedSteps", completed.into()),
        ])
        .unchecked_into())
    }

    /// Host-requested seven-cycle entry; distinct from the physical RESET line.
    #[wasm_bindgen(js_name = beginReset)]
    pub fn begin_reset(&mut self) {
        self.cpu.begin_reset();
    }

    /// A timeout is resumable with cycle/step. Calling reset again starts a new entry.
    pub fn reset(&mut self) -> Result<StepValue, JsValue> {
        self.cpu
            .reset(&mut self.ram)
            .map(js::step)
            .map_err(js::cpu_error)
    }

    #[wasm_bindgen(js_name = setResetLine)]
    pub fn set_reset_line(&mut self, asserted: bool) {
        self.cpu.set_reset_line(asserted);
    }
    #[wasm_bindgen(js_name = setIrqLine)]
    pub fn set_irq_line(&mut self, asserted: bool) {
        self.cpu.set_irq_line(asserted);
    }
    #[wasm_bindgen(js_name = setNmiLine)]
    pub fn set_nmi_line(&mut self, asserted: bool) {
        self.cpu.set_nmi_line(asserted);
    }
    #[wasm_bindgen(js_name = setReady)]
    pub fn set_ready(&mut self, ready: bool) {
        self.cpu.set_ready(ready);
    }
    #[wasm_bindgen(js_name = setSoLine)]
    pub fn set_so_line(&mut self, asserted: bool) {
        self.cpu.set_so_line(asserted);
    }
}

impl Default for Cpu6502Ram {
    fn default() -> Self {
        Self::new()
    }
}
