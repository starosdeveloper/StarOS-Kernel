// SPDX-License-Identifier: MIT OR Apache-2.0
//! DMA Engine Subsystem

pub mod engine;
pub mod mapping;
pub mod pool;

pub use engine::{
    DmaDirection, DmaStatus, DmaChannel, DmaDevice, DmaDescriptor,
    DmaDeviceOps, ScatterGatherEntry, DmaDescriptorQueue, DmaCookieTracker,
    dma_caps,
    dma_async_device_register, dma_async_device_unregister,
    dmaengine_get, dmaengine_put,
    dma_find_channel, dma_request_channel, dma_release_channel,
};

pub use mapping::{
    DmaAddr, DmaDataDirection, DmaAllocation, dma_attrs,
    dma_alloc_coherent, dma_free_coherent,
    dma_alloc_attrs, dma_free_attrs,
    dma_map_single, dma_unmap_single,
    dma_map_single_attrs, dma_unmap_single_attrs,
    dma_sync_single_for_cpu, dma_sync_single_for_device,
    dmam_alloc_coherent, dmam_free_coherent, dmam_alloc_attrs,
    dma_get_mask, dma_set_mask, dma_set_coherent_mask,
};

pub use pool::{
    DmaPool,
    dma_pool_create, dma_pool_destroy,
    dma_pool_alloc, dma_pool_free,
};
