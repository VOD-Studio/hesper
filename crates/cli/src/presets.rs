//! Apple-1 program images bundled with the host, independent of the CPU core.
//!
//! The whole collection published by <https://apple1software.com/> (The
//! Apple-1 Software Library, © Relate.IT) was downloaded on 2026-09-14 and is
//! stored here byte-for-byte as that site transfers it over Web Serial to a
//! real machine: one `.bin` per 6502 RAM block, named `<id>-<address>.bin`,
//! written at that address before boot. Nothing is patched, relocated or
//! re-assembled, and the host has no network code — `include_bytes!` is the
//! only way an image reaches the emulator.
//!
//! Every entry keeps its published category, author, year, licence and source
//! page, and the exact command that starts it after boot (see
//! [`ProgramPreset::startup`]). Per-file SHA-256, the download date and the
//! licence caveats are in `crates/cli/assets/README.md`.
//!
//! `little-tower` needs RAM at `$1000–$1FFF`, outside the fixed mapping.
//! `memory-test-1000-1fff` fits in RAM but diagnoses that absent bank.
//! [`ProgramPreset::limitation`] distinguishes these from functional evidence.
//!
//! Programs marked in that README as BASIC programs are stored together with
//! the 4096-byte Huston BASIC image at `$E000`, because the site ships BASIC
//! with them and they cannot run without it; their start command is BASIC's
//! warm entry `E2B3R`, the same one the site appends to its listing.

use std::fmt::Write as _;

/// One contiguous block of Apple-1 RAM written before boot.
///
/// Blocks are independent because the published programs are not single
/// images: the BASIC ones carry a 182-byte tape header at `$004A` next to
/// their program at `$0280`/`$0300`/`$0800`, and `TypeWriter` is three
/// separate blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramBlock {
    /// First address of the block; the loader validates the complete range.
    pub address: u16,
    /// Bytes written there, unchanged from the published listing.
    pub bytes: &'static [u8],
}

/// Where a preset sits in the published library. The four categories are the
/// site's own tabs; the labels are what the TUI picker and `--list-presets`
/// print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Games,
    Fun,
    Programming,
    Utilities,
}

impl Category {
    /// Site order, used by the TUI picker and the `--list-presets` report.
    pub const ALL: [Self; 4] = [Self::Games, Self::Fun, Self::Programming, Self::Utilities];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Games => "Games 游戏",
            Self::Fun => "Fun 娱乐",
            Self::Programming => "Programming 编程",
            Self::Utilities => "Utilities 工具",
        }
    }
}

/// A known limitation of the current fixed RAM mapping, not a test verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetLimitation {
    /// The image itself extends into unmapped RAM and cannot be loaded.
    UnmappedLoad,
    /// The diagnostic loads, but its target RAM is absent, so it reports an error.
    UnmappedTestRam,
}

/// A bundled program: where it came from, how it loads, how it starts.
#[derive(Debug, PartialEq, Eq)]
pub struct ProgramPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    /// Author as published on the program page (`unknown` where the site says
    /// so).
    pub author: &'static str,
    pub year: &'static str,
    /// Licence label exactly as published on the program page; empty when the
    /// page declares none. See [`ProgramPreset::license_label`].
    pub license: &'static str,
    /// The program page this image was downloaded from; the site, its author
    /// and its licence are the only provenance available for these binaries.
    pub source: &'static str,
    /// Address the launch form shows. For BASIC programs this is the program
    /// block, not the BASIC image shipped alongside it.
    pub load: u16,
    /// Address typed at the Woz Monitor prompt to start the program.
    pub entry: u16,
    /// One-line start instruction, shown in the launch form, the picker and
    /// the running session's notice.
    pub startup: &'static str,
    /// Known load or runtime limitation. `None` does not mean fully verified;
    /// per-program evidence is in the bundled assets' compatibility matrix.
    pub limitation: Option<PresetLimitation>,
    /// Every RAM block to write, in address order.
    pub blocks: &'static [ProgramBlock],
}

impl ProgramPreset {
    /// Shared CLI/TUI wording keeps loadability separate from functional proof.
    pub fn compatibility_note(&self) -> &'static str {
        match self.limitation {
            Some(PresetLimitation::UnmappedLoad) => "不可加载：$1000–$1FFF 无 RAM",
            Some(PresetLimitation::UnmappedTestRam) => "可加载：目标无 RAM，预期报错",
            None => "功能未完整验收；详见兼容性矩阵",
        }
    }

    /// Total bytes written, across all blocks.
    pub fn size(&self) -> usize {
        self.blocks.iter().map(|block| block.bytes.len()).sum()
    }

    /// Human-readable `$004A–$00FF, $0300–$0FFF` list of the blocks.
    pub fn ranges(&self) -> String {
        let mut text = String::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            let end = block.address + block.bytes.len() as u16 - 1;
            let _ = write!(text, "${:04X}–${end:04X}", block.address);
        }
        text
    }

    pub fn license_label(&self) -> &'static str {
        if self.license.is_empty() {
            "站点未声明许可证"
        } else {
            self.license
        }
    }

    pub fn find(id: &str) -> Option<&'static Self> {
        APPLE1_PRESETS.iter().find(|preset| preset.id == id)
    }

    /// The presets of one category, in catalogue order.
    pub fn in_category(category: Category) -> impl Iterator<Item = &'static ProgramPreset> {
        APPLE1_PRESETS
            .iter()
            .filter(move |preset| preset.category == category)
    }
}

/// `include_bytes!` needs a literal path, so the helper keeps the table below
/// to one line per block.
macro_rules! block {
    ($address:literal, $path:literal) => {
        ProgramBlock {
            address: $address,
            bytes: include_bytes!(concat!("../assets/programs/", $path)),
        }
    };
}

/// Every program published under the site's four categories on 2026-09-14:
/// 42 entries, in catalogue order. `basic-huston` is the same 4096-byte image
/// the collection's BASIC programs are shipped with.
pub const APPLE1_PRESETS: &[ProgramPreset] = &[
    // 15 Puzzle - Jeff Jetton, 2020 - MIT License
    // https://apple1software.com/games/15-puzzle/
    ProgramPreset {
        id: "15-puzzle",
        name: "15 Puzzle",
        category: Category::Games,
        author: "Jeff Jetton",
        year: "2020",
        license: "MIT License",
        source: "https://apple1software.com/games/15-puzzle/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/15-puzzle-0300.bin")],
    },
    // 2048 - Denis Paryshev, 2018 - no licence declared on the page
    // https://apple1software.com/games/2048/
    ProgramPreset {
        id: "2048",
        name: "2048",
        category: Category::Games,
        author: "Denis Paryshev",
        year: "2018",
        license: "",
        source: "https://apple1software.com/games/2048/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[block!(0x0280, "games/2048-0280.bin")],
    },
    // Blackjack - unknown, 1976 - no licence declared on the page
    // https://apple1software.com/games/blackjack/
    ProgramPreset {
        id: "blackjack",
        name: "Blackjack",
        category: Category::Games,
        author: "unknown",
        year: "1976",
        license: "",
        source: "https://apple1software.com/games/blackjack/",
        load: 0x0800,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "games/blackjack-004a.bin"),
            block!(0x0800, "games/blackjack-0800.bin"),
        ],
    },
    // Codebreaker - Uncle Bernie, 2021 - Custom License
    // https://apple1software.com/games/codebreaker/
    ProgramPreset {
        id: "codebreaker",
        name: "Codebreaker",
        category: Category::Games,
        author: "Uncle Bernie",
        year: "2021",
        license: "Custom License",
        source: "https://apple1software.com/games/codebreaker/",
        load: 0x0800,
        entry: 0x0800,
        startup: "启动后输入 0800R",
        limitation: None,
        blocks: &[block!(0x0800, "games/codebreaker-0800.bin")],
    },
    // Dobble - Claudio Parmigiani, 2024 - no licence declared on the page
    // https://apple1software.com/games/dobble/
    ProgramPreset {
        id: "dobble",
        name: "Dobble",
        category: Category::Games,
        author: "Claudio Parmigiani",
        year: "2024",
        license: "",
        source: "https://apple1software.com/games/dobble/",
        load: 0x0280,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "games/dobble-004a.bin"),
            block!(0x0280, "games/dobble-0280.bin"),
        ],
    },
    // Hamurabi - David H. Ahl, 1971 - no licence declared on the page
    // https://apple1software.com/games/hamurabi/
    ProgramPreset {
        id: "hamurabi",
        name: "Hamurabi",
        category: Category::Games,
        author: "David H. Ahl",
        year: "1971",
        license: "",
        source: "https://apple1software.com/games/hamurabi/",
        load: 0x0300,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "games/hamurabi-004a.bin"),
            block!(0x0300, "games/hamurabi-0300.bin"),
        ],
    },
    // Little Tower - Arnaud Verhille, 2000 - no licence declared on the page
    // https://apple1software.com/games/little-tower/
    ProgramPreset {
        id: "little-tower",
        name: "Little Tower",
        category: Category::Games,
        author: "Arnaud Verhille",
        year: "2000",
        license: "",
        source: "https://apple1software.com/games/little-tower/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: Some(PresetLimitation::UnmappedLoad),
        blocks: &[block!(0x0300, "games/little-tower-0300.bin")],
    },
    // Lunar Lander (Text Only) - Mark Garetz, 1976 - no licence declared on the page
    // https://apple1software.com/games/lunar-lander/text-only/
    ProgramPreset {
        id: "lunar-lander-text-only",
        name: "Lunar Lander (Text Only)",
        category: Category::Games,
        author: "Mark Garetz",
        year: "1976",
        license: "",
        source: "https://apple1software.com/games/lunar-lander/text-only/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/lunar-lander-text-only-0300.bin")],
    },
    // Lunar Lander (ASCII Graphics) - Corey Cohen, 2012 - no licence declared on the page
    // https://apple1software.com/games/lunar-lander/ascii-graphics/
    ProgramPreset {
        id: "lunar-lander-ascii-graphics",
        name: "Lunar Lander (ASCII Graphics)",
        category: Category::Games,
        author: "Corey Cohen",
        year: "2012",
        license: "",
        source: "https://apple1software.com/games/lunar-lander/ascii-graphics/",
        load: 0x0300,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "games/lunar-lander-ascii-graphics-004a.bin"),
            block!(0x0300, "games/lunar-lander-ascii-graphics-0300.bin"),
        ],
    },
    // Mastermind - Steve Wozniak, 1976 - no licence declared on the page
    // https://apple1software.com/games/mastermind/
    ProgramPreset {
        id: "mastermind",
        name: "Mastermind",
        category: Category::Games,
        author: "Steve Wozniak",
        year: "1976",
        license: "",
        source: "https://apple1software.com/games/mastermind/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/mastermind-0300.bin")],
    },
    // Microchess - Peter R. Jennings, 1976 - Custom License
    // https://apple1software.com/games/microchess/
    ProgramPreset {
        id: "microchess",
        name: "Microchess",
        category: Category::Games,
        author: "Peter R. Jennings",
        year: "1976",
        license: "Custom License",
        source: "https://apple1software.com/games/microchess/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/microchess-0300.bin")],
    },
    // Mini-Startrek - Robert J. Bishop, 1977 - no licence declared on the page
    // https://apple1software.com/games/mini-startrek/
    ProgramPreset {
        id: "mini-startrek",
        name: "Mini-Startrek",
        category: Category::Games,
        author: "Robert J. Bishop",
        year: "1977",
        license: "",
        source: "https://apple1software.com/games/mini-startrek/",
        load: 0x0300,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "games/mini-startrek-004a.bin"),
            block!(0x0300, "games/mini-startrek-0300.bin"),
        ],
    },
    // Peg Solitaire - Jeff Jetton, 2025 - MIT License
    // https://apple1software.com/games/peg-solitaire/
    ProgramPreset {
        id: "peg-solitaire",
        name: "Peg Solitaire",
        category: Category::Games,
        author: "Jeff Jetton",
        year: "2025",
        license: "MIT License",
        source: "https://apple1software.com/games/peg-solitaire/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/peg-solitaire-0300.bin")],
    },
    // Shut the Box - Jeff Jetton, 2020 - MIT License
    // https://apple1software.com/games/shut-the-box/
    ProgramPreset {
        id: "shut-the-box",
        name: "Shut the Box",
        category: Category::Games,
        author: "Jeff Jetton",
        year: "2020",
        license: "MIT License",
        source: "https://apple1software.com/games/shut-the-box/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/shut-the-box-0300.bin")],
    },
    // Worple! - Jeff Jetton, 2022 - MIT License
    // https://apple1software.com/games/worple/
    ProgramPreset {
        id: "worple",
        name: "Worple!",
        category: Category::Games,
        author: "Jeff Jetton",
        year: "2022",
        license: "MIT License",
        source: "https://apple1software.com/games/worple/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "games/worple-0300.bin")],
    },
    // Apple 30th Anniversary - David Schmenk, 2006 - no licence declared on the page
    // https://apple1software.com/fun/30th/
    ProgramPreset {
        id: "30th",
        name: "Apple 30th Anniversary",
        category: Category::Fun,
        author: "David Schmenk",
        year: "2006",
        license: "",
        source: "https://apple1software.com/fun/30th/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[block!(0x0280, "fun/30th-0280.bin")],
    },
    // 99 Bottles of Beer - Barry M., 2010 - no licence declared on the page
    // https://apple1software.com/fun/beer/
    ProgramPreset {
        id: "beer",
        name: "99 Bottles of Beer",
        category: Category::Fun,
        author: "Barry M.",
        year: "2010",
        license: "",
        source: "https://apple1software.com/fun/beer/",
        load: 0x0BEE,
        entry: 0x0BEE,
        startup: "启动后输入 0BEER",
        limitation: None,
        blocks: &[block!(0x0BEE, "fun/beer-0bee.bin")],
    },
    // Cat - Denis Paryshev, 2022 - no licence declared on the page
    // https://apple1software.com/fun/cat/
    ProgramPreset {
        id: "cat",
        name: "Cat",
        category: Category::Fun,
        author: "Denis Paryshev",
        year: "2022",
        license: "",
        source: "https://apple1software.com/fun/cat/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[block!(0x0280, "fun/cat-0280.bin")],
    },
    // Cellular - Ken Wesson, 2007 - no licence declared on the page
    // https://apple1software.com/fun/cellular/
    ProgramPreset {
        id: "cellular",
        name: "Cellular",
        category: Category::Fun,
        author: "Ken Wesson",
        year: "2007",
        license: "",
        source: "https://apple1software.com/fun/cellular/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "fun/cellular-0300.bin")],
    },
    // Mandelbrot 65 - Frederic Stark, 2024 - MIT License
    // https://apple1software.com/fun/mandelbrot-65/
    ProgramPreset {
        id: "mandelbrot-65",
        name: "Mandelbrot 65",
        category: Category::Fun,
        author: "Frederic Stark",
        year: "2024",
        license: "MIT License",
        source: "https://apple1software.com/fun/mandelbrot-65/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[block!(0x0280, "fun/mandelbrot-65-0280.bin")],
    },
    // Pasart - Ken Wesson, 2007 - no licence declared on the page
    // https://apple1software.com/fun/pasart/
    ProgramPreset {
        id: "pasart",
        name: "Pasart",
        category: Category::Fun,
        author: "Ken Wesson",
        year: "2007",
        license: "",
        source: "https://apple1software.com/fun/pasart/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "fun/pasart-0300.bin")],
    },
    // Twinkle Twinkle Little Star - Corey Cohen, 2012 - no licence declared on the page
    // https://apple1software.com/fun/twinkle/
    ProgramPreset {
        id: "twinkle",
        name: "Twinkle Twinkle Little Star",
        category: Category::Fun,
        author: "Corey Cohen",
        year: "2012",
        license: "",
        source: "https://apple1software.com/fun/twinkle/",
        load: 0x0800,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "fun/twinkle-004a.bin"),
            block!(0x0800, "fun/twinkle-0800.bin"),
        ],
    },
    // A1-Assembler - San Bergmans, 2020 - no licence declared on the page
    // https://apple1software.com/programming/a1assembler/
    ProgramPreset {
        id: "a1assembler",
        name: "A1-Assembler",
        category: Category::Programming,
        author: "San Bergmans",
        year: "2020",
        license: "",
        source: "https://apple1software.com/programming/a1assembler/",
        load: 0xE000,
        entry: 0xE000,
        startup: "启动后输入 E000R",
        limitation: None,
        blocks: &[block!(0xE000, "programming/a1assembler-e000.bin")],
    },
    // ASCII HEX (Keyboard) - Arthur L. Schawlow, 1978 - no licence declared on the page
    // https://apple1software.com/programming/ascii-hex/keyboard/
    ProgramPreset {
        id: "ascii-hex-keyboard",
        name: "ASCII HEX (Keyboard)",
        category: Category::Programming,
        author: "Arthur L. Schawlow",
        year: "1978",
        license: "",
        source: "https://apple1software.com/programming/ascii-hex/keyboard/",
        load: 0x0700,
        entry: 0x0700,
        startup: "启动后输入 0700R",
        limitation: None,
        blocks: &[block!(0x0700, "programming/ascii-hex-keyboard-0700.bin")],
    },
    // ASCII HEX (Printing) - Arthur L. Schawlow, 1978 - no licence declared on the page
    // https://apple1software.com/programming/ascii-hex/printing/
    ProgramPreset {
        id: "ascii-hex-printing",
        name: "ASCII HEX (Printing)",
        category: Category::Programming,
        author: "Arthur L. Schawlow",
        year: "1978",
        license: "",
        source: "https://apple1software.com/programming/ascii-hex/printing/",
        load: 0x0750,
        entry: 0x0750,
        startup: "启动后输入 0750R",
        limitation: None,
        blocks: &[block!(0x0750, "programming/ascii-hex-printing-0750.bin")],
    },
    // Apple BASIC (C) - Steve Wozniak, 1976 - no licence declared on the page
    // https://apple1software.com/programming/basic/c/
    ProgramPreset {
        id: "basic-c",
        name: "Apple BASIC (C)",
        category: Category::Programming,
        author: "Steve Wozniak",
        year: "1976",
        license: "",
        source: "https://apple1software.com/programming/basic/c/",
        load: 0xE000,
        entry: 0xE000,
        startup: "启动后输入 E000R",
        limitation: None,
        blocks: &[block!(0xE000, "programming/basic-c-e000.bin")],
    },
    // Apple BASIC (D) - Steve Wozniak, 1976 - no licence declared on the page
    // https://apple1software.com/programming/basic/d/
    ProgramPreset {
        id: "basic-d",
        name: "Apple BASIC (D)",
        category: Category::Programming,
        author: "Steve Wozniak",
        year: "1976",
        license: "",
        source: "https://apple1software.com/programming/basic/d/",
        load: 0xE000,
        entry: 0xE000,
        startup: "启动后输入 E000R",
        limitation: None,
        blocks: &[block!(0xE000, "programming/basic-d-e000.bin")],
    },
    // Apple BASIC (Huston) - Steve Wozniak, 1977 - no licence declared on the page
    // https://apple1software.com/programming/basic/huston/
    ProgramPreset {
        id: "basic-huston",
        name: "Apple BASIC (Huston)",
        category: Category::Programming,
        author: "Steve Wozniak",
        year: "1977",
        license: "",
        source: "https://apple1software.com/programming/basic/huston/",
        load: 0xE000,
        entry: 0xE000,
        startup: "启动后输入 E000R",
        limitation: None,
        blocks: &[block!(0xE000, "programming/basic-huston-e000.bin")],
    },
    // Apple BASIC (Pagetable) - Steve Wozniak, 1976 - no licence declared on the page
    // https://apple1software.com/programming/basic/pagetable/
    ProgramPreset {
        id: "basic-pagetable",
        name: "Apple BASIC (Pagetable)",
        category: Category::Programming,
        author: "Steve Wozniak",
        year: "1976",
        license: "",
        source: "https://apple1software.com/programming/basic/pagetable/",
        load: 0xE000,
        entry: 0xE000,
        startup: "启动后输入 E000R",
        limitation: None,
        blocks: &[block!(0xE000, "programming/basic-pagetable-e000.bin")],
    },
    // Dis-Assembler - Steve Wozniak, Allen Baum, 1976 - no licence declared on the page
    // https://apple1software.com/programming/dis-assembler/
    ProgramPreset {
        id: "dis-assembler",
        name: "Dis-Assembler",
        category: Category::Programming,
        author: "Steve Wozniak, Allen Baum",
        year: "1976",
        license: "",
        source: "https://apple1software.com/programming/dis-assembler/",
        load: 0x0800,
        entry: 0x0800,
        startup: "启动后输入 0800R",
        limitation: None,
        blocks: &[block!(0x0800, "programming/dis-assembler-0800.bin")],
    },
    // Hellorld! - Bobby Nijssen, 2024 - no licence declared on the page
    // https://apple1software.com/programming/hellorld/
    ProgramPreset {
        id: "hellorld",
        name: "Hellorld!",
        category: Category::Programming,
        author: "Bobby Nijssen",
        year: "2024",
        license: "",
        source: "https://apple1software.com/programming/hellorld/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[block!(0x0300, "programming/hellorld-0300.bin")],
    },
    // Stringout (Espinosa) - Chris Espinosa, 1976 - no licence declared on the page
    // https://apple1software.com/programming/stringout/espinosa/
    ProgramPreset {
        id: "stringout-espinosa",
        name: "Stringout (Espinosa)",
        category: Category::Programming,
        author: "Chris Espinosa",
        year: "1976",
        license: "",
        source: "https://apple1software.com/programming/stringout/espinosa/",
        load: 0x0400,
        entry: 0x0400,
        startup: "启动后输入 0400R",
        limitation: None,
        blocks: &[block!(0x0400, "programming/stringout-espinosa-0400.bin")],
    },
    // Stringout (Meier) - Chris Espinosa, M. Meier, 1976 - no licence declared on the page
    // https://apple1software.com/programming/stringout/meier/
    ProgramPreset {
        id: "stringout-meier",
        name: "Stringout (Meier)",
        category: Category::Programming,
        author: "Chris Espinosa, M. Meier",
        year: "1976",
        license: "",
        source: "https://apple1software.com/programming/stringout/meier/",
        load: 0x0400,
        entry: 0x0400,
        startup: "启动后输入 0400R",
        limitation: None,
        blocks: &[block!(0x0400, "programming/stringout-meier-0400.bin")],
    },
    // Memory Test (0009-027F △) - Mike Willegal, 2021 - no licence declared on the page
    // https://apple1software.com/utilities/memory-test/0009-027f/
    ProgramPreset {
        id: "memory-test-0009-027f",
        name: "Memory Test (0009-027F △)",
        category: Category::Utilities,
        author: "Mike Willegal",
        year: "2021",
        license: "",
        source: "https://apple1software.com/utilities/memory-test/0009-027f/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[
            block!(0x0000, "utilities/memory-test-0009-027f-0000.bin"),
            block!(0x0280, "utilities/memory-test-0009-027f-0280.bin"),
        ],
    },
    // Memory Test (03A2-0FFF △) - Mike Willegal, 2021 - no licence declared on the page
    // https://apple1software.com/utilities/memory-test/03a2-0fff/
    ProgramPreset {
        id: "memory-test-03a2-0fff",
        name: "Memory Test (03A2-0FFF △)",
        category: Category::Utilities,
        author: "Mike Willegal",
        year: "2021",
        license: "",
        source: "https://apple1software.com/utilities/memory-test/03a2-0fff/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[
            block!(0x0000, "utilities/memory-test-03a2-0fff-0000.bin"),
            block!(0x0280, "utilities/memory-test-03a2-0fff-0280.bin"),
        ],
    },
    // Memory Test (1000-1FFF ▽) - Mike Willegal, 2021 - no licence declared on the page
    // https://apple1software.com/utilities/memory-test/1000-1fff/
    ProgramPreset {
        id: "memory-test-1000-1fff",
        name: "Memory Test (1000-1FFF ▽)",
        category: Category::Utilities,
        author: "Mike Willegal",
        year: "2021",
        license: "",
        source: "https://apple1software.com/utilities/memory-test/1000-1fff/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: Some(PresetLimitation::UnmappedTestRam),
        blocks: &[
            block!(0x0000, "utilities/memory-test-1000-1fff-0000.bin"),
            block!(0x0280, "utilities/memory-test-1000-1fff-0280.bin"),
        ],
    },
    // Memory Test (E000-EFFF ▽) - Mike Willegal, 2021 - no licence declared on the page
    // https://apple1software.com/utilities/memory-test/e000-efff/
    ProgramPreset {
        id: "memory-test-e000-efff",
        name: "Memory Test (E000-EFFF ▽)",
        category: Category::Utilities,
        author: "Mike Willegal",
        year: "2021",
        license: "",
        source: "https://apple1software.com/utilities/memory-test/e000-efff/",
        load: 0x0280,
        entry: 0x0280,
        startup: "启动后输入 0280R",
        limitation: None,
        blocks: &[
            block!(0x0000, "utilities/memory-test-e000-efff-0000.bin"),
            block!(0x0280, "utilities/memory-test-e000-efff-0280.bin"),
        ],
    },
    // Party Checkin - Erik Bruchez, 2024 - MIT License
    // https://apple1software.com/utilities/party/
    ProgramPreset {
        id: "party",
        name: "Party Checkin",
        category: Category::Utilities,
        author: "Erik Bruchez",
        year: "2024",
        license: "MIT License",
        source: "https://apple1software.com/utilities/party/",
        load: 0xE000,
        entry: 0xE000,
        startup: "启动后输入 E000R",
        limitation: None,
        blocks: &[block!(0xE000, "utilities/party-e000.bin")],
    },
    // Resistor Calculator - Paolo Di Leo, 2007 - no licence declared on the page
    // https://apple1software.com/utilities/resistor-calculator/
    ProgramPreset {
        id: "resistor-calculator",
        name: "Resistor Calculator",
        category: Category::Utilities,
        author: "Paolo Di Leo",
        year: "2007",
        license: "",
        source: "https://apple1software.com/utilities/resistor-calculator/",
        load: 0x0800,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "utilities/resistor-calculator-004a.bin"),
            block!(0x0800, "utilities/resistor-calculator-0800.bin"),
        ],
    },
    // Stopwatch - Larry Nelson, Bob Huelsdonk, Val Golding, 1978 - no licence declared on the page
    // https://apple1software.com/utilities/stopwatch/
    ProgramPreset {
        id: "stopwatch",
        name: "Stopwatch",
        category: Category::Utilities,
        author: "Larry Nelson, Bob Huelsdonk, Val Golding",
        year: "1978",
        license: "",
        source: "https://apple1software.com/utilities/stopwatch/",
        load: 0x0800,
        entry: 0xE2B3,
        startup: "启动后输入 E2B3R 进入 BASIC，再输入 RUN",
        limitation: None,
        blocks: &[
            // The site transfers Huston BASIC (4 KiB @ $E000) with these
            // BASIC programs; `load` above is the program block, not this one.
            block!(0xE000, "programming/basic-huston-e000.bin"),
            block!(0x004A, "utilities/stopwatch-004a.bin"),
            block!(0x0800, "utilities/stopwatch-0800.bin"),
        ],
    },
    // Test Program - Steve Wozniak, 1976 - no licence declared on the page
    // https://apple1software.com/utilities/test-program/
    ProgramPreset {
        id: "test-program",
        name: "Test Program",
        category: Category::Utilities,
        author: "Steve Wozniak",
        year: "1976",
        license: "",
        source: "https://apple1software.com/utilities/test-program/",
        load: 0x0000,
        entry: 0x0000,
        startup: "启动后输入 0000R",
        limitation: None,
        blocks: &[block!(0x0000, "utilities/test-program-0000.bin")],
    },
    // TypeWriter - Landon J. Smith, 2025 - no licence declared on the page
    // https://apple1software.com/utilities/typewriter/
    ProgramPreset {
        id: "typewriter",
        name: "TypeWriter",
        category: Category::Utilities,
        author: "Landon J. Smith",
        year: "2025",
        license: "",
        source: "https://apple1software.com/utilities/typewriter/",
        load: 0x0300,
        entry: 0x0300,
        startup: "启动后输入 0300R",
        limitation: None,
        blocks: &[
            block!(0x0300, "utilities/typewriter-0300.bin"),
            block!(0x0400, "utilities/typewriter-0400.bin"),
            block!(0x0440, "utilities/typewriter-0440.bin"),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use hesper_apple1::Apple1Bus;
    use sha2::{Digest, Sha256};

    /// The whole published library is embedded: one entry per program page.
    /// The count is a provenance check, not a UI limit — adding a preset
    /// means updating it.
    #[test]
    fn every_published_program_is_embedded_once_per_category() {
        assert_eq!(APPLE1_PRESETS.len(), 42);
        assert_eq!(ProgramPreset::in_category(Category::Games).count(), 15);
        assert_eq!(ProgramPreset::in_category(Category::Fun).count(), 7);
        assert_eq!(
            ProgramPreset::in_category(Category::Programming).count(),
            11
        );
        assert_eq!(ProgramPreset::in_category(Category::Utilities).count(), 9);
    }

    /// A preset that cannot be loaded or cannot be started is worse than no
    /// preset: every block must fit one RAM bank, every entry must land inside
    /// a block it will actually write, and ids must be addressable from
    /// `--preset`.
    #[test]
    fn every_preset_loads_into_ram_and_starts_inside_its_own_image() {
        let mut ids = std::collections::HashSet::new();
        for preset in APPLE1_PRESETS {
            assert!(ids.insert(preset.id), "duplicate preset id: {}", preset.id);
            assert!(!preset.blocks.is_empty(), "{} has no blocks", preset.id);
            assert!(preset.size() > 0, "{} is empty", preset.id);
            assert!(
                !preset.startup.is_empty(),
                "{} has no start command",
                preset.id
            );
            let mut entry_in_block = false;
            let mut fits_a_bank = true;
            for block in preset.blocks {
                if Apple1Bus::validate_ram_load(block.address, block.bytes.len()).is_err() {
                    fits_a_bank = false;
                }
                let end = block.address + block.bytes.len() as u16;
                if (block.address..end).contains(&preset.entry) {
                    entry_in_block = true;
                }
            }
            assert!(
                entry_in_block,
                "{}: entry ${:04X} is outside {}",
                preset.id,
                preset.entry,
                preset.ranges()
            );
            let load_in_block = preset.blocks.iter().any(|block| {
                (block.address..block.address + block.bytes.len() as u16).contains(&preset.load)
            });
            assert!(load_in_block, "{}: load address is not written", preset.id);
            assert_eq!(
                matches!(preset.limitation, Some(PresetLimitation::UnmappedLoad)),
                !fits_a_bank,
                "{}: load limitation must match what the banks accept",
                preset.id
            );
        }
    }

    /// The BASIC image the collection ships with its BASIC programs, and the
    /// one bundled here since the first preset, are the same bytes.
    #[test]
    fn bundled_basic_matches_the_published_huston_image() {
        let preset = ProgramPreset::find("basic-huston").expect("basic-huston preset");
        assert_eq!(preset.blocks.len(), 1);
        let basic = preset.blocks[0];
        assert_eq!(basic.address, 0xE000);
        assert_eq!(basic.bytes.len(), 4096);
        assert_eq!(
            format!("{:x}", Sha256::digest(basic.bytes)),
            "311c85f22996e655ae3a0881e0841a547c52f5ec20cd810035ec91ce13a27cbe"
        );
    }
}
