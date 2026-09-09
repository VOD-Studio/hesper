//! Bounded host diagnostics shared by external runners; never linked into CPU.
use std::collections::VecDeque;

use hesper_cpu6502::{Cpu, Cycle, DebugState};

pub struct Trace {
    history: VecDeque<(u64, Cycle, DebugState)>,
    count: u64,
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            history: VecDeque::with_capacity(32),
            count: 0,
        }
    }
}

impl Trace {
    pub fn record(&mut self, cycle: Cycle, cpu: &Cpu) {
        self.count += 1;
        if self.history.len() == 32 {
            self.history.pop_front();
        }
        self.history
            .push_back((self.count, cycle, cpu.debug_state()));
    }

    pub fn failure(&self, reason: &str, cpu: &Cpu) -> String {
        let mut message = format!(
            "{reason}\ncurrent: {:?}\nrecent bus cycles (last {} of {}):\n",
            cpu.debug_state(),
            self.history.len(),
            self.count
        );
        for (number, cycle, state) in &self.history {
            message.push_str(&format!(
                "#{number} {:?} ${:04X}={:02X} SYNC={} stalled={} next={:?} pins={:?} latches={:?}\n",
                cycle.bus.direction, cycle.bus.address, cycle.bus.data, cycle.bus.sync,
                cycle.stalled, state.execution.map(|e| e.phase), state.pins, state.latches
            ));
            if let Some(step) = cycle.completed {
                message.push_str(&format!(
                    "  ${:04X} {:?} {:?} -> {:?} +{} cycles\n",
                    step.address, step.kind, step.before, step.after, step.cycles
                ));
            }
        }
        message
    }
}
