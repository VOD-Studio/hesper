//! Bounded browser and Node host interface to the single-hart Linux board.

use core::mem::MaybeUninit;
use hesper_riscv_linux::{
    BootError, BootImages, DRAM_BASE, DRAM_SIZE, FDT_ADDRESS, INITRAMFS_ADDRESS, KERNEL_ADDRESS,
    LoadError, Machine, dram_range,
};
use hesper_wasm_support as js;
use js_sys::{BigInt, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

const PROBE_LIMIT: u64 = 4096;
const SERIAL_LIMIT: u32 = 4096;

/// Owns a single machine. Guest images are provided and authenticated by the host.
#[wasm_bindgen]
pub struct RiscvLinux {
    machine: Machine,
}

#[wasm_bindgen]
impl RiscvLinux {
    /// Board boot with verified host-supplied image bytes and OpenSBI resident end.
    pub fn boot(
        firmware: &Uint8Array,
        firmware_end: BigInt,
        kernel: &Uint8Array,
        initramfs: &Uint8Array,
        fdt: &Uint8Array,
    ) -> Result<RiscvLinux, JsValue> {
        let firmware_end = address(firmware_end, "firmware_end")?;
        if !(DRAM_BASE + 1..=KERNEL_ADDRESS).contains(&firmware_end) {
            return Err(js::error(
                "InvalidBootImage",
                "OpenSBI resident end must fit before the Linux Image",
            ));
        }
        // Check every raw length before allocating or copying any image bytes.
        // Machine::boot additionally validates parsed extents and image contents.
        let firmware_len = image_len(firmware, "OpenSBI", 1, firmware_end - DRAM_BASE)?;
        let kernel_len = image_len(
            kernel,
            "Linux Image",
            64,
            INITRAMFS_ADDRESS - KERNEL_ADDRESS,
        )?;
        let initramfs_len = image_len(initramfs, "initramfs", 6, FDT_ADDRESS - INITRAMFS_ADDRESS)?;
        let fdt_len = image_len(
            fdt,
            "DTB",
            40,
            DRAM_BASE + DRAM_SIZE as u64 - FDT_ADDRESS - BootImages::FDT_GROWTH,
        )?;

        let [mut fw_buf, mut kernel_buf, mut initrd_buf, mut fdt_buf] = [const { Vec::new() }; 4];
        let machine = Machine::boot(BootImages {
            firmware: stage(firmware, firmware_len, "OpenSBI", &mut fw_buf)?,
            firmware_end,
            kernel: stage(kernel, kernel_len, "Linux Image", &mut kernel_buf)?,
            initramfs: stage(initramfs, initramfs_len, "initramfs", &mut initrd_buf)?,
            fdt: stage(fdt, fdt_len, "DTB", &mut fdt_buf)?,
        })
        .map_err(boot_error)?;
        Ok(Self { machine })
    }

    /// Load a small bare-metal probe. The hart starts at the normal DRAM reset vector.
    pub fn bare(address: BigInt, program: &Uint8Array) -> Result<RiscvLinux, JsValue> {
        let address = self::address(address, "address")?;
        let len = image_len(program, "bare program", 1, PROBE_LIMIT)?;
        if dram_range(address, len).is_none() {
            return Err(load_error(LoadError {
                address,
                bytes: len,
            }));
        }
        let mut buffer = [MaybeUninit::uninit(); PROBE_LIMIT as usize];
        let program = program.copy_to_uninit(&mut buffer[..len]);
        let mut machine = Machine::try_new().map_err(boot_error)?;
        machine.load(address, program).map_err(load_error)?;
        Ok(Self { machine })
    }

    /// Run at most one million transitions synchronously; return the fuel spent.
    pub fn run(&mut self, fuel: f64) -> Result<u32, JsValue> {
        Ok(self
            .machine
            .run(js::integer(fuel, 0, js::MAX_BATCH, "fuel")?)
            .spent)
    }

    /// Queue the accepted prefix of at most 4096 raw serial bytes.
    pub fn send(&mut self, bytes: &Uint8Array) -> Result<u32, JsValue> {
        let length = byte_length(bytes)?;
        let len = length.min(f64::from(SERIAL_LIMIT)) as u32;
        if len == 0 {
            return Ok(0);
        }
        let prefix = (length > f64::from(len)).then(|| bytes.subarray(0, len));
        let mut buffer = [MaybeUninit::uninit(); SERIAL_LIMIT as usize];
        let bytes = prefix
            .as_ref()
            .unwrap_or(bytes)
            .copy_to_uninit(&mut buffer[..len as usize]);
        Ok(self.machine.serial_input(bytes) as u32)
    }

    /// Return a detached copy of at most 4096 completed serial bytes.
    pub fn receive(&mut self, limit: f64) -> Result<Uint8Array, JsValue> {
        let len = js::integer(limit, 0, u32::MAX, "limit")?.min(SERIAL_LIMIT) as usize;
        let mut buffer = [0; SERIAL_LIMIT as usize];
        let read = self.machine.serial_output(&mut buffer[..len]);
        Ok(Uint8Array::from(&buffer[..read]))
    }

    /// Side-effect-free CPU, trap, device, and queue diagnostics, not a savestate.
    pub fn snapshot(&self) -> String {
        format!("{:#x?}", self.machine.diagnostics())
    }

    pub fn register(&self, index: f64) -> Result<u64, JsValue> {
        let index = js::integer(index, 0, 31, "register index")? as usize;
        Ok(self.machine.debug_state().x[index])
    }

    /// Read exactly one little-endian physical DRAM word without touching devices.
    pub fn word(&self, address: BigInt) -> Result<u32, JsValue> {
        let address = self::address(address, "address")?;
        let bytes = self
            .machine
            .inspect(address, 4)
            .ok_or_else(|| load_error(LoadError { address, bytes: 4 }))?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("four-byte DRAM probe"),
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn pc(&self) -> u64 {
        self.machine.debug_state().pc
    }

    #[wasm_bindgen(getter)]
    pub fn satp(&self) -> u64 {
        self.machine.debug_state().satp
    }

    #[wasm_bindgen(getter)]
    pub fn retired(&self) -> u64 {
        self.machine.debug_state().retired
    }

    #[wasm_bindgen(getter)]
    pub fn time(&self) -> u64 {
        self.machine.debug_state().time
    }

    #[wasm_bindgen(getter, js_name = stalledTraps)]
    pub fn stalled_traps(&self) -> u32 {
        self.machine.diagnostics().stalled_traps
    }
}

fn address(value: BigInt, name: &str) -> Result<u64, JsValue> {
    u64::try_from(value).map_err(|_| {
        js::error(
            "InvalidArgument",
            &format!("{name} must be an unsigned 64-bit bigint"),
        )
    })
}

fn byte_length(bytes: &Uint8Array) -> Result<f64, JsValue> {
    if !bytes.is_instance_of::<Uint8Array>() {
        return Err(js::error("InvalidArgument", "expected a Uint8Array"));
    }
    // The js-sys length getter returns u32, which can wrap a larger JS length.
    Reflect::get(bytes, &JsValue::from_str("length"))?
        .as_f64()
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .ok_or_else(|| js::error("InvalidArgument", "invalid Uint8Array length"))
}

fn image_len(bytes: &Uint8Array, name: &str, min: u32, max: u64) -> Result<usize, JsValue> {
    let len = byte_length(bytes)?;
    if len < f64::from(min) || len > max as f64 {
        return Err(js::error(
            "InvalidBootImage",
            &format!("{name} length must be in {min}..={max} bytes"),
        ));
    }
    Ok(len as usize)
}

fn stage<'a>(
    bytes: &Uint8Array,
    len: usize,
    name: &str,
    buffer: &'a mut Vec<u8>,
) -> Result<&'a [u8], JsValue> {
    buffer.try_reserve_exact(len).map_err(|_| {
        js::error(
            "AllocationFailed",
            &format!("could not stage {len} bytes for {name}"),
        )
    })?;
    Ok(bytes.copy_to_uninit(&mut buffer.spare_capacity_mut()[..len]))
}

fn boot_error(error: BootError) -> JsValue {
    let code = match error {
        BootError::Allocation => "AllocationFailed",
        _ => "InvalidBootImage",
    };
    js::error(code, &error.to_string())
}

fn load_error(error: LoadError) -> JsValue {
    js::error("InvalidLoad", &error.to_string())
}
