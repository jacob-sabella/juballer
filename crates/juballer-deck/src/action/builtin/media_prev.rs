use super::media_common::{fire, MediaCmd};
use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct MediaPrev;

impl BuildFromArgs for MediaPrev {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for MediaPrev {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        fire(cx, "action.media.prev", MediaCmd::Prev);
    }
}
