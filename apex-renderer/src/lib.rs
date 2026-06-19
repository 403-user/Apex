pub mod wgpu_renderer;
pub mod atlas;
pub mod glyph_key;
pub mod shaper;
pub mod font_manager;

use std::path::PathBuf;
use apex_config::ApexConfig;
use wgpu_renderer::WgpuRenderer;

pub async fn run_event_loop(config: ApexConfig, atlas_dump: Option<PathBuf>) -> anyhow::Result<()> {
    let mut renderer = WgpuRenderer::new(config, atlas_dump).await?;
    renderer.run().await?;
    Ok(())
}
