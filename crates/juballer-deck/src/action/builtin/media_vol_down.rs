use super::media_common::{fire, MediaCmd};
use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct MediaVolDown;

impl BuildFromArgs for MediaVolDown {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for MediaVolDown {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        fire(cx, "action.media.vol_down", MediaCmd::VolDown);
    }
}
