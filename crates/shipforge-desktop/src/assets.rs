use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "brand.svg" => include_bytes!("../assets/brand.svg"),
            "icons/activity.svg" => include_bytes!("../assets/icons/activity.svg"),
            "icons/archive.svg" => include_bytes!("../assets/icons/archive.svg"),
            "icons/arrow-down-to-line.svg" => {
                include_bytes!("../assets/icons/arrow-down-to-line.svg")
            }
            "icons/arrow-left.svg" => include_bytes!("../assets/icons/arrow-left.svg"),
            "icons/arrow-right.svg" => include_bytes!("../assets/icons/arrow-right.svg"),
            "icons/arrow-up-from-line.svg" => {
                include_bytes!("../assets/icons/arrow-up-from-line.svg")
            }
            "icons/arrow-up-right.svg" => include_bytes!("../assets/icons/arrow-up-right.svg"),
            "icons/ban.svg" => include_bytes!("../assets/icons/ban.svg"),
            "icons/bell.svg" => include_bytes!("../assets/icons/bell.svg"),
            "icons/blocks.svg" => include_bytes!("../assets/icons/blocks.svg"),
            "icons/book-check.svg" => include_bytes!("../assets/icons/book-check.svg"),
            "icons/book-open.svg" => include_bytes!("../assets/icons/book-open.svg"),
            "icons/book-text.svg" => include_bytes!("../assets/icons/book-text.svg"),
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
            "icons/circle-arrow-right.svg" => {
                include_bytes!("../assets/icons/circle-arrow-right.svg")
            }
            "icons/circle-check-big.svg" => include_bytes!("../assets/icons/circle-check-big.svg"),
            "icons/circle-check.svg" => include_bytes!("../assets/icons/circle-check.svg"),
            "icons/circle-dashed.svg" => include_bytes!("../assets/icons/circle-dashed.svg"),
            "icons/circle-dot.svg" => include_bytes!("../assets/icons/circle-dot.svg"),
            "icons/circle-help.svg" => include_bytes!("../assets/icons/circle-help.svg"),
            "icons/circle-minus.svg" => include_bytes!("../assets/icons/circle-minus.svg"),
            "icons/circle-pause.svg" => include_bytes!("../assets/icons/circle-pause.svg"),
            "icons/circle-play.svg" => include_bytes!("../assets/icons/circle-play.svg"),
            "icons/circle.svg" => include_bytes!("../assets/icons/circle.svg"),
            "icons/clipboard-list.svg" => include_bytes!("../assets/icons/clipboard-list.svg"),
            "icons/clipboard.svg" => include_bytes!("../assets/icons/clipboard.svg"),
            "icons/clock-3.svg" => include_bytes!("../assets/icons/clock-3.svg"),
            "icons/clock.svg" => include_bytes!("../assets/icons/clock.svg"),
            "icons/cloud.svg" => include_bytes!("../assets/icons/cloud.svg"),
            "icons/columns-2.svg" => include_bytes!("../assets/icons/columns-2.svg"),
            "icons/columns-3.svg" => include_bytes!("../assets/icons/columns-3.svg"),
            "icons/command.svg" => include_bytes!("../assets/icons/command.svg"),
            "icons/compass.svg" => include_bytes!("../assets/icons/compass.svg"),
            "icons/copy.svg" => include_bytes!("../assets/icons/copy.svg"),
            "icons/database.svg" => include_bytes!("../assets/icons/database.svg"),
            "icons/diamond.svg" => include_bytes!("../assets/icons/diamond.svg"),
            "icons/dot.svg" => include_bytes!("../assets/icons/dot.svg"),
            "icons/download.svg" => include_bytes!("../assets/icons/download.svg"),
            "icons/ellipsis-vertical.svg" => {
                include_bytes!("../assets/icons/ellipsis-vertical.svg")
            }
            "icons/ellipsis.svg" => include_bytes!("../assets/icons/ellipsis.svg"),
            "icons/file-archive.svg" => include_bytes!("../assets/icons/file-archive.svg"),
            "icons/file-audio.svg" => include_bytes!("../assets/icons/file-audio.svg"),
            "icons/file-code.svg" => include_bytes!("../assets/icons/file-code.svg"),
            "icons/file-cog.svg" => include_bytes!("../assets/icons/file-cog.svg"),
            "icons/file-image.svg" => include_bytes!("../assets/icons/file-image.svg"),
            "icons/file-json.svg" => include_bytes!("../assets/icons/file-json.svg"),
            "icons/file-spreadsheet.svg" => include_bytes!("../assets/icons/file-spreadsheet.svg"),
            "icons/file-text.svg" => include_bytes!("../assets/icons/file-text.svg"),
            "icons/file-video.svg" => include_bytes!("../assets/icons/file-video.svg"),
            "icons/flag.svg" => include_bytes!("../assets/icons/flag.svg"),
            "icons/folder-kanban.svg" => include_bytes!("../assets/icons/folder-kanban.svg"),
            "icons/folder-open.svg" => include_bytes!("../assets/icons/folder-open.svg"),
            "icons/folder.svg" => include_bytes!("../assets/icons/folder.svg"),
            "icons/folders.svg" => include_bytes!("../assets/icons/folders.svg"),
            "icons/funnel.svg" => include_bytes!("../assets/icons/funnel.svg"),
            "icons/git-branch.svg" => include_bytes!("../assets/icons/git-branch.svg"),
            "icons/globe.svg" => include_bytes!("../assets/icons/globe.svg"),
            "icons/grip-vertical.svg" => include_bytes!("../assets/icons/grip-vertical.svg"),
            "icons/hammer.svg" => include_bytes!("../assets/icons/hammer.svg"),
            "icons/hard-drive.svg" => include_bytes!("../assets/icons/hard-drive.svg"),
            "icons/house.svg" => include_bytes!("../assets/icons/house.svg"),
            "icons/inbox.svg" => include_bytes!("../assets/icons/inbox.svg"),
            "icons/key-round.svg" => include_bytes!("../assets/icons/key-round.svg"),
            "icons/layers.svg" => include_bytes!("../assets/icons/layers.svg"),
            "icons/layout-dashboard.svg" => include_bytes!("../assets/icons/layout-dashboard.svg"),
            "icons/layout-grid.svg" => include_bytes!("../assets/icons/layout-grid.svg"),
            "icons/link-2.svg" => include_bytes!("../assets/icons/link-2.svg"),
            "icons/list-checks.svg" => include_bytes!("../assets/icons/list-checks.svg"),
            "icons/list-filter.svg" => include_bytes!("../assets/icons/list-filter.svg"),
            "icons/list-todo.svg" => include_bytes!("../assets/icons/list-todo.svg"),
            "icons/list-tree.svg" => include_bytes!("../assets/icons/list-tree.svg"),
            "icons/list.svg" => include_bytes!("../assets/icons/list.svg"),
            "icons/lock.svg" => include_bytes!("../assets/icons/lock.svg"),
            "icons/log-in.svg" => include_bytes!("../assets/icons/log-in.svg"),
            "icons/mail.svg" => include_bytes!("../assets/icons/mail.svg"),
            "icons/menu.svg" => include_bytes!("../assets/icons/menu.svg"),
            "icons/messages-square.svg" => include_bytes!("../assets/icons/messages-square.svg"),
            "icons/monitor.svg" => include_bytes!("../assets/icons/monitor.svg"),
            "icons/network.svg" => include_bytes!("../assets/icons/network.svg"),
            "icons/panel-left-close.svg" => include_bytes!("../assets/icons/panel-left-close.svg"),
            "icons/panel-left-open.svg" => include_bytes!("../assets/icons/panel-left-open.svg"),
            "icons/panel-left.svg" => include_bytes!("../assets/icons/panel-left.svg"),
            "icons/panel-top.svg" => include_bytes!("../assets/icons/panel-top.svg"),
            "icons/panels-top-left.svg" => include_bytes!("../assets/icons/panels-top-left.svg"),
            "icons/pause.svg" => include_bytes!("../assets/icons/pause.svg"),
            "icons/play.svg" => include_bytes!("../assets/icons/play.svg"),
            "icons/plug.svg" => include_bytes!("../assets/icons/plug.svg"),
            "icons/plus.svg" => include_bytes!("../assets/icons/plus.svg"),
            "icons/puzzle.svg" => include_bytes!("../assets/icons/puzzle.svg"),
            "icons/rotate-cw.svg" => include_bytes!("../assets/icons/rotate-cw.svg"),
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/send.svg" => include_bytes!("../assets/icons/send.svg"),
            "icons/server.svg" => include_bytes!("../assets/icons/server.svg"),
            "icons/settings-2.svg" => include_bytes!("../assets/icons/settings-2.svg"),
            "icons/settings.svg" => include_bytes!("../assets/icons/settings.svg"),
            "icons/shield-check.svg" => include_bytes!("../assets/icons/shield-check.svg"),
            "icons/signal-high.svg" => include_bytes!("../assets/icons/signal-high.svg"),
            "icons/signal-low.svg" => include_bytes!("../assets/icons/signal-low.svg"),
            "icons/signal-medium.svg" => include_bytes!("../assets/icons/signal-medium.svg"),
            "icons/sliders-horizontal.svg" => {
                include_bytes!("../assets/icons/sliders-horizontal.svg")
            }
            "icons/square-check.svg" => include_bytes!("../assets/icons/square-check.svg"),
            "icons/square-dashed-kanban.svg" => {
                include_bytes!("../assets/icons/square-dashed-kanban.svg")
            }
            "icons/square-dot.svg" => include_bytes!("../assets/icons/square-dot.svg"),
            "icons/square-minus.svg" => include_bytes!("../assets/icons/square-minus.svg"),
            "icons/square-pause.svg" => include_bytes!("../assets/icons/square-pause.svg"),
            "icons/square-pen.svg" => include_bytes!("../assets/icons/square-pen.svg"),
            "icons/square.svg" => include_bytes!("../assets/icons/square.svg"),
            "icons/sticky-note.svg" => include_bytes!("../assets/icons/sticky-note.svg"),
            "icons/tag.svg" => include_bytes!("../assets/icons/tag.svg"),
            "icons/tags.svg" => include_bytes!("../assets/icons/tags.svg"),
            "icons/target.svg" => include_bytes!("../assets/icons/target.svg"),
            "icons/terminal.svg" => include_bytes!("../assets/icons/terminal.svg"),
            "icons/timer.svg" => include_bytes!("../assets/icons/timer.svg"),
            "icons/triangle-alert.svg" => include_bytes!("../assets/icons/triangle-alert.svg"),
            "icons/type.svg" => include_bytes!("../assets/icons/type.svg"),
            "icons/upload.svg" => include_bytes!("../assets/icons/upload.svg"),
            "icons/user-round.svg" => include_bytes!("../assets/icons/user-round.svg"),
            "icons/users-round.svg" => include_bytes!("../assets/icons/users-round.svg"),
            "icons/users.svg" => include_bytes!("../assets/icons/users.svg"),
            "icons/workflow.svg" => include_bytes!("../assets/icons/workflow.svg"),
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
