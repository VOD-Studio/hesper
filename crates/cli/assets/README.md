# Bundled Apple-1 programs

`programs/` holds every program published by
[The Apple-1 Software Library](https://apple1software.com/) — the site's four
categories (Games, Fun, Programming, Utilities), **42 program pages and 56 RAM
blocks, 74 983 bytes in total** — downloaded once on 2026-09-14 and stored
byte-for-byte as that site hands an image to a real Apple-1 over Web Serial.

Nothing here is patched, relocated, reassembled or regenerated. Each file is
one contiguous RAM block exactly as it appears in the site's own Wozmon listing
(`GET /{category}/{slug}/wozmon?basic=false&autostart=true`, base64 inside
JSON) — the same transfer format the project documents for real hardware. The
host embeds the files with `include_bytes!`; the CLI and TUI contain no network
code and never download anything, and the Woz Monitor ROM is still supplied
externally with `--rom` and never shipped here.

The metadata for each program — id, category, author, year, licence label as
published, source page, load address, start command — lives in
`crates/cli/src/presets.rs`, in the table next to the `include_bytes!` calls.
`cargo run -p hesper -- apple1 --list-presets` prints all of it without a ROM.

## Conventions

- **One file per RAM block**, named `<preset-id>-<address>.bin` and written at
  that address before boot. Programs are not all one image: the BASIC ones
  carry a 182-byte tape header at `$004A` beside their tokenized program, and
  `typewriter` is three blocks (`$0300`, `$0400`, `$0440`).
- **Load address** shown in the launch form and `--list-presets` is the largest
  block — the program rather than a tape header or the BASIC image shipped with
  it.
- **Start command** is the last line of the site's own listing with autostart
  enabled (`xxxxR`), i.e. exactly what the site types over serial after the
  transfer. On the BASIC programs it is `E2B3R`, BASIC's warm entry, which
  re-enters BASIC without clearing the program that was just loaded; `RUN`
  then executes it.
- **BASIC programs** (8 of the 42) are stored together with the 4096-byte
  Huston BASIC image at `$E000`, because the site ships BASIC with them and
  they cannot run without it. `programming/basic/*` above are the four BASIC
  variants the site offers on their own.
- **`little-tower`** runs from `$0300` to `$14CD` in the published listing, so
  it needs the `$1000–$1FFF` expansion this machine does not model: the preset
  is listed (it is part of the library) but loading it fails with the RAM bank
  error, and the picker says why.
- **`basic-huston-e000.bin`** is byte-identical to the image the user supplied
  on 2026-09-14 (`311c85f2…`). That image is the site's Huston variant and the
  one the site transfers with every BASIC program above, so there is a single
  copy here and no second, subtly different BASIC.

## Programs

The Load column lists the blocks stored for that preset. The eight BASIC
programs are additionally installed with `programming/basic-huston-e000.bin`
at `$E000` (not repeated here).

| Preset id | Category | Program | Author, year | Load | Start | Licence as published | Source page |
|---|---|---|---|---|---|---|---|
| `15-puzzle` | Games 游戏 | 15 Puzzle | Jeff Jetton, 2020 | $0300–$06E5 | `0300R` | MIT License | <https://apple1software.com/games/15-puzzle/> |
| `2048` | Games 游戏 | 2048 | Denis Paryshev, 2018 | $0280–$0A29 | `0280R` | — | <https://apple1software.com/games/2048/> |
| `blackjack` | Games 游戏 | Blackjack (BASIC) | unknown, 1976 | $004A–$00FF, $0800–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/games/blackjack/> |
| `codebreaker` | Games 游戏 | Codebreaker | Uncle Bernie, 2021 | $0800–$0FFF | `0800R` | Custom License | <https://apple1software.com/games/codebreaker/> |
| `dobble` | Games 游戏 | Dobble (BASIC) | Claudio Parmigiani, 2024 | $004A–$00FF, $0280–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/games/dobble/> |
| `hamurabi` | Games 游戏 | Hamurabi (BASIC) | David H. Ahl, 1971 | $004A–$00FF, $0300–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/games/hamurabi/> |
| `little-tower` | Games 游戏 | Little Tower | Arnaud Verhille, 2000 | $0300–$14CD | `0300R` | — | <https://apple1software.com/games/little-tower/> |
| `lunar-lander-text-only` | Games 游戏 | Lunar Lander (Text Only) | Mark Garetz, 1976 | $0300–$09B8 | `0300R` | — | <https://apple1software.com/games/lunar-lander/text-only/> |
| `lunar-lander-ascii-graphics` | Games 游戏 | Lunar Lander (ASCII Graphics) (BASIC) | Corey Cohen, 2012 | $004A–$00FF, $0300–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/games/lunar-lander/ascii-graphics/> |
| `mastermind` | Games 游戏 | Mastermind | Steve Wozniak, 1976 | $0300–$03B0 | `0300R` | — | <https://apple1software.com/games/mastermind/> |
| `microchess` | Games 游戏 | Microchess | Peter R. Jennings, 1976 | $0300–$0BC7 | `0300R` | Custom License | <https://apple1software.com/games/microchess/> |
| `mini-startrek` | Games 游戏 | Mini-Startrek (BASIC) | Robert J. Bishop, 1977 | $004A–$00FF, $0300–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/games/mini-startrek/> |
| `peg-solitaire` | Games 游戏 | Peg Solitaire | Jeff Jetton, 2025 | $0300–$05FF | `0300R` | MIT License | <https://apple1software.com/games/peg-solitaire/> |
| `shut-the-box` | Games 游戏 | Shut the Box | Jeff Jetton, 2020 | $0300–$06FF | `0300R` | MIT License | <https://apple1software.com/games/shut-the-box/> |
| `worple` | Games 游戏 | Worple! | Jeff Jetton, 2022 | $0300–$0FFE | `0300R` | MIT License | <https://apple1software.com/games/worple/> |
| `30th` | Fun 娱乐 | Apple 30th Anniversary | David Schmenk, 2006 | $0280–$0FFF | `0280R` | — | <https://apple1software.com/fun/30th/> |
| `beer` | Fun 娱乐 | 99 Bottles of Beer | Barry M., 2010 | $0BEE–$0CB5 | `0BEER` | — | <https://apple1software.com/fun/beer/> |
| `cat` | Fun 娱乐 | Cat | Denis Paryshev, 2022 | $0280–$0546 | `0280R` | — | <https://apple1software.com/fun/cat/> |
| `cellular` | Fun 娱乐 | Cellular | Ken Wesson, 2007 | $0300–$052C | `0300R` | — | <https://apple1software.com/fun/cellular/> |
| `mandelbrot-65` | Fun 娱乐 | Mandelbrot 65 | Frederic Stark, 2024 | $0280–$07B6 | `0280R` | MIT License | <https://apple1software.com/fun/mandelbrot-65/> |
| `pasart` | Fun 娱乐 | Pasart | Ken Wesson, 2007 | $0300–$0535 | `0300R` | — | <https://apple1software.com/fun/pasart/> |
| `twinkle` | Fun 娱乐 | Twinkle Twinkle Little Star (BASIC) | Corey Cohen, 2012 | $004A–$00FF, $0800–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/fun/twinkle/> |
| `a1assembler` | Programming 编程 | A1-Assembler | San Bergmans, 2020 | $E000–$EFCD | `E000R` | — | <https://apple1software.com/programming/a1assembler/> |
| `ascii-hex-keyboard` | Programming 编程 | ASCII HEX (Keyboard) | Arthur L. Schawlow, 1978 | $0700–$071C | `0700R` | — | <https://apple1software.com/programming/ascii-hex/keyboard/> |
| `ascii-hex-printing` | Programming 编程 | ASCII HEX (Printing) | Arthur L. Schawlow, 1978 | $0750–$0776 | `0750R` | — | <https://apple1software.com/programming/ascii-hex/printing/> |
| `basic-c` | Programming 编程 | Apple BASIC (C) | Steve Wozniak, 1976 | $E000–$EFFF | `E000R` | — | <https://apple1software.com/programming/basic/c/> |
| `basic-d` | Programming 编程 | Apple BASIC (D) | Steve Wozniak, 1976 | $E000–$EFFF | `E000R` | — | <https://apple1software.com/programming/basic/d/> |
| `basic-huston` | Programming 编程 | Apple BASIC (Huston) | Steve Wozniak, 1977 | $E000–$EFFF | `E000R` | — | <https://apple1software.com/programming/basic/huston/> |
| `basic-pagetable` | Programming 编程 | Apple BASIC (Pagetable) | Steve Wozniak, 1976 | $E000–$EFFF | `E000R` | — | <https://apple1software.com/programming/basic/pagetable/> |
| `dis-assembler` | Programming 编程 | Dis-Assembler | Steve Wozniak, Allen Baum, 1976 | $0800–$09DB | `0800R` | — | <https://apple1software.com/programming/dis-assembler/> |
| `hellorld` | Programming 编程 | Hellorld! | Bobby Nijssen, 2024 | $0300–$031C | `0300R` | — | <https://apple1software.com/programming/hellorld/> |
| `stringout-espinosa` | Programming 编程 | Stringout (Espinosa) | Chris Espinosa, 1976 | $0400–$0432 | `0400R` | — | <https://apple1software.com/programming/stringout/espinosa/> |
| `stringout-meier` | Programming 编程 | Stringout (Meier) | Chris Espinosa, M. Meier, 1976 | $0400–$0429 | `0400R` | — | <https://apple1software.com/programming/stringout/meier/> |
| `memory-test-0009-027f` | Utilities 工具 | Memory Test (0009-027F △) | Mike Willegal, 2021 | $0000–$0003, $0280–$03A1 | `0280R` | — | <https://apple1software.com/utilities/memory-test/0009-027f/> |
| `memory-test-03a2-0fff` | Utilities 工具 | Memory Test (03A2-0FFF △) | Mike Willegal, 2021 | $0000–$0003, $0280–$03A1 | `0280R` | — | <https://apple1software.com/utilities/memory-test/03a2-0fff/> |
| `memory-test-1000-1fff` | Utilities 工具 | Memory Test (1000-1FFF ▽) | Mike Willegal, 2021 | $0000–$0003, $0280–$03A1 | `0280R` | — | <https://apple1software.com/utilities/memory-test/1000-1fff/> |
| `memory-test-e000-efff` | Utilities 工具 | Memory Test (E000-EFFF ▽) | Mike Willegal, 2021 | $0000–$0003, $0280–$03A1 | `0280R` | — | <https://apple1software.com/utilities/memory-test/e000-efff/> |
| `party` | Utilities 工具 | Party Checkin | Erik Bruchez, 2024 | $E000–$EE94 | `E000R` | MIT License | <https://apple1software.com/utilities/party/> |
| `resistor-calculator` | Utilities 工具 | Resistor Calculator (BASIC) | Paolo Di Leo, 2007 | $004A–$00FF, $0800–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/utilities/resistor-calculator/> |
| `stopwatch` | Utilities 工具 | Stopwatch (BASIC) | Larry Nelson, Bob Huelsdonk, Val Golding, 1978 | $004A–$00FF, $0800–$0FFF | `E2B3R 然后 RUN` | — | <https://apple1software.com/utilities/stopwatch/> |
| `test-program` | Utilities 工具 | Test Program | Steve Wozniak, 1976 | $0000–$000A | `0000R` | — | <https://apple1software.com/utilities/test-program/> |
| `typewriter` | Utilities 工具 | TypeWriter | Landon J. Smith, 2025 | $0300–$03AA, $0400–$0419, $0440–$0459 | `0300R` | — | <https://apple1software.com/utilities/typewriter/> |

## Files

Every file below is present in this directory, embedded by `include_bytes!`
and listed here with the SHA-256 of its stored bytes.

| File | Program block | Bytes | SHA-256 |
|---|---|---|---|
| `games/15-puzzle-0300.bin` | 15 Puzzle `$0300–$06E5` | 998 | `48cf03a4999e61bc11e75cad2c31125ad16e04be8e201372b69a2658d45335ce` |
| `games/2048-0280.bin` | 2048 `$0280–$0A29` | 1962 | `b8d6d04385d1f24b46807eb26abd094ce03e6c5321a95ac4e80a948210e81cc9` |
| `games/blackjack-004a.bin` | Blackjack `$004A–$00FF` | 182 | `d6162beb0e47b1a226d963fa728f5d001fa87df534dc81d57d0d07f5a20619a6` |
| `games/blackjack-0800.bin` | Blackjack `$0800–$0FFF` | 2048 | `d0bff2c90c89c685654738176608bd2df1d095a418cf05270681f9e60ab0a169` |
| `games/codebreaker-0800.bin` | Codebreaker `$0800–$0FFF` | 2048 | `579cf54db52501cb1a1e544d85c25dced5b7079a14fd67e6e4874f840e44e3b2` |
| `games/dobble-004a.bin` | Dobble `$004A–$00FF` | 182 | `570affd8d25ef47266ab22525c84b146aeeecaf5f24c472a12520c0e38f134ef` |
| `games/dobble-0280.bin` | Dobble `$0280–$0FFF` | 3456 | `e85aa270228dfa3819e234b376177ad750c370cb9f78950d885f03079227937a` |
| `games/hamurabi-004a.bin` | Hamurabi `$004A–$00FF` | 182 | `9ac6b64916df3b028b4b2fc4f43aea0c508d19f0fd5e1a34a769345a6c40a6e2` |
| `games/hamurabi-0300.bin` | Hamurabi `$0300–$0FFF` | 3328 | `1a23ce668fda30a0d9628b08aa11091ac384986523e4e04a5fc3bef695bdaca5` |
| `games/little-tower-0300.bin` | Little Tower `$0300–$14CD` | 4558 | `fcbcaa9b1c3927b121c66bf287a18aef01d90cccb2ff5a9581cf7f2655ae7d62` |
| `games/lunar-lander-text-only-0300.bin` | Lunar Lander (Text Only) `$0300–$09B8` | 1721 | `cce06d64a0d5e28d34c099827405642131e0453887b8323fd08e9d42d95bb887` |
| `games/lunar-lander-ascii-graphics-004a.bin` | Lunar Lander (ASCII Graphics) `$004A–$00FF` | 182 | `e7e3654ae138fa87af07018e3df32135835980a23418dc3af87833bb9edaa68b` |
| `games/lunar-lander-ascii-graphics-0300.bin` | Lunar Lander (ASCII Graphics) `$0300–$0FFF` | 3328 | `a3810f4f7522d80c4029713773411919619cd80868b2d7978f8cd3728bd1d549` |
| `games/mastermind-0300.bin` | Mastermind `$0300–$03B0` | 177 | `80e1aa7008bfc51a29a496ca3626e73b56de5e5923ab262fb24f0859a5097313` |
| `games/microchess-0300.bin` | Microchess `$0300–$0BC7` | 2248 | `be5fda9d9db767bba5a08e1bec9b0208c93c45ec6ea16622f1ec6e11ebf9154f` |
| `games/mini-startrek-004a.bin` | Mini-Startrek `$004A–$00FF` | 182 | `1672465179d0a25339c799ba512049a3600a7a2ff83f91662a62dbbcc7ae9109` |
| `games/mini-startrek-0300.bin` | Mini-Startrek `$0300–$0FFF` | 3328 | `6a47bfece26c9bac4cb05d43c8f77b9eb981c3a89d00504baa67ec8a7ddc01cf` |
| `games/peg-solitaire-0300.bin` | Peg Solitaire `$0300–$05FF` | 768 | `fb3d3851db4be521d1ce6362cca5bdd61e33dbeb74927fdd89ce60d451f569a3` |
| `games/shut-the-box-0300.bin` | Shut the Box `$0300–$06FF` | 1024 | `cdbcdc028f9336c31866e7d4d6076db6e3efd2af8bd7fe7e3fc35bb102d47431` |
| `games/worple-0300.bin` | Worple! `$0300–$0FFE` | 3327 | `aae16b9339ae4ae968b5e23ba226dcf1ea92ce7f1d1caf89fc74293a4b6305cd` |
| `fun/30th-0280.bin` | Apple 30th Anniversary `$0280–$0FFF` | 3456 | `ff0ff5ea82a2b2c4df2ba36d2fc11957b47b073e1ef0dc7ae19683f646e8eeb0` |
| `fun/beer-0bee.bin` | 99 Bottles of Beer `$0BEE–$0CB5` | 200 | `1fffd9da9c6ff10cdf44fe2f47bad28c2a4a4292f558f565213404be3ccdbe9b` |
| `fun/cat-0280.bin` | Cat `$0280–$0546` | 711 | `0917891b83c24036d57b87161a531d610c4b16bf8e89c4f0dea1f2c2c4c3a328` |
| `fun/cellular-0300.bin` | Cellular `$0300–$052C` | 557 | `35706149b173be95e90e9d6cbb44c6823a6d7b0185c0b9c8be444d6cd19ae499` |
| `fun/mandelbrot-65-0280.bin` | Mandelbrot 65 `$0280–$07B6` | 1335 | `3ad221616ac83b231355de9a1b367d9c66ac0cb9d744fc67b48c0620158d5951` |
| `fun/pasart-0300.bin` | Pasart `$0300–$0535` | 566 | `f1a8d902d6042fcadfb383e41f039a126dbe8503e0fef09626519a4c2bfa2cc2` |
| `fun/twinkle-004a.bin` | Twinkle Twinkle Little Star `$004A–$00FF` | 182 | `7c86f6c5d21a1a2b3579ab0118f6a880132b2ffe9292be6673ee9548e9e4ea93` |
| `fun/twinkle-0800.bin` | Twinkle Twinkle Little Star `$0800–$0FFF` | 2048 | `3e6d856d662f14d16f4703503d4806421f52b33e6fc4dbaa53c3c9bcff3fb73e` |
| `programming/a1assembler-e000.bin` | A1-Assembler `$E000–$EFCD` | 4046 | `5e53e9466d7d78619334cb79cfe4e1b23c5c04e7aa996abd785ca9a21650bd9d` |
| `programming/ascii-hex-keyboard-0700.bin` | ASCII HEX (Keyboard) `$0700–$071C` | 29 | `e7a7109e6ac24bef97d2d9474152a2171c785bef247525c22b4bc13a0c8bf620` |
| `programming/ascii-hex-printing-0750.bin` | ASCII HEX (Printing) `$0750–$0776` | 39 | `16f058d7262fe77bac9af208f5486b03bd08bc56a5f2d209b17bb7c4274fddcf` |
| `programming/basic-c-e000.bin` | Apple BASIC (C) `$E000–$EFFF` | 4096 | `e423c5c1acff4bea521a72dd4ce4b1435a442cfaad2604b1bfd6edd0fb6d0fc9` |
| `programming/basic-d-e000.bin` | Apple BASIC (D) `$E000–$EFFF` | 4096 | `56d5cd968557c81a99cde298d76030f65bb7ce9a85bc2ff0fed5726d50b91499` |
| `programming/basic-huston-e000.bin` | Apple BASIC (Huston) `$E000–$EFFF` | 4096 | `311c85f22996e655ae3a0881e0841a547c52f5ec20cd810035ec91ce13a27cbe` |
| `programming/basic-pagetable-e000.bin` | Apple BASIC (Pagetable) `$E000–$EFFF` | 4096 | `bf80009454610a1066489da635a8afb51ad42442d307251896a53bedbeaadd46` |
| `programming/dis-assembler-0800.bin` | Dis-Assembler `$0800–$09DB` | 476 | `8abe0f47c536ceb167deebe755449e92faacfdb752a27d68e73cb607ba9cc234` |
| `programming/hellorld-0300.bin` | Hellorld! `$0300–$031C` | 29 | `89789eda00137da0e4f64171e83d8e6aaf2669be09d526bef245f9a05392a6c1` |
| `programming/stringout-espinosa-0400.bin` | Stringout (Espinosa) `$0400–$0432` | 51 | `5b0effaca1877d0a28e77d429b7ced94a5df4d3b26cbf5bf4278dc87dea3cd46` |
| `programming/stringout-meier-0400.bin` | Stringout (Meier) `$0400–$0429` | 42 | `68afd26c317f9e8b1e00a6d99f46d42aa659c46ff6e8cad1440e302f6867d30e` |
| `utilities/memory-test-0009-027f-0000.bin` | Memory Test (0009-027F △) `$0000–$0003` | 4 | `fea0fa40195ab13425e05619091db75e58b9308e49ac4ae0ba03af0f31e85aff` |
| `utilities/memory-test-0009-027f-0280.bin` | Memory Test (0009-027F △) `$0280–$03A1` | 290 | `3ed60dfeae33dbb19aa4b8457bf8f2f05ea875ce8f54a80fed7e5948148c1b10` |
| `utilities/memory-test-03a2-0fff-0000.bin` | Memory Test (03A2-0FFF △) `$0000–$0003` | 4 | `af1a8fa1bc66576f657bc59b64d117a44cb79436942690a21b341a395cf05e94` |
| `utilities/memory-test-03a2-0fff-0280.bin` | Memory Test (03A2-0FFF △) `$0280–$03A1` | 290 | `3ed60dfeae33dbb19aa4b8457bf8f2f05ea875ce8f54a80fed7e5948148c1b10` |
| `utilities/memory-test-1000-1fff-0000.bin` | Memory Test (1000-1FFF ▽) `$0000–$0003` | 4 | `87230ca20b237068e47cf5e39110dcb36a45668b9014193df324c7ebd085c338` |
| `utilities/memory-test-1000-1fff-0280.bin` | Memory Test (1000-1FFF ▽) `$0280–$03A1` | 290 | `3ed60dfeae33dbb19aa4b8457bf8f2f05ea875ce8f54a80fed7e5948148c1b10` |
| `utilities/memory-test-e000-efff-0000.bin` | Memory Test (E000-EFFF ▽) `$0000–$0003` | 4 | `01693e8dec08fb53eb20693ff9d11ae53afe6973897ed71ef566ff0734d1b65e` |
| `utilities/memory-test-e000-efff-0280.bin` | Memory Test (E000-EFFF ▽) `$0280–$03A1` | 290 | `3ed60dfeae33dbb19aa4b8457bf8f2f05ea875ce8f54a80fed7e5948148c1b10` |
| `utilities/party-e000.bin` | Party Checkin `$E000–$EE94` | 3733 | `5fa01ebebd873cdcafe78cadcc90cc1ed032692f3b33f3f82f62970e99c11d83` |
| `utilities/resistor-calculator-004a.bin` | Resistor Calculator `$004A–$00FF` | 182 | `dcc038f453f674ed52525fa59d08ea6ae43ce9ce89760cae6331f1a8e465f841` |
| `utilities/resistor-calculator-0800.bin` | Resistor Calculator `$0800–$0FFF` | 2048 | `688d8c799366aeac22fbd609d32dac1c499eaf046b2f34bdfff5e4d0350ded9d` |
| `utilities/stopwatch-004a.bin` | Stopwatch `$004A–$00FF` | 182 | `c21a993cf7a57f8dd16376c6bbf9892add424a1df63ea9f7bff383e0db78bfc5` |
| `utilities/stopwatch-0800.bin` | Stopwatch `$0800–$0FFF` | 2048 | `9f51085db06dffbb5389ccb802587359d32454fec3910dfed48093333cdd4e86` |
| `utilities/test-program-0000.bin` | Test Program `$0000–$000A` | 11 | `c380df376dff865769f6da0ad5adf22c18552e18cd0464a7405b682148c8b5cb` |
| `utilities/typewriter-0300.bin` | TypeWriter `$0300–$03AA` | 171 | `a57d17dbbf5553e89112e8b3d283765f79e9bc7d5ed19723e04dd3c5c45ac91d` |
| `utilities/typewriter-0400.bin` | TypeWriter `$0400–$0419` | 26 | `b9f2165a68416fd38a4d70753fabd488d1db8581c175545217d0d66ce0b8fbca` |
| `utilities/typewriter-0440.bin` | TypeWriter `$0440–$0459` | 26 | `7ef0e52b04439692bb18b7b163324127f8ce3b2ecb0e9cbb6ba8878c42dc3771` |

## Verification

- `cargo test -p hesper --lib presets` — 42 entries, unique ids, every block
  inside one RAM bank, every start address inside a block the preset writes,
  and `needs_expansion` matching what the banks actually accept.
- `cargo test -p hesper --test apple1 presets_can_be_listed_without_rom_and_conflicting_options_are_rejected`
  — `--list-presets` names all 42 programs with category, size, ranges, start
  command and source page.
- ROM-gated (run with `make wozmon-tests`):
  `bundled_basic_runs_calculations_and_a_numbered_loop` (`basic-huston`),
  `bundled_basic_program_runs_after_the_published_warm_entry`
  (`resistor-calculator` at `E2B3R` then `RUN`),
  `bundled_assembly_program_runs_from_its_published_entry`
  (`15-puzzle` at `0300R`), and
  `presets_longer_than_a_ram_bank_are_rejected_with_that_reason`
  (`little-tower`).
- To re-download and compare any program with the publisher:

  ```sh
  curl -s "https://apple1software.com/games/15-puzzle/wozmon?basic=false&autostart=true" \
    | python3 -c 'import base64,json,sys; sys.stdout.write(base64.b64decode(json.load(sys.stdin)["data"]).decode("latin1"))'
  ```

  The listing's `AAAA`/`:BB CC …` lines are the blocks stored here; the last
  line (`0300R`) is the start command in the table above.

## Licensing

The site publishes a licence statement on 8 of the 42 pages: six are MIT
(`15-puzzle`, `mandelbrot-65`, `peg-solitaire`, `shut-the-box`, `worple`,
`party`), two are labelled "Custom License" (`codebreaker`, `microchess`), and
the other 34 pages - including every historical Apple-1 tape program and Apple
BASIC itself - carry no licence statement at all.

These are third-party historical binaries, redistributed here exactly as the
publisher serves them so the emulator can be exercised against the same images
a real machine receives. No new licence, public-domain status or additional
provenance beyond what the table records is asserted for them; copyright stays
with their authors, and the collection itself is © Relate.IT. Everything is
downloaded from the public site, dated, hashed above and reproducible with the
command in Verification — delete this directory and the presets with it if a
program's terms turn out to require that.
