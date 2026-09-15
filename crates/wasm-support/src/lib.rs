//! Internal, shared JS value conversion for Hesper's two Wasm packages.
//! Objects are owned JS snapshots, never views into live emulated devices.

use hesper_cpu6502::{CpuError, Cycle, DebugState, Direction, Registers, Step, StepKind};
use wasm_bindgen::prelude::*;

/// Maximum work in one synchronous JS call; hosts schedule successive batches.
pub const MAX_BATCH: u32 = 1_000_000;

/// Validate before narrowing: direct wasm-bindgen integer arguments wrap/truncate.
pub fn integer(value: f64, min: u32, max: u32, name: &str) -> Result<u32, JsValue> {
    if !value.is_finite() || value.fract() != 0.0 || value < min as f64 || value > max as f64 {
        return Err(error(
            "InvalidArgument",
            &format!("{name} must be an integer in {min}..={max}"),
        ));
    }
    Ok(value as u32)
}

pub fn address(value: f64) -> Result<u16, JsValue> {
    integer(value, 0, u16::MAX.into(), "address").map(|n| n as u16)
}

pub fn budget(value: f64) -> Result<u64, JsValue> {
    integer(value, 1, MAX_BATCH, "budget").map(u64::from)
}

/// Every property is written onto a fresh, extensible object created here.
pub fn object<const N: usize>(fields: [(&str, JsValue); N]) -> JsValue {
    let object = js_sys::Object::new();
    for (key, value) in fields {
        // defineProperty avoids inherited setters on Object.prototype.
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(&descriptor, &"value".into(), &value).expect("fresh descriptor");
        js_sys::Reflect::set(&descriptor, &"enumerable".into(), &true.into())
            .expect("fresh descriptor");
        js_sys::Object::define_property(&object, &key.into(), &descriptor);
    }
    object.into()
}

pub fn error(code: &str, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    error.set_name("HesperError");
    js_sys::Reflect::set(&error, &"code".into(), &code.into()).expect("fresh error");
    error.into()
}

pub fn cpu_error(err: CpuError) -> JsValue {
    let (code, address) = match err {
        CpuError::UnsupportedOpcode { address, .. } => ("UnsupportedOpcode", address),
        CpuError::CycleBudgetExceeded { address, .. } => ("CycleBudgetExceeded", address),
    };
    let value = error(code, &err.to_string());
    js_sys::Reflect::set(&value, &"address".into(), &address.into()).expect("fresh error");
    let (key, data) = match err {
        CpuError::UnsupportedOpcode { opcode, .. } => ("opcode", JsValue::from(opcode)),
        CpuError::CycleBudgetExceeded { budget, .. } => ("budget", JsValue::from(budget)),
    };
    js_sys::Reflect::set(&value, &key.into(), &data).expect("fresh error");
    value
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** All snapshots are detached JS data, not restorable savestates. */
export interface RegistersSnapshot {
    readonly a: number; readonly x: number; readonly y: number;
    readonly sp: number; readonly pc: number; readonly status: number;
}
export type StepKind = 'instruction' | 'irq' | 'nmi' | 'reset';
export interface StepSnapshot {
    readonly address: number; readonly kind: StepKind; readonly opcode: number | null;
    readonly before: RegistersSnapshot; readonly after: RegistersSnapshot; readonly cycles: bigint;
}
export interface CycleSnapshot {
    readonly bus: { readonly address: number; readonly data: number;
        readonly direction: 'read' | 'write'; readonly sync: boolean };
    readonly stalled: boolean; readonly completed: StepSnapshot | null;
}
export interface DebugSnapshot {
    readonly registers: RegistersSnapshot; readonly nextClockPhase: 'Phi1' | 'Phi2';
    readonly execution: { readonly kind: StepKind; readonly opcode: number | null;
        readonly instructionAddress: number; readonly phase: string; readonly cycles: bigint;
        readonly baseAddress: number; readonly effectiveAddress: number;
        readonly lowByte: number; readonly data: number; readonly stackAddress: number } | null;
    readonly fetchWaitCycles: bigint;
    readonly pins: { readonly irq: boolean; readonly nmi: boolean; readonly reset: boolean;
        readonly ready: boolean; readonly so: boolean };
    readonly latches: { readonly irqSample: boolean; readonly irqPending: boolean;
        readonly nmiSample: boolean; readonly nmiEdge: boolean; readonly nmiPending: boolean;
        readonly resetSample: boolean; readonly resetStop: boolean; readonly resetActive: boolean;
        readonly soSample: boolean; readonly soPending: boolean };
    readonly pendingVWrites: readonly [boolean | null, boolean | null];
}
export interface HesperError extends Error {
    readonly code: string; readonly address?: number; readonly opcode?: number; readonly budget?: bigint;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "RegistersSnapshot")]
    pub type RegistersValue;
    #[wasm_bindgen(typescript_type = "StepSnapshot")]
    pub type StepValue;
    #[wasm_bindgen(typescript_type = "CycleSnapshot")]
    pub type CycleValue;
    #[wasm_bindgen(typescript_type = "DebugSnapshot")]
    pub type DebugValue;
}

pub fn registers(r: Registers) -> RegistersValue {
    object([
        ("a", r.a.into()),
        ("x", r.x.into()),
        ("y", r.y.into()),
        ("sp", r.sp.into()),
        ("pc", r.pc.into()),
        ("status", r.status.bits().into()),
    ])
    .unchecked_into()
}

fn kind(kind: StepKind) -> (&'static str, JsValue) {
    match kind {
        StepKind::Instruction { opcode } => ("instruction", opcode.into()),
        StepKind::Irq => ("irq", JsValue::NULL),
        StepKind::Nmi => ("nmi", JsValue::NULL),
        StepKind::Reset => ("reset", JsValue::NULL),
    }
}

pub fn step(s: Step) -> StepValue {
    let (kind, opcode) = kind(s.kind);
    object([
        ("address", s.address.into()),
        ("kind", kind.into()),
        ("opcode", opcode),
        ("before", registers(s.before).into()),
        ("after", registers(s.after).into()),
        ("cycles", s.cycles.into()),
    ])
    .unchecked_into()
}

pub fn cycle(c: Cycle) -> CycleValue {
    object([
        (
            "bus",
            object([
                ("address", c.bus.address.into()),
                ("data", c.bus.data.into()),
                (
                    "direction",
                    match c.bus.direction {
                        Direction::Read => "read",
                        Direction::Write => "write",
                    }
                    .into(),
                ),
                ("sync", c.bus.sync.into()),
            ]),
        ),
        ("stalled", c.stalled.into()),
        (
            "completed",
            c.completed.map_or(JsValue::NULL, |s| step(s).into()),
        ),
    ])
    .unchecked_into()
}

pub fn debug(d: DebugState) -> DebugValue {
    let execution = d.execution.map_or(JsValue::NULL, |e| {
        let (kind, opcode) = kind(e.kind);
        object([
            ("kind", kind.into()),
            ("opcode", opcode),
            ("instructionAddress", e.instruction_address.into()),
            ("phase", format!("{:?}", e.phase).into()),
            ("cycles", e.cycles.into()),
            ("baseAddress", e.base_address.into()),
            ("effectiveAddress", e.effective_address.into()),
            ("lowByte", e.low_byte.into()),
            ("data", e.data.into()),
            ("stackAddress", e.stack_address.into()),
        ])
    });
    let writes = js_sys::Array::new();
    for pending in d.pending_v_writes {
        writes.push(&pending.map_or(JsValue::NULL, JsValue::from));
    }
    object([
        ("registers", registers(d.registers).into()),
        ("nextClockPhase", format!("{:?}", d.next_clock_phase).into()),
        ("execution", execution),
        ("fetchWaitCycles", d.fetch_wait_cycles.into()),
        (
            "pins",
            object([
                ("irq", d.pins.irq.into()),
                ("nmi", d.pins.nmi.into()),
                ("reset", d.pins.reset.into()),
                ("ready", d.pins.ready.into()),
                ("so", d.pins.so.into()),
            ]),
        ),
        (
            "latches",
            object([
                ("irqSample", d.latches.irq_sample.into()),
                ("irqPending", d.latches.irq_pending.into()),
                ("nmiSample", d.latches.nmi_sample.into()),
                ("nmiEdge", d.latches.nmi_edge.into()),
                ("nmiPending", d.latches.nmi_pending.into()),
                ("resetSample", d.latches.reset_sample.into()),
                ("resetStop", d.latches.reset_stop.into()),
                ("resetActive", d.latches.reset_active.into()),
                ("soSample", d.latches.so_sample.into()),
                ("soPending", d.latches.so_pending.into()),
            ]),
        ),
        ("pendingVWrites", writes.into()),
    ])
    .unchecked_into()
}
