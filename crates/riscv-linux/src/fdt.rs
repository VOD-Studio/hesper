const HEADER_SIZE: usize = 40;
const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

/// Validate the tree and return the byte offset of the unique 8-byte initrd end.
pub(crate) fn check(bytes: &[u8], initrd_start: u64, initrd_end: u64) -> Option<usize> {
    let mut header = bytes.get(..HEADER_SIZE)?;
    if word(&mut header)? != FDT_MAGIC || size(&mut header)? != bytes.len() {
        return None;
    }
    let structure_start = size(&mut header)?;
    let strings_start = size(&mut header)?;
    let reservations_start = size(&mut header)?;
    if word(&mut header)? != 17 || word(&mut header)? != 16 {
        return None;
    }
    if word(&mut header)? != 0 {
        return None;
    }
    let strings_end = strings_start.checked_add(size(&mut header)?)?;
    let structure_end = structure_start.checked_add(size(&mut header)?)?;
    if reservations_start < HEADER_SIZE
        || reservations_start % 8 != 0
        || structure_start % 4 != 0
        || structure_end > strings_start
    {
        return None;
    }
    let mut reservations = bytes.get(reservations_start..structure_start)?;
    while take(&mut reservations, 16)? != [0; 16] {}

    let mut structure = bytes.get(structure_start..structure_end)?;
    let strings = bytes.get(strings_start..strings_end)?;
    let initrd_start = initrd_start.to_be_bytes();
    let initrd_end = initrd_end.to_be_bytes();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut properties_allowed = false;
    let mut reserved_memory_seen = false;
    let mut chosen_seen = false;
    let mut in_chosen = false;
    let mut chosen_properties = 0u8;
    let mut initrd_end_offset = None;
    loop {
        match word(&mut structure)? {
            FDT_BEGIN_NODE => {
                let name = c_string(structure)?;
                let encoded_len = name.len() + 1;
                take(&mut structure, encoded_len)?;
                padding(&mut structure, encoded_len)?;
                if depth == 0 {
                    if root_seen || !name.is_empty() {
                        return None;
                    }
                    root_seen = true;
                } else if name.is_empty() || name.contains(&b'/') {
                    return None;
                } else if depth == 1 {
                    match name {
                        b"chosen" if !chosen_seen => {
                            chosen_seen = true;
                            in_chosen = true;
                        }
                        b"reserved-memory" if !reserved_memory_seen => {
                            reserved_memory_seen = true;
                        }
                        b"chosen" | b"reserved-memory" => return None,
                        // libfdt also matches a unit-address suffix in these lookups.
                        name if name.starts_with(b"chosen@")
                            || name.starts_with(b"reserved-memory@") =>
                        {
                            return None;
                        }
                        _ => {}
                    }
                }
                depth += 1;
                properties_allowed = true;
            }
            FDT_END_NODE => {
                if depth == 2 {
                    in_chosen = false;
                }
                depth = depth.checked_sub(1)?;
                properties_allowed = false;
            }
            FDT_PROP if properties_allowed => {
                let len = size(&mut structure)?;
                let offset = size(&mut structure)?;
                let name = c_string(strings.get(offset..)?)?;
                if name.is_empty() {
                    return None;
                }
                let value_offset = structure_end - structure.len();
                let value = take(&mut structure, len)?;
                padding(&mut structure, len)?;
                if depth == 2 && in_chosen {
                    let (bit, expected): (u8, &[u8]) = match name {
                        b"bootargs" => (1, b"console=ttyS0 rdinit=/init\0"),
                        b"linux,initrd-start" => (2, &initrd_start),
                        b"linux,initrd-end" => (4, &initrd_end),
                        _ => continue,
                    };
                    if chosen_properties & bit != 0 || value != expected {
                        return None;
                    }
                    chosen_properties |= bit;
                    if bit == 4 {
                        initrd_end_offset = Some(value_offset);
                    }
                }
            }
            FDT_NOP => {}
            FDT_END => {
                if depth != 0
                    || !structure.is_empty()
                    || !reserved_memory_seen
                    || chosen_properties != 7
                {
                    return None;
                }
                return initrd_end_offset;
            }
            _ => return None,
        }
    }
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    let (prefix, rest) = bytes.split_at_checked(len)?;
    *bytes = rest;
    Some(prefix)
}

fn word(bytes: &mut &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(take(bytes, 4)?.try_into().ok()?))
}

fn size(bytes: &mut &[u8]) -> Option<usize> {
    word(bytes)?.try_into().ok()
}

fn c_string(bytes: &[u8]) -> Option<&[u8]> {
    let end = bytes.iter().position(|&byte| byte == 0)?;
    Some(&bytes[..end])
}

fn padding(bytes: &mut &[u8], len: usize) -> Option<()> {
    take(bytes, (4 - len % 4) % 4)?
        .iter()
        .all(|&byte| byte == 0)
        .then_some(())
}
