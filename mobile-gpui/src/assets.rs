//! gpui-kit's component icons plus the Lucide icons the app's own screens
//! use. gpui-kit embeds only its component set by default.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
  ExtraIcons,
  [
    CircleQuestionMark,
    CornerDownRight,
    ListIndentDecrease,
    ListIndentIncrease
  ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
  fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
    if let Some(bytes) = ExtraIcons.load(path)? {
      return Ok(Some(bytes));
    }
    gpui_kit::assets::Assets.load(path)
  }

  fn list(&self, path: &str) -> Result<Vec<SharedString>> {
    let mut paths = gpui_kit::assets::Assets.list(path)?;
    paths.extend(ExtraIcons.list(path)?);
    paths.sort();
    paths.dedup();
    Ok(paths)
  }
}
