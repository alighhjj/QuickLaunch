//! 从 `.icns` 中提取可用的 PNG 负载。
//!
//! 刻意不引入图像解码库：现代 macOS 应用的 `.icns` 内部以 PNG 分块存储，
//! 我们只需按 ICNS 容器格式切出分块、挑一个尺寸合适的 PNG 原样透传即可，
//! 全程零解码、零格式转换，也就没有本地图像依赖。

use std::path::Path;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;

/// 分块类型按「优先使用」排序。
/// 取 128/256 而非最大的 1024：显示尺寸是 32~48pt，128px 已满足 Retina，
/// 再大只会成倍放大 base64 体积。
const PREFERRED: [&[u8; 4]; 10] = [
    b"ic08", // 256x256  PNG
    b"ic07", // 128x128  PNG
    b"ic13", // 256x256@2x
    b"ic09", // 512x512  PNG
    b"ic12", // 64x64@2x
    b"ic11", // 32x32@2x
    b"ic10", // 1024x1024 (512@2x)
    b"icp6", // 64x64
    b"icp5", // 32x32
    b"icp4", // 16x16
];

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 读取 `.icns` 并返回 `data:image/png;base64,...`。
/// 若文件不含 PNG 分块（极老的图标只带 `is32`/`il32` 位图），返回 `None`，
/// 前端会退化为字母徽标，不影响使用。
pub fn icns_to_data_url(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let chunk = pick_png_chunk(&bytes)?;
    Some(format!("data:image/png;base64,{}", B64.encode(chunk)))
}

fn pick_png_chunk(buf: &[u8]) -> Option<&[u8]> {
    if buf.len() < 8 || &buf[0..4] != b"icns" {
        return None;
    }
    // 头部：magic(4) + 文件总长(4)
    let declared = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let end = declared.min(buf.len());

    let mut found: Vec<([u8; 4], &[u8])> = Vec::new();
    let mut off = 8usize;
    while off + 8 <= end {
        let mut kind = [0u8; 4];
        kind.copy_from_slice(&buf[off..off + 4]);
        let len = u32::from_be_bytes([buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7]]) as usize;
        // 长度字段含自身 8 字节头部；越界说明文件被截断，就地停止。
        if len < 8 || off + len > end {
            break;
        }
        let data = &buf[off + 8..off + len];
        if data.len() > PNG_MAGIC.len() && data[..8] == PNG_MAGIC {
            found.push((kind, data));
        }
        off += len;
    }

    for want in PREFERRED.iter() {
        if let Some(pos) = found.iter().position(|(kind, _)| kind == *want) {
            return Some(found[pos].1);
        }
    }
    // 兜底：任何 PNG 分块都好过没有。
    found.first().map(|(_, data)| *data)
}
