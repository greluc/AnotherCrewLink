//! What renderer rungs this machine actually offers.
//!
//! §4.8 item 6 said the chain was "wgpu/DX12, then WARP through `force_fallback_adapter`,
//! then a CPU rasteriser". Two of those three are assertions about what Windows provides,
//! and the client has to pick between them at start-up on a machine nobody here has seen.
//! So this enumerates the adapters and prints what each one is.
//!
//! **What it found**, on Windows 11 x64 with a discrete card:
//!
//! ```text
//! RESULT adapters=3
//!   DiscreteGpu name="NVIDIA GeForce RTX 5090" driver="32.0.16.1656" backend=Dx12
//!   IntegratedGpu name="AMD Radeon(TM) Graphics" driver="32.0.21045.5002" backend=Dx12
//!   Cpu name="Microsoft Basic Render Driver" driver="10.0.26100.8972" backend=Dx12
//! ```
//!
//! The third is WARP, and its driver version is the operating system's build number rather
//! than a vendor's — which is what says it ships with Windows rather than with a card. It
//! is also the CPU rasteriser the plan asked for as a separate third rung, so there is no
//! third rung; see [`acl_ui::renderer`].
//!
//! DX12 only, on purpose. It is the backend §4.8 names and the only one Windows guarantees
//! — enabling Vulkan as well would list adapters the client will not use and make the
//! output look like more choice than there is.
//!
//! Run it with `cargo run -p gpu-probe --release`.

fn main() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    let instance = wgpu::Instance::new(descriptor);
    // `enumerate_adapters` is a future for the sake of WebGPU, where enumeration is
    // asynchronous. On a native backend it is ready when it is returned; blocking on it
    // is the whole of what this probe does before it prints.
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));

    println!("RESULT adapters={}", adapters.len());
    for adapter in &adapters {
        let info = adapter.get_info();
        println!(
            "  {:?} name={:?} driver={:?} backend={:?}",
            info.device_type, info.name, info.driver, info.backend
        );
    }

    // The question the chain turns on: is there a rung below the hardware one, and does
    // this machine have it? A `Cpu` adapter is WARP -- Windows's own D3D12 implementation,
    // running on the processor.
    let kinds: Vec<acl_ui::renderer::Adapter> = adapters
        .iter()
        .map(|adapter| match adapter.get_info().device_type {
            wgpu::DeviceType::Cpu => acl_ui::renderer::Adapter::Cpu,
            // Everything else is hardware as far as the choice goes. `VirtualGpu` is a
            // passed-through device in a virtual machine and `Other` is a driver that did
            // not say; neither is WARP, which is the only distinction the chain draws.
            _ => acl_ui::renderer::Adapter::Gpu,
        })
        .collect();

    for rung in acl_ui::renderer::chain(true) {
        let found = acl_ui::renderer::choose(rung, &kinds);
        println!("RUNG {rung:?} available={}", found.is_some());
    }
}
