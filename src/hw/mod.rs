/// Log GPU adapter details for hardware-acceleration diagnostics (Phase 8).
pub fn log_adapter_info(adapter: &wgpu::Adapter) {
    let info = adapter.get_info();
    log::info!(
        "GPU adapter: {} | backend={:?} | driver={} | device_type={:?}",
        info.name,
        info.backend,
        info.driver,
        info.device_type,
    );

    let features = adapter.features();
    if features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
        log::debug!("GPU supports BC texture compression");
    }
}