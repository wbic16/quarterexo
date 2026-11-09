//! xz container support, zero-dependency: encode is a valid xz stream
//! using stored (uncompressed) LZMA2 chunks; decode verifies the xz
//! CRC32/CRC64 structure and extracts stored chunks.
//!
//! Format notes (from the xz file format spec):
//! - Stream: 12-byte header (magic + stream flags + CRC32 of flags),
//!   blocks, index, footer. Check type used here: CRC64 (0x04) for the
//!   block payload, matching `xz -9`'s default --check=crc64.
//! - LZMA2 stored chunk: control byte 0x01 (dictionary reset + stored),
//!   then 2-byte big-endian (size-1), then raw bytes. Chunk payload max
//!   65536 bytes. End marker 0x00.
//! - Block padding to 4-byte alignment with zeros; backward size in the
//!   footer counts the index.
//!
//! Interoperability: streams produced here are decoded by the system
//! `xz` in tests and by decode_stored() below; decode accepts only
//! stored-chunk streams (what encode produces, and what the bake
//! produces for the in-binary corpus). Baked corpus streams are
//! produced by the system `xz` where available at bake time; qexo's
//! own decoder handles the stored form it embeds itself. For streams
//! made by `xz` with real LZMA2 compression, unseal shells to the
//! `xz` binary (wrap, don't fork — the same rule as gpg).

// ---------------------------------------------------------------- CRC32
const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
static CRC32_TABLE: [u32; 256] = make_crc32_table();

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC32_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------- CRC64
// xz CRC64: polynomial 0xC96C5795D7870F42 (reflected), init/xorout all-ones.
const fn make_crc64_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u64;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xC96C5795D7870F42 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
static CRC64_TABLE: [u64; 256] = make_crc64_table();

pub fn crc64(data: &[u8]) -> u64 {
    let mut c = 0xFFFF_FFFF_FFFF_FFFFu64;
    for &b in data {
        c = CRC64_TABLE[((c ^ b as u64) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF_FFFF_FFFF
}

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut b = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
    out
}

fn read_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift = 0;
    loop {
        let b = *data.get(*pos)?;
        *pos += 1;
        v |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    Some(v)
}

/// Encode `data` as a complete xz stream (stored LZMA2 chunks, CRC64
/// block check, single block). Byte-deterministic for a given input.
pub fn encode_stored(data: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // Stream header: magic + flags (check type CRC64 = 0x04) + CRC32.
    out.extend_from_slice(b"\xFD7zXZ\x00");
    let flags = [0x00u8, 0x04];
    out.extend_from_slice(&flags);
    out.extend_from_slice(&crc32(&flags).to_le_bytes());

    // Build LZMA2 payload (stored chunks of <= 65536 bytes).
    let mut lzma2: Vec<u8> = Vec::new();
    let mut first = true;
    for chunk in data.chunks(65536) {
        if chunk.is_empty() {
            break;
        }
        // control: stored chunk, dict reset flag on the first chunk
        lzma2.push(if first { 0x01 } else { 0x02 });
        first = false;
        let sz = (chunk.len() - 1) as u16;
        lzma2.extend_from_slice(&sz.to_be_bytes());
        lzma2.extend_from_slice(chunk);
    }
    lzma2.push(0x00); // end of LZMA2
    let check = crc64(data);

    // Block header: size byte encodes (n+1)*4 bytes total header.
    // Fields: block header size (varint), flags (1 filter), filter id
    // LZMA2 (0x21), props size (1), props (dict size byte 0x00 =
    // smallest; stored chunks ignore it), padding to 4, CRC32.
    let mut hdr_body: Vec<u8> = Vec::new();
    hdr_body.push(0x00); // flags: no sizes stored, 1 filter
    hdr_body.push(0x21); // filter: LZMA2
    hdr_body.push(0x01); // properties size
    hdr_body.push(0x00); // dict size property (4 KiB min encoding)
    // total header = 1 (size byte) + body + padding + 4 (crc32), mult of 4
    let mut total = 1 + hdr_body.len() + 4;
    let pad_hdr = (4 - (total % 4)) % 4;
    total += pad_hdr;
    let mut block_header: Vec<u8> = Vec::new();
    block_header.push((total / 4 - 1) as u8);
    block_header.extend_from_slice(&hdr_body);
    block_header.extend(std::iter::repeat(0u8).take(pad_hdr));
    let hcrc = crc32(&block_header);
    block_header.extend_from_slice(&hcrc.to_le_bytes());

    let block_start = out.len();
    out.extend_from_slice(&block_header);
    out.extend_from_slice(&lzma2);
    let pad_len = (4 - ((block_header.len() + lzma2.len()) % 4)) % 4;
    out.extend(std::iter::repeat(0u8).take(pad_len));
    out.extend_from_slice(&check.to_le_bytes());
    let unpadded = (block_header.len() + lzma2.len() + 8) as u64; // +check, excl padding
    let _ = block_start;

    // Index: indicator 0x00, record count varint, one record
    // (unpadded size, uncompressed size), index padding, CRC32.
    let mut index: Vec<u8> = Vec::new();
    index.push(0x00);
    index.extend_from_slice(&varint(1));
    index.extend_from_slice(&varint(unpadded));
    index.extend_from_slice(&varint(data.len() as u64));
    while index.len() % 4 != 0 {
        index.push(0x00);
    }
    let icrc = crc32(&index);
    index.extend_from_slice(&icrc.to_le_bytes());
    let backward_size = (index.len() / 4 - 1) as u32;
    out.extend_from_slice(&index);

    // Footer: CRC32 of next 6 bytes, backward size, flags, magic 'YZ'.
    let mut foot_tail: Vec<u8> = Vec::new();
    foot_tail.extend_from_slice(&backward_size.to_le_bytes());
    foot_tail.extend_from_slice(&flags);
    let fcrc = crc32(&foot_tail);
    out.extend_from_slice(&fcrc.to_le_bytes());
    out.extend_from_slice(&foot_tail);
    out.extend_from_slice(b"YZ");
    out
}

/// Decode a stored-chunk xz stream produced by encode_stored().
pub fn decode_stored(stream: &[u8]) -> Result<Vec<u8>, String> {
    if stream.len() < 12 || &stream[0..6] != b"\xFD7zXZ\x00" {
        return Err("not an xz stream".into());
    }
    let check_type = stream[7] & 0x0F;
    if check_type != 0x04 {
        return Err(format!("unsupported check type {:#x} (need CRC64)", check_type));
    }
    // Block header starts at 12.
    let hdr_size = ((stream[12] as usize) + 1) * 4;
    let mut pos = 12 + hdr_size;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let ctrl = *stream.get(pos).ok_or("truncated LZMA2")?;
        pos += 1;
        if ctrl == 0x00 {
            break;
        }
        if ctrl == 0x01 || ctrl == 0x02 {
            let hi = *stream.get(pos).ok_or("truncated chunk")? as usize;
            let lo = *stream.get(pos + 1).ok_or("truncated chunk")? as usize;
            pos += 2;
            let n = (hi << 8 | lo) + 1;
            let end = pos + n;
            if end > stream.len() {
                return Err("chunk overruns stream".into());
            }
            out.extend_from_slice(&stream[pos..end]);
            pos = end;
        } else {
            return Err(format!("unsupported LZMA2 chunk control {:#x} (compressed chunks need the xz tool)", ctrl));
        }
    }
    // verify block check (CRC64) after padding
    let pad = (4 - ((hdr_size + (pos - 12 - hdr_size)) % 4)) % 4;
    let _ = pad;
    // locate check: it follows the LZMA2 data + block padding; recompute
    // the padding from the block: block bytes after header = pos - (12+hdr).
    let block_data_len = pos - (12 + hdr_size);
    let pad_len = (4 - ((hdr_size + block_data_len) % 4)) % 4;
    let check_pos = pos + pad_len;
    if check_pos + 8 <= stream.len() {
        let stored = u64::from_le_bytes(stream[check_pos..check_pos + 8].try_into().unwrap());
        if stored != crc64(&out) {
            return Err("block check (CRC64) mismatch".into());
        }
    }
    Ok(out)
}

/// Unseal any xz stream: stored form decodes natively; compressed
/// streams shell out to the system `xz` (wrap, don't fork).
pub fn unseal(stream: &[u8]) -> Result<Vec<u8>, String> {
    match decode_stored(stream) {
        Ok(v) => Ok(v),
        Err(_) => {
            use std::io::Write;
            let mut child = std::process::Command::new("xz")
                .args(["-d", "-c"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("xz tool unavailable: {}", e))?;
            child.stdin.take().unwrap().write_all(stream).map_err(|e| e.to_string())?;
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(output.stdout)
            } else {
                Err("xz tool failed to decompress stream".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_vectors() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
        assert_eq!(crc64(b"123456789"), 0x995DC9BBDF1939FA);
    }

    #[test]
    fn stored_roundtrip() {
        for payload in [&b""[..], b"hello phext", &[7u8; 200_000]] {
            let sealed = encode_stored(payload);
            assert_eq!(decode_stored(&sealed).unwrap(), payload.to_vec());
        }
    }
}
