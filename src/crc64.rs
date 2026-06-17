use std::sync::OnceLock;

const H1_INIT: u32 = 0xFAC432B1;
const H2_INIT: u32 = 0x0CD5E44A;
const P1: u32 = 0x0060_0340;
const P2: u32 = 0x00F0_D50B;

static CRC64_TABLE: OnceLock<[(u32, u32); 256]> = OnceLock::new();

fn crc64_table() -> &'static [(u32, u32); 256] {
    CRC64_TABLE.get_or_init(|| {
        let mut t = [(0u32, 0u32); 256];
        for i in 0..256u32 {
            let mut h1 = 0u32;
            let mut h2 = 0u32;
            let mut v = i;
            for _ in 0..8 {
                h1 <<= 1;
                if (h2 & 0x8000_0000) != 0 {
                    h1 |= 1;
                }
                h2 <<= 1;
                if (v & 0x80) != 0 {
                    h1 ^= P1;
                    h2 ^= P2;
                }
                v <<= 1;
            }
            t[i as usize] = (h1, h2);
        }
        t
    })
}

fn java_crc64(value: &str) -> i64 {
    let table = crc64_table();
    let mut h1 = H1_INIT;
    let mut h2 = H2_INIT;
    let mask: u32 = 0xFFFF_FFFF;
    for code_unit in value.encode_utf16() {
        let old_h1 = h1;
        let old_h2 = h2;
        let idx = ((old_h1 >> 24) & 0xFF) as usize;
        h1 = (old_h1 << 8 & mask) ^ (old_h2 >> 24) ^ table[idx].0;
        h2 = (old_h2 << 8) ^ (code_unit as u32) ^ table[idx].1;
    }
    (h2 as i32 as i64) * 4_294_967_296i64 + (h1 as i32 as i64)
}

/// 计算 CRC64 值，算法与 Java Crc.getCrc64 完全一致。
pub fn crc64_ecma(value: &str) -> i64 {
    java_crc64(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc64_matches_java_implementation() {
        assert_eq!(crc64_ecma("test"), 318199613220311169);
        assert_eq!(
            crc64_ecma("8105:ZTE-CMAH-HF,SubNetwork=500,ManagedElement=1561205,EnbFunction=379834,EutranCellFdd=6"),
            -9125147111095642511
        );
    }
}
