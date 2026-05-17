pub mod wgpu_renderer;
pub mod atlas;

use wgpu_renderer::WgpuRenderer;

pub async fn run_event_loop() -> anyhow::Result<()> {
    let mut renderer = WgpuRenderer::new().await?;
    renderer.run().await?;
    Ok(())
}
