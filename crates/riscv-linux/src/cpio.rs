const HEADER_SIZE: usize = 110;
const MAGIC: &[u8] = b"070701";
// Linux's initramfs unpacker bounds c_namesize (including NULs) by PATH_MAX.
const MAX_NAME_SIZE: usize = 4096;

pub(crate) fn has_newc_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

pub(crate) fn check(mut bytes: &[u8]) -> Option<()> {
    loop {
        let (header, rest) = bytes.split_at_checked(HEADER_SIZE)?;
        // c_chksum must be zero for newc.
        if !has_newc_magic(header)
            || !header[MAGIC.len()..].iter().all(u8::is_ascii_hexdigit)
            || &header[102..110] != b"00000000"
        {
            return None;
        }
        let data_len = hex_size(&header[54..62])?;
        let name_len = hex_size(&header[94..102])?;
        if !(2..=MAX_NAME_SIZE).contains(&name_len) {
            return None;
        }
        let (name, rest) = rest.split_at_checked(name_len)?;
        let end = name.iter().position(|&byte| byte == 0)?;
        // c_namesize can include extra NULs, but no bytes of another name.
        if end == 0 || !all_zero(&name[end..]) {
            return None;
        }
        let rest = skip_padding(rest, HEADER_SIZE.checked_add(name_len)?)?;
        if &name[..end] == b"TRAILER!!!" {
            return (data_len == 0 && all_zero(rest)).then_some(());
        }
        bytes = skip_padding(rest.get(data_len..)?, data_len)?;
    }
}

fn hex_size(field: &[u8]) -> Option<usize> {
    usize::from_str_radix(core::str::from_utf8(field).ok()?, 16).ok()
}

fn skip_padding(bytes: &[u8], preceding_len: usize) -> Option<&[u8]> {
    let (padding, rest) = bytes.split_at_checked((4 - preceding_len % 4) % 4)?;
    all_zero(padding).then_some(rest)
}

fn all_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte == 0)
}
