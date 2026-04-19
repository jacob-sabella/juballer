//! tile.set_shader action — swap the current tile's raw-wgpu shader source.
//!
//! Args (mutually exclusive; at least one required):
//!   wgsl  : string path to a .wgsl file → custom shader
//!   video : string URI (currently v4l2:///dev/videoN) → live video
//!
//! The new source takes effect on the next render frame.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::tile::TileShaderSource;
use crate::{Error, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub struct TileSetShader {
    src: TileShaderSource,
}

impl BuildFromArgs for TileSetShader {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let wgsl = args.get("wgsl").and_then(|v| v.as_str());
        let video = args.get("video").and_then(|v| v.as_str());
        let src = match (wgsl, video) {
            (Some(p), None) => TileShaderSource::CustomShader {
                wgsl_path: PathBuf::from(p),
                params: std::collections::HashMap::new(),
            },
            (None, Some(u)) => TileShaderSource::Video { uri: u.to_string() },
            (Some(_), Some(_)) => {
                return Err(Error::Config(
                    "tile.set_shader requires exactly one of `wgsl` or `video`, not both".into(),
                ))
            }
            (None, None) => {
                return Err(Error::Config(
                    "tile.set_shader requires `wgsl` or `video`".into(),
                ))
            }
        };
        Ok(Self { src })
    }
}

impl Action for TileSetShader {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.tile.set_shader(self.src.clone());
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgsl_arg_parses() {
        let mut args = toml::Table::new();
        args.insert(
            "wgsl".into(),
            toml::Value::String("/tmp/plasma.wgsl".into()),
        );
        let a = TileSetShader::from_args(&args).unwrap();
        match a.src {
            TileShaderSource::CustomShader { wgsl_path, .. } => {
                assert_eq!(wgsl_path, PathBuf::from("/tmp/plasma.wgsl"));
            }
            _ => panic!("expected CustomShader"),
        }
    }

    #[test]
    fn video_arg_parses() {
        let mut args = toml::Table::new();
        args.insert(
            "video".into(),
            toml::Value::String("v4l2:///dev/video0".into()),
        );
        let a = TileSetShader::from_args(&args).unwrap();
        match a.src {
            TileShaderSource::Video { uri } => assert_eq!(uri, "v4l2:///dev/video0"),
            _ => panic!("expected Video"),
        }
    }

    #[test]
    fn both_args_errors() {
        let mut args = toml::Table::new();
        args.insert("wgsl".into(), toml::Value::String("a".into()));
        args.insert("video".into(), toml::Value::String("b".into()));
        let err = TileSetShader::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn neither_arg_errors() {
        let err = TileSetShader::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
