//! Export the existing host assets for examples/web without duplicating its catalogue.
use hesper::{DEMO_DONE, DEMO_PROGRAM, DEMO_START, presets::APPLE1_PRESETS};
use serde::Serialize;

#[derive(Serialize)]
struct Block {
    address: u16,
    bytes: &'static [u8],
}

#[derive(Serialize)]
struct Preset {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    author: &'static str,
    year: &'static str,
    license: &'static str,
    source: &'static str,
    startup: &'static str,
    entry: u16,
    load: u16,
    note: &'static str,
    expanded_note: &'static str,
    blocks: Vec<Block>,
}

#[derive(Serialize)]
struct Assets {
    demo_start: u16,
    demo_done: u16,
    demo: &'static [u8],
    presets: Vec<Preset>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = Assets {
        demo_start: DEMO_START,
        demo_done: DEMO_DONE,
        demo: DEMO_PROGRAM,
        presets: APPLE1_PRESETS
            .iter()
            .map(|p| Preset {
                id: p.id,
                name: p.name,
                category: p.category.label(),
                author: p.author,
                year: p.year,
                license: p.license_label(),
                source: p.source,
                startup: p.startup,
                entry: p.entry,
                load: p.load,
                note: p.compatibility_note_for(false),
                expanded_note: p.compatibility_note_for(true),
                blocks: p
                    .blocks
                    .iter()
                    .map(|b| Block {
                        address: b.address,
                        bytes: b.bytes,
                    })
                    .collect(),
            })
            .collect(),
    };
    print!("{}", toml::to_string(&assets)?);
    Ok(())
}
