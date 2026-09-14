# Bundled Apple-1 programs

`basic-huston.bin` is the 4096-byte image supplied by the user on 2026-09-14,
copied byte-for-byte from their file of the same name. The Huston variant name
comes from that supplied filename; no upstream byte-for-byte identity is claimed.

- Preset ID: `basic-huston`
- SHA-256: `311c85f22996e655ae3a0881e0841a547c52f5ec20cd810035ec91ce13a27cbe`
- Load range: `$E000–$EFFF` (the Apple-1 high RAM bank)
- Cold entry: `$E000`, entered with `E000R` in Woz Monitor
- Warm entry: `$E2B3`, entered with `E2B3R` after RESET to preserve the BASIC program

The host embeds these bytes with `include_bytes!`; it neither downloads nor
patches the image. The program is loaded into writable RAM before boot. Physical
RESET preserves RAM, while a new machine reloads the original bundled bytes.
Woz Monitor is still supplied separately via the existing ROM loader.

This is a third-party historical binary, not original Hesper source; no new
license or public-domain status is asserted for it. The original
[Apple-1 BASIC user manual](https://wiki.reactivemicro.com/images/7/75/Preliminary_APPLE_1_BASIC_USERS_MANUAL.pdf)
documents the BASIC entry command; Mike Willegal's
[Brain Board operations guide](https://www.willegal.net/appleii/bb-v5_2.pdf)
describes Huston BASIC at `$E000`.

Local execution is covered by `bundled_basic_runs_calculations_and_a_numbered_loop`
in `crates/cli/tests/apple1.rs`, run with `make wozmon-tests`. The offline preset
test checks the supplied image's size, hash and RAM range.
