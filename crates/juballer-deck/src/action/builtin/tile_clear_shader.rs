//! tile.clear_shader action — remove the tile's raw-wgpu shader source.
//!
//! No args.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct TileClearShader;

impl BuildFromArgs for TileClearShader {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for TileClearShader {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.tile.clear_shader();
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_accepts_empty() {
        let _ = TileClearShader::from_args(&toml::Table::new()).unwrap();
    }
}
