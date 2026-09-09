// Host-only reference driver. Loads unmodified upstream code from a hashed cache.
// No reference simulator/netlist is linked into or copied into the Rust CPU.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const root = path.resolve(__dirname, '..');
const fixtures = path.join(root, 'crates/cpu6502/tests/data/visual6502');
const manifest = JSON.parse(fs.readFileSync(path.join(fixtures, 'manifest.json')));
const cache = path.join(root, '.cache/cpu6502/visual6502', manifest.revision);
const sources = Object.entries(manifest.files).map(([name, hash]) => {
    const bytes = fs.readFileSync(path.join(cache, name));
    if (crypto.createHash('sha256').update(bytes).digest('hex') !== hash) throw Error(`${name}: hash mismatch`);
    return [name, bytes.toString()];
});

function reference(spec) {
    const c = vm.createContext({console});
    for (const name of ['nodenames.js', 'segdefs.js', 'transdefs.js', 'wires.js', 'chipsim.js', 'macros.js']) {
        vm.runInContext(sources.find(([file]) => file === name)[1], c, {filename: name});
    }
    // Disable presentation only, then call the upstream circuit setup and reset.
    vm.runInContext('refresh=function(){};chipStatus=function(){};setCellValue=function(){};setupNodes();setupTransistors();', c);
    c.memory = Array(65536).fill(0xea);
    const start = spec.pc ?? 0x8000;
    const p = spec.p ?? 0x20;
    const boot = [0xa2,0xfd,0x9a,0xa2,0,0xa0,0,0xa9,p,0x48,0xa9,0,0x28,0x4c,start&255,start>>8];
    boot.forEach((value, i) => c.memory[0x200+i] = value);
    spec.program.forEach((value, i) => c.memory[start+i] = value);
    [0,0xa0,0,2,0,0x90].forEach((value, i) => c.memory[0xfffa+i] = value);
    c.memory[0x9000] = c.memory[0xa000] = 0x40;
    c.initChip();
    c.setHigh('so');
    let ready = false;
    for (let half = 0; half < 200; half++) {
        if (!c.readBit('clk0') && c.readBit('sync') && c.readAddressBus() === start) { ready = true; break; }
        c.halfStep();
    }
    if (!ready) throw Error(`${spec.name}: bootstrap cycle budget exceeded`);
    for (const [address, value] of spec.ram ?? []) c.memory[address] = value;
    const initial = {pc: start, a: c.readA(), x: c.readX(), y: c.readY(), s: c.readSP(),
        p: [0,1,2,3,6,7].reduce((p, i) => p | c.readBit(`p${i}`) << i, 0x20),
        ram: c.memory.flatMap((value, address) => value === 0xea ? [] : [[address, value]])};
    if (initial.pc !== c.readPC() || initial.a || initial.x || initial.y || initial.s !== 0xfd || initial.p !== p) {
        throw Error(`${spec.name}: unexpected bootstrap registers`);
    }
    const cycles = [];
    for (let cycle = 0; cycle < 24; cycle++) {
        for (const [half, pin, asserted] of spec.events) if (half === cycle * 2) c[asserted ? 'setLow' : 'setHigh'](pin);
        c.halfStep(); // Low -> high: upstream bus writes occur here.
        cycles.push([c.readAddressBus(), c.readDataBus(), c.readBit('rw') ? 'read' : 'write', !!c.readBit('sync')]);
        for (const [half, pin, asserted] of spec.events) if (half === cycle * 2 + 1) c[asserted ? 'setLow' : 'setHigh'](pin);
        c.halfStep(); // High -> low: establish next address/read data.
    }
    return {name: spec.name, initial, events: spec.events, cycles};
}

const scenarios = [];
for (const pin of ['irq', 'nmi']) {
    for (const [family, program, pc] of [
        ['nop', [0xea,0xea,0xea], 0x8000], ['read', [0xad,0,0x20,0xea], 0x8000],
        ['rmw', [0xee,0,0x20,0xea], 0x8000], ['branch', [0xd0,0,0xea], 0x8000],
        ['branch-cross', [0xd0,1,0xea,0xea], 0x80fd],
    ]) for (let at = 0; at < 7; at++) {
        scenarios.push({name: `${family}-${pin}-${at}`, program, pc, events: [[at*2,pin,true]]});
    }
}
for (let at = 0; at < 7; at++) {
    scenarios.push({name: `brk-nmi-${at}`, program: [0,0xea,0xea], events: [[at*2,'nmi',true]]});
    scenarios.push({name: `irq-nmi-${at}`, program: [0xea,0xea], events: [[0,'irq',true],[(at+2)*2,'nmi',true]]});
}
for (const first of [0x58, 0x28]) for (const next of [0x78, 0x28, 0x40]) {
    scenarios.push({name: `i-combination-${first}-${next}`, p: 0x24, program: [first,next,0xea],
        ram: [[0x1fe,0x20],[0x1ff,0x24],[0x100,0x80],[0x101,0x80]], events: [[0,'irq',true]]});
}
for (const pin of ['irq','nmi']) for (const width of [1,2,3]) {
    scenarios.push({name: `${pin}-pulse-${width}`, program: [0xea,0xea,0xea], events: [[0,pin,true],[width*2,pin,false]]});
}

const args = process.argv.slice(2);
if (args.length && !(args.length === 2 && args[0] === '--record')) throw Error('Usage: node tools/verify_visual6502.cjs [--record OUTPUT]');
const actual = scenarios.map(reference);
const serialized = '[\n' + actual.map(c => JSON.stringify(c)).join(',\n') + '\n]\n';
if (args[0] === '--record') {
    fs.writeFileSync(args[1], serialized);
    console.log(`Recorded ${actual.length} independent revD traces to ${args[1]}`);
} else {
    const expected = fs.readFileSync(path.join(fixtures, 'pins.json'), 'utf8');
    if (serialized !== expected) throw Error('Reference traces differ from pinned fixtures; inspect differences before changing any oracle');
    console.log(`Visual6502 revD @ ${manifest.revision}: ${actual.length} pin traces reproduced`);
}
