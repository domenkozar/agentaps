use gpui::{AssetSource, Result, SharedString};
use gpui_component_assets::Assets;
use std::borrow::Cow;

pub(super) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "icons/mobile.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../assets/mobile.svg"
            ))));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = Assets.list(path)?;
        if "icons/mobile.svg".starts_with(path) {
            assets.push("icons/mobile.svg".into());
        }
        Ok(assets)
    }
}
