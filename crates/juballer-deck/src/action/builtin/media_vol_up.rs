use super::media_common::{fire, MediaCmd};
use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct MediaVolUp;

impl BuildFromArgs for MediaVolUp {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for MediaVolUp {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        fire(cx, "action.media.vol_up", MediaCmd::VolUp);
    }
}
