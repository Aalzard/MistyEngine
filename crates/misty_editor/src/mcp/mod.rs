//! MCP server: AI agents (Claude, Codex…) drive the editor through the Model Context Protocol.
//!
//! Two ways in, one set of tools ([`tools`]):
//! - `misty --mcp [project]` ([`headless`]): opens the project without a window and serves the
//!   tools over stdio (JSON-RPC 2.0, one message per line). The MCP client starts it.
//! - The live editor ([`live`]): with Edit ▸ Preferences ▸ AI Tools ▸ "Allow AI tools (MCP)"
//!   on (off by default), the editor listens on a loopback port with a random token written to
//!   `Library/MistyMcp.json`; `misty --mcp-connect [project]` ([`bridge::connect_main`]) is the
//!   stdio proxy an MCP client starts to reach it. Destructive tools wait for Allow / Deny in
//!   the editor.
//!
//! The protocol is implemented here with `serde_json` ([`protocol`]): the MCP subset a tool
//! server needs (`initialize`, `ping`, `tools/list`, `tools/call`) is small, and the Rust MCP
//! crates bring an async runtime the editor has no other use for. Edits go through the editor's
//! own commands and undo stack, so an AI edit is one Ctrl+Z away like any other.
//!
//! Nothing listens beyond 127.0.0.1; see `docs/mcp.md` for the security model.

pub mod bridge;
pub mod headless;
pub mod live;
pub mod protocol;
pub mod shot;
pub mod tools;

#[cfg(test)]
mod tests;

/// Standard base64 (with padding), for image content.
pub fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// RGBA8 pixels as a PNG file.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// 32 random bytes from the OS, as hex (the bridge token).
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no OS randomness: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compare secrets without stopping at the first difference.
pub fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
