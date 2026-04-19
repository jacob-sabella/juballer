use super::media_common::{fire, MediaCmd};
use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct MediaPlaypause;

impl BuildFromArgs for MediaPlaypause {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for MediaPlaypause {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        fire(cx, "action.media.playpause", MediaCmd::PlayPause);
    }
}
