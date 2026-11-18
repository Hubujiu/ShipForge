use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "brand.svg" => include_bytes!("../assets/brand.svg"),
            "icons/arrow-down-to-line.svg" => {
                include_bytes!("../assets/icons/arrow-down-to-line.svg")
            }
            "icons/arrow-left.svg" => include_bytes!("../assets/icons/arrow-left.svg"),
            "icons/arrow-right.svg" => include_bytes!("../assets/icons/arrow-right.svg"),
            "icons/bell-off.svg" => include_bytes!("../assets/icons/bell-off.svg"),
            "icons/bell.svg" => include_bytes!("../assets/icons/bell.svg"),
            "icons/blocks.svg" => include_bytes!("../assets/icons/blocks.svg"),
            "icons/book-open.svg" => include_bytes!("../assets/icons/book-open.svg"),
            "icons/bot.svg" => include_bytes!("../assets/icons/bot.svg"),
            "icons/boxes.svg" => include_bytes!("../assets/icons/boxes.svg"),
            "icons/calendar-days.svg" => include_bytes!("../assets/icons/calendar-days.svg"),
            "icons/calendar-range.svg" => include_bytes!("../assets/icons/calendar-range.svg"),
            "icons/chart-no-axes-column-increasing.svg" => {
                include_bytes!("../assets/icons/chart-no-axes-column-increasing.svg")
            }
            "icons/check.svg" => include_bytes!("../assets/icons/check.svg"),
            "icons/chevron-down.svg" => include_bytes!("../assets/icons/chevron-down.svg"),
            "icons/chevron-right.svg" => include_bytes!("../assets/icons/chevron-right.svg"),
            "icons/chevrons-up-down.svg" => include_bytes!("../assets/icons/chevrons-up-down.svg"),
            "icons/circle-dot.svg" => include_bytes!("../assets/icons/circle-dot.svg"),
            "icons/clock-arrow-left.svg" => include_bytes!("../assets/icons/clock-arrow-left.svg"),
            "icons/cloud.svg" => include_bytes!("../assets/icons/cloud.svg"),
            "icons/columns-3.svg" => include_bytes!("../assets/icons/columns-3.svg"),
            "icons/compass.svg" => include_bytes!("../assets/icons/compass.svg"),
            "icons/copy.svg" => include_bytes!("../assets/icons/copy.svg"),
            "icons/database.svg" => include_bytes!("../assets/icons/database.svg"),
            "icons/download.svg" => include_bytes!("../assets/icons/download.svg"),
            "icons/file-text.svg" => include_bytes!("../assets/icons/file-text.svg"),
            "icons/git-branch.svg" => include_bytes!("../assets/icons/git-branch.svg"),
            "icons/house.svg" => include_bytes!("../assets/icons/house.svg"),
            "icons/inbox.svg" => include_bytes!("../assets/icons/inbox.svg"),
            "icons/layers.svg" => include_bytes!("../assets/icons/layers.svg"),
            "icons/list-checks.svg" => include_bytes!("../assets/icons/list-checks.svg"),
            "icons/list-filter.svg" => include_bytes!("../assets/icons/list-filter.svg"),
            "icons/list.svg" => include_bytes!("../assets/icons/list.svg"),
            "icons/messages-square.svg" => include_bytes!("../assets/icons/messages-square.svg"),
            "icons/monitor.svg" => include_bytes!("../assets/icons/monitor.svg"),
            "icons/panel-left-close.svg" => include_bytes!("../assets/icons/panel-left-close.svg"),
            "icons/panel-left.svg" => include_bytes!("../assets/icons/panel-left.svg"),
            "icons/panels-top-left.svg" => include_bytes!("../assets/icons/panels-top-left.svg"),
            "icons/play.svg" => include_bytes!("../assets/icons/play.svg"),
            "icons/plug.svg" => include_bytes!("../assets/icons/plug.svg"),
            "icons/plus.svg" => include_bytes!("../assets/icons/plus.svg"),
            "icons/puzzle.svg" => include_bytes!("../assets/icons/puzzle.svg"),
            "icons/rotate-cw.svg" => include_bytes!("../assets/icons/rotate-cw.svg"),
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/send.svg" => include_bytes!("../assets/icons/send.svg"),
            "icons/settings-2.svg" => include_bytes!("../assets/icons/settings-2.svg"),
            "icons/settings.svg" => include_bytes!("../assets/icons/settings.svg"),
            "icons/sliders-horizontal.svg" => {
                include_bytes!("../assets/icons/sliders-horizontal.svg")
            }
            "icons/square-pen.svg" => include_bytes!("../assets/icons/square-pen.svg"),
            "icons/tag.svg" => include_bytes!("../assets/icons/tag.svg"),
            "icons/target.svg" => include_bytes!("../assets/icons/target.svg"),
            "icons/triangle-alert.svg" => include_bytes!("../assets/icons/triangle-alert.svg"),
            "icons/type.svg" => include_bytes!("../assets/icons/type.svg"),
            "icons/upload.svg" => include_bytes!("../assets/icons/upload.svg"),
            "icons/users-round.svg" => include_bytes!("../assets/icons/users-round.svg"),
            "icons/users.svg" => include_bytes!("../assets/icons/users.svg"),
            "icons/x.svg" => include_bytes!("../assets/icons/x.svg"),
            "icons/zap.svg" => include_bytes!("../assets/icons/zap.svg"),
            _ => return gpui_kit::assets::Assets.load(path),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
