//! Static SPA assets bundled into the binary. Currently a minimal index.html
//! that reports the server is alive; the full SPA bundle replaces it.

pub const INDEX_HTML: &str = include_str!("assets/index.html");
