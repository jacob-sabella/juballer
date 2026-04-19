use super::media_common::{fire, MediaCmd};
use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct MediaNext;

impl BuildFromArgs for MediaNext {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for MediaNext {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        fire(cx, "action.media.next", MediaCmd::Next);
    }
}
