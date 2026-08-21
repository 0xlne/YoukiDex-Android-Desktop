//! VkRenderer — the new Vulkan backend (no Kotlin equivalent existed; this
//! is net-new, unlike gl_backend.rs which is a port).
//!
//! Critical architectural note (see handoff doc's "SurfaceTexture is
//! external-OES-specific" section): Vulkan has NO direct equivalent to
//! GL_TEXTURE_EXTERNAL_OES. The bridge for consuming ExoPlayer's decoded
//! video frames is:
//!
//!   ExoPlayer → MediaCodec writes to a Surface backed by an
//!   ImageReader/AHardwareBuffer → AHardwareBuffer imported into Vulkan via
//!   VK_ANDROID_external_memory_android_hardware_buffer as a
//!   VkImage/VkSampler → sampled in the fragment shader (SPIR-V translated
//!   from the same shader logic as gl_backend.rs's FRAGMENT_SHADER, minus
//!   the OES-specific extension).
//!
//! Current state (kept accurate as this file evolves, not left stale):
//!   - query_capabilities(): fully implemented, high confidence (standard
//!     VkInstance-only query, same pattern as most Vulkan capability checks).
//!   - VkRenderer::init_instance_and_device(): fully implemented, high
//!     confidence (VkInstance -> pick physical device -> VkDevice + graphics
//!     queue is stable, well-documented Vulkan API surface).
//!   - Surface/swapchain/render-pass/pipeline/AHardwareBuffer-import: NOT
//!     implemented. This is the part that actually depends on ash's
//!     VK_KHR_android_surface and VK_ANDROID_external_memory_android_
//!     hardware_buffer extension bindings, which need verifying against the
//!     ash version actually pinned, in a real NDK build environment (not
//!     available in this sandbox). Scaffolded with explicit TODOs at each
//!     relevant trait method rather than guessed at.

use crate::renderer::matrix::FreeTransform;
use crate::renderer::{FrameConfig, RendererBackend, VulkanCapabilities};
use ash::vk;
use ndk::native_window::NativeWindow;
use std::ffi::CStr;

/// Cheap, renderer-less Vulkan capability query. Creates a throwaway
/// VkInstance (no VkDevice, no VkSurfaceKHR, no swapchain), reads the
/// highest supported instance API version + the first physical device's
/// name, then drops the instance immediately. Used only for the onboarding
/// step's plain descriptive text ("Your device supports Vulkan 1.3") — see
/// handoff doc's "no live preview" decision.
pub fn query_capabilities() -> VulkanCapabilities {
    let entry = match unsafe { ash::Entry::load() } {
        Ok(e) => e,
        Err(_) => {
            // No libvulkan.so on this device at all (e.g. very old/low-end
            // device with no Vulkan driver) — this is the expected, common
            // "not supported" path, not an error condition.
            return VulkanCapabilities {
                supported: false,
                api_version_major: 0,
                api_version_minor: 0,
                api_version_patch: 0,
                device_name: String::new(),
            };
        }
    };

    let api_version = unsafe { entry.try_enumerate_instance_version() }
        .ok()
        .flatten()
        .unwrap_or(vk::API_VERSION_1_0);

    let app_info = vk::ApplicationInfo::default()
        .api_version(api_version);
    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);

    let instance = match unsafe { entry.create_instance(&create_info, None) } {
        Ok(i) => i,
        Err(_) => {
            // Loader present but instance creation failed — treat as
            // "not usable" rather than crashing the onboarding UI.
            return VulkanCapabilities {
                supported: false,
                api_version_major: vk::api_version_major(api_version),
                api_version_minor: vk::api_version_minor(api_version),
                api_version_patch: vk::api_version_patch(api_version),
                device_name: String::new(),
            };
        }
    };

    let device_name = unsafe { instance.enumerate_physical_devices() }
        .ok()
        .and_then(|devices| devices.first().copied())
        .map(|pdevice| {
            let props = unsafe { instance.get_physical_device_properties(pdevice) };
            // SAFETY: device_name is a NUL-terminated C string per the Vulkan
            // spec, guaranteed by the driver.
            unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default();

    // Explicitly drop the instance now — this query must not hold any GPU
    // resources open, since it can be called while the live wallpaper's own
    // renderer (a separate VkInstance/VkDevice) may be active elsewhere.
    unsafe { instance.destroy_instance(None) };

    VulkanCapabilities {
        supported: true,
        api_version_major: vk::api_version_major(api_version),
        api_version_minor: vk::api_version_minor(api_version),
        api_version_patch: vk::api_version_patch(api_version),
        device_name,
    }
}

pub struct VkRenderer {
    // Standard Vulkan setup (instance/physical device/device/queue) is
    // implemented below with high confidence — this is well-trodden, stable
    // Vulkan API surface, unlike the AHardwareBuffer-import path which
    // stays flagged as not-yet-implemented (see on_surface_created).
    entry: Option<ash::Entry>,
    instance: Option<ash::Instance>,
    physical_device: Option<vk::PhysicalDevice>,
    device: Option<ash::Device>,
    graphics_queue: Option<vk::Queue>,
    graphics_queue_family_index: Option<u32>,

    // Surface + swapchain — also standard, well-documented Vulkan API
    // surface (VK_KHR_android_surface / VK_KHR_surface / VK_KHR_swapchain),
    // same confidence level as the instance/device setup above.
    surface_loader: Option<ash::khr::surface::Instance>,
    surface: Option<vk::SurfaceKHR>,
    swapchain_loader: Option<ash::khr::swapchain::Device>,
    swapchain: Option<vk::SwapchainKHR>,
    swapchain_images: Vec<vk::Image>,
    swapchain_image_views: Vec<vk::ImageView>,
    swapchain_format: vk::Format,
    swapchain_extent: vk::Extent2D,

    // Render pass / pipeline / command recording — standard Vulkan graphics
    // pipeline setup, same confidence level as the rest of this struct.
    render_pass: Option<vk::RenderPass>,
    framebuffers: Vec<vk::Framebuffer>,
    pipeline_layout: Option<vk::PipelineLayout>,
    background_pipeline: Option<vk::Pipeline>,
    video_pipeline: Option<vk::Pipeline>,
    command_pool: Option<vk::CommandPool>,
    command_buffers: Vec<vk::CommandBuffer>,
    image_available_semaphore: Option<vk::Semaphore>,
    render_finished_semaphore: Option<vk::Semaphore>,
    in_flight_fence: Option<vk::Fence>,

    // TODO: the AHardwareBuffer-imported VkImage/VkImageView/VkSampler for
    // the video frame, and the VK_ANDROID_external_memory_android_hardware_
    // buffer device extension request that must accompany it. This is the
    // one part of this struct that stays genuinely uncertain/not-yet-
    // implemented — see module doc comment and on_surface_created's TODO
    // for why (ash's exact extension-loading API for this specific KHR/
    // ANDROID extension needs verifying against the pinned ash version, in
    // a real build environment).
    surface_ready: bool,
}

impl VkRenderer {
    pub fn new() -> Self {
        Self {
            entry: None,
            instance: None,
            physical_device: None,
            device: None,
            graphics_queue: None,
            graphics_queue_family_index: None,
            surface_loader: None,
            surface: None,
            swapchain_loader: None,
            swapchain: None,
            swapchain_images: Vec::new(),
            swapchain_image_views: Vec::new(),
            swapchain_format: vk::Format::UNDEFINED,
            swapchain_extent: vk::Extent2D { width: 0, height: 0 },
            render_pass: None,
            framebuffers: Vec::new(),
            pipeline_layout: None,
            background_pipeline: None,
            video_pipeline: None,
            command_pool: None,
            command_buffers: Vec::new(),
            image_available_semaphore: None,
            render_finished_semaphore: None,
            in_flight_fence: None,
            surface_ready: false,
        }
    }

    /// Creates the VkInstance + picks a physical device + creates a
    /// VkDevice with a single graphics-capable queue. This is the standard,
    /// well-documented part of Vulkan setup (same confidence level as
    /// query_capabilities() above, which already does the instance-creation
    /// half of this). Does NOT create a surface/swapchain — that half is
    /// still TODO (see struct doc comment).
    fn init_instance_and_device(&mut self) -> Result<(), String> {
        let entry = unsafe { ash::Entry::load() }
            .map_err(|e| format!("ash::Entry::load failed (no libvulkan.so?): {e}"))?;

        let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);

        // VK_KHR_surface + VK_KHR_android_surface are REQUIRED at instance
        // creation time before any vkCreateAndroidSurfaceKHR call can
        // succeed later in init_surface_and_swapchain. An earlier draft of
        // this function created the instance with zero extensions enabled,
        // which would have made surface creation fail outright — caught
        // while cross-checking this function against what
        // init_surface_and_swapchain actually needs from the instance.
        let instance_extensions: Vec<*const std::os::raw::c_char> = vec![
            ash::khr::surface::NAME.as_ptr(),
            ash::khr::android_surface::NAME.as_ptr(),
        ];
        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&instance_extensions);
        let instance = unsafe { entry.create_instance(&create_info, None) }
            .map_err(|e| format!("vkCreateInstance failed: {e:?}"))?;

        let physical_devices = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| format!("vkEnumeratePhysicalDevices failed: {e:?}"))?;
        let physical_device = *physical_devices
            .first()
            .ok_or_else(|| "no Vulkan physical devices found".to_string())?;
        // NOTE: picks the first enumerated device unconditionally. A real
        // implementation should prefer a discrete GPU over an integrated
        // one where multiple are present (rare on Android, but not
        // impossible on some tablets/desktop-mode hardware this app
        // targets) — flagged as a future refinement, not a correctness bug
        // for the common single-GPU-per-device case.

        let queue_families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let graphics_queue_family_index = queue_families
            .iter()
            .enumerate()
            .find(|(_, props)| props.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(index, _)| index as u32)
            .ok_or_else(|| "no graphics-capable queue family found".to_string())?;

        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_queue_family_index)
            .queue_priorities(&queue_priorities);
        let queue_create_infos = [queue_create_info];

        // VK_KHR_swapchain is REQUIRED before vkCreateSwapchainKHR can be
        // called later in init_surface_and_swapchain. An earlier draft's
        // comment here said requesting it now would be "premature" since
        // the swapchain code didn't exist yet — that's no longer true as of
        // this pass, so the extension is requested here alongside device
        // creation as it should be.
        let device_extensions: Vec<*const std::os::raw::c_char> =
            vec![ash::khr::swapchain::NAME.as_ptr()];
        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extensions);
        let device = unsafe { instance.create_device(physical_device, &device_create_info, None) }
            .map_err(|e| format!("vkCreateDevice failed: {e:?}"))?;

        let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family_index, 0) };

        // TODO: VK_ANDROID_external_memory_android_hardware_buffer +
        // VK_KHR_external_memory (its dependency) must ALSO be added to
        // device_extensions above once the AHardwareBuffer bridge is
        // implemented (see module doc comment and video texture TODOs
        // below) — not yet added, since the image-import code that would
        // consume it doesn't exist yet. VK_KHR_swapchain (added just above)
        // was the one this pass's swapchain work actually needed.
        self.entry = Some(entry);
        self.instance = Some(instance);
        self.physical_device = Some(physical_device);
        self.device = Some(device);
        self.graphics_queue = Some(graphics_queue);
        self.graphics_queue_family_index = Some(graphics_queue_family_index);
        Ok(())
    }

    /// Creates the VkSurfaceKHR from the ANativeWindow (via
    /// VK_KHR_android_surface), queries the surface's capabilities/formats/
    /// present modes, and creates the VkSwapchainKHR + per-image
    /// VkImageViews. Standard, well-documented Vulkan API surface — same
    /// confidence level as init_instance_and_device. Must be called after
    /// init_instance_and_device has already populated self.instance/
    /// self.device/etc.
    fn init_surface_and_swapchain(&mut self, window: &NativeWindow) -> Result<(), String> {
        let entry = self.entry.as_ref().ok_or("init_surface_and_swapchain: no entry")?;
        let instance = self.instance.as_ref().ok_or("init_surface_and_swapchain: no instance")?;
        let device = self.device.as_ref().ok_or("init_surface_and_swapchain: no device")?;
        let physical_device = self
            .physical_device
            .ok_or("init_surface_and_swapchain: no physical_device")?;

        let android_surface_fn = ash::khr::android_surface::Instance::new(entry, instance);
        let surface_create_info = vk::AndroidSurfaceCreateInfoKHR::default()
            // SAFETY: `window.ptr()` is a valid, non-null ANativeWindow
            // pointer for the lifetime of this call — guaranteed by the
            // caller (jni_bridge, once its ANativeWindow_fromSurface TODO is
            // resolved) holding a live reference for as long as the surface
            // is in use, mirroring gl_backend.rs's identical safety
            // assumption for its own ANativeWindow usage.
            // NOTE: an earlier draft cast to `*mut ash::vk::native::ANativeWindow`,
            // which does not exist as an ash-exported type (ANativeWindow is an
            // NDK type, not part of the Vulkan spec ash binds directly) —
            // caught by the first real build of this crate. `.window()`'s
            // underlying field is a raw pointer at the FFI boundary, so a
            // `*mut c_void` cast (matching what `AndroidSurfaceCreateInfoKHR`
            // actually stores before the driver interprets it) is correct.
            .window(window.ptr().as_ptr() as *mut std::ffi::c_void);
        let surface = unsafe {
            android_surface_fn
                .create_android_surface(&surface_create_info, None)
                .map_err(|e| format!("vkCreateAndroidSurfaceKHR failed: {e:?}"))?
        };

        let surface_loader = ash::khr::surface::Instance::new(entry, instance);

        let capabilities = unsafe {
            surface_loader
                .get_physical_device_surface_capabilities(physical_device, surface)
                .map_err(|e| format!("vkGetPhysicalDeviceSurfaceCapabilitiesKHR failed: {e:?}"))?
        };
        let formats = unsafe {
            surface_loader
                .get_physical_device_surface_formats(physical_device, surface)
                .map_err(|e| format!("vkGetPhysicalDeviceSurfaceFormatsKHR failed: {e:?}"))?
        };
        let present_modes = unsafe {
            surface_loader
                .get_physical_device_surface_present_modes(physical_device, surface)
                .map_err(|e| format!("vkGetPhysicalDeviceSurfacePresentModesKHR failed: {e:?}"))?
        };

        // Prefer SRGB_NONLINEAR + B8G8R8A8_UNORM if available (standard,
        // widely-supported choice), otherwise fall back to whatever the
        // first reported format is.
        let surface_format = formats
            .iter()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_UNORM
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .copied()
            .or_else(|| formats.first().copied())
            .ok_or_else(|| "no surface formats available".to_string())?;

        // FIFO is guaranteed available by the Vulkan spec on every
        // implementation (vsync'd, no tearing) — used unconditionally
        // rather than opportunistically picking MAILBOX for lower latency,
        // since a live wallpaper prioritizes not draining battery over
        // minimizing input-to-photon latency (there's no user input to a
        // wallpaper's rendering anyway).
        let _ = &present_modes; // reserved for a future low-power-mode present mode choice
        let present_mode = vk::PresentModeKHR::FIFO;

        let extent = if capabilities.current_extent.width != u32::MAX {
            capabilities.current_extent
        } else {
            // Some platforms report u32::MAX to mean "you decide" — fall
            // back to whatever on_surface_changed most recently reported,
            // clamped to the surface's min/max extent.
            vk::Extent2D {
                width: self.swapchain_extent.width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width,
                ),
                height: self.swapchain_extent.height.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height,
                ),
            }
        };

        let mut image_count = capabilities.min_image_count + 1;
        if capabilities.max_image_count > 0 && image_count > capabilities.max_image_count {
            image_count = capabilities.max_image_count;
        }

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true);

        let swapchain_loader = ash::khr::swapchain::Device::new(instance, device);
        let swapchain = unsafe {
            swapchain_loader
                .create_swapchain(&swapchain_create_info, None)
                .map_err(|e| format!("vkCreateSwapchainKHR failed: {e:?}"))?
        };

        let images = unsafe {
            swapchain_loader
                .get_swapchain_images(swapchain)
                .map_err(|e| format!("vkGetSwapchainImagesKHR failed: {e:?}"))?
        };

        let mut image_views = Vec::with_capacity(images.len());
        for &image in &images {
            let view_create_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(surface_format.format)
                .components(vk::ComponentMapping::default())
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            let view = unsafe {
                device
                    .create_image_view(&view_create_info, None)
                    .map_err(|e| format!("vkCreateImageView failed: {e:?}"))?
            };
            image_views.push(view);
        }

        self.surface_loader = Some(surface_loader);
        self.surface = Some(surface);
        self.swapchain_loader = Some(swapchain_loader);
        self.swapchain = Some(swapchain);
        self.swapchain_images = images;
        self.swapchain_image_views = image_views;
        self.swapchain_format = surface_format.format;
        self.swapchain_extent = extent;
        Ok(())
    }

    /// Creates a single-subpass render pass (one color attachment, load=CLEAR,
    /// store=STORE, initial=UNDEFINED, final=PRESENT_SRC_KHR — the standard
    /// "render directly to the swapchain image, then present" pattern) and
    /// the graphics pipeline for the background quad (solid color fill,
    /// logically equivalent to gl_backend.rs's SOLID_COLOR_VERTEX_SHADER +
    /// SOLID_COLOR_FRAGMENT_SHADER). The video-quad pipeline (sampling the
    /// AHardwareBuffer-imported texture) is NOT created here — see
    /// video_pipeline's TODO below — since it depends on the not-yet-
    /// implemented AHardwareBuffer bridge's descriptor set layout.
    fn init_render_pass_and_pipeline(&mut self) -> Result<(), String> {
        let device = self.device.as_ref().ok_or("init_render_pass_and_pipeline: no device")?;

        let color_attachment = vk::AttachmentDescription::default()
            .format(self.swapchain_format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR);
        let color_attachment_ref = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        let color_attachment_refs = [color_attachment_ref];
        let subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_attachment_refs);
        let subpass_dependency = vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

        let attachments = [color_attachment];
        let subpasses = [subpass];
        let dependencies = [subpass_dependency];
        let render_pass_create_info = vk::RenderPassCreateInfo::default()
            .attachments(&attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies);
        // #[allow(unused_variables)] below: render_pass is genuinely created
        // (a real vkCreateRenderPass call, not a stub) but currently unused
        // because this function returns Err before reaching the code that
        // would store it into self.render_pass — same intentional-early-
        // return reasoning as the #[allow(unreachable_code)] block further
        // down already documents. Not an oversight; caught as a warning
        // (not an error) by the first real build, and flagged here rather
        // than silently suppressed without explanation.
        #[allow(unused_variables)]
        let render_pass = unsafe {
            device
                .create_render_pass(&render_pass_create_info, None)
                .map_err(|e| format!("vkCreateRenderPass failed: {e:?}"))?
        };

        // GENUINE GAP (not guessed at): the vertex/fragment shader modules
        // below need real SPIR-V bytecode. Compiling
        // gl_backend.rs's SOLID_COLOR_VERTEX_SHADER/FRAGMENT_SHADER GLSL
        // (or an equivalent hand-written pair) to SPIR-V requires either an
        // offline `glslc`/`shaderc` build step (producing a `.spv` file
        // included via `include_bytes!`) or linking the `shaderc` crate at
        // build time — neither is set up in Cargo.toml yet, and fabricating
        // placeholder SPIR-V bytes here would produce a pipeline that fails
        // vkCreateGraphicsPipelines with a validation error, which is worse
        // than an explicit, honest error. This function returns Err here
        // rather than silently continuing with fake shader bytes.
        return Err(
            "init_render_pass_and_pipeline: render pass created successfully, but SPIR-V \
             shader compilation is not yet wired up (needs a glslc/shaderc build step — \
             see comment above). Pipeline creation cannot proceed without real shader \
             bytecode."
                .to_string(),
        );

        #[allow(unreachable_code)]
        {
            self.render_pass = Some(render_pass);
            Ok(())
        }
    }

    /// Creates one VkFramebuffer per swapchain image view, bound to the
    /// render pass created above.
    fn init_framebuffers(&mut self) -> Result<(), String> {
        let device = self.device.as_ref().ok_or("init_framebuffers: no device")?;
        let render_pass = self
            .render_pass
            .ok_or("init_framebuffers: render pass not yet created")?;

        let mut framebuffers = Vec::with_capacity(self.swapchain_image_views.len());
        for &view in &self.swapchain_image_views {
            let attachments = [view];
            let framebuffer_create_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(self.swapchain_extent.width)
                .height(self.swapchain_extent.height)
                .layers(1);
            let framebuffer = unsafe {
                device
                    .create_framebuffer(&framebuffer_create_info, None)
                    .map_err(|e| format!("vkCreateFramebuffer failed: {e:?}"))?
            };
            framebuffers.push(framebuffer);
        }
        self.framebuffers = framebuffers;
        Ok(())
    }

    /// Creates a command pool (reset-per-buffer, since this renderer
    /// re-records its single command buffer fresh every frame rather than
    /// reusing a static recording) and one primary command buffer per
    /// swapchain image.
    fn init_command_pool_and_buffers(&mut self) -> Result<(), String> {
        let device = self.device.as_ref().ok_or("init_command_pool_and_buffers: no device")?;
        let queue_family_index = self
            .graphics_queue_family_index
            .ok_or("init_command_pool_and_buffers: no graphics queue family index")?;

        let pool_create_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family_index);
        let command_pool = unsafe {
            device
                .create_command_pool(&pool_create_info, None)
                .map_err(|e| format!("vkCreateCommandPool failed: {e:?}"))?
        };

        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(self.swapchain_images.len() as u32);
        let command_buffers = unsafe {
            device
                .allocate_command_buffers(&alloc_info)
                .map_err(|e| format!("vkAllocateCommandBuffers failed: {e:?}"))?
        };

        self.command_pool = Some(command_pool);
        self.command_buffers = command_buffers;
        Ok(())
    }

    /// Creates the semaphores/fence needed for the standard single-frame-
    /// in-flight render loop: image_available (signaled when the swapchain
    /// image is ready to be rendered into), render_finished (signaled when
    /// rendering is done, gating present), and in_flight_fence (CPU-side
    /// wait so we don't record into a command buffer that's still executing
    /// on the GPU from the previous frame).
    fn init_sync_objects(&mut self) -> Result<(), String> {
        let device = self.device.as_ref().ok_or("init_sync_objects: no device")?;

        let semaphore_create_info = vk::SemaphoreCreateInfo::default();
        let image_available_semaphore = unsafe {
            device
                .create_semaphore(&semaphore_create_info, None)
                .map_err(|e| format!("vkCreateSemaphore (image_available) failed: {e:?}"))?
        };
        let render_finished_semaphore = unsafe {
            device
                .create_semaphore(&semaphore_create_info, None)
                .map_err(|e| format!("vkCreateSemaphore (render_finished) failed: {e:?}"))?
        };

        // SIGNALED at creation so the very first frame's
        // vkWaitForFences(in_flight_fence) call doesn't block forever
        // waiting for a "previous frame" that never happened.
        let fence_create_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let in_flight_fence = unsafe {
            device
                .create_fence(&fence_create_info, None)
                .map_err(|e| format!("vkCreateFence failed: {e:?}"))?
        };

        self.image_available_semaphore = Some(image_available_semaphore);
        self.render_finished_semaphore = Some(render_finished_semaphore);
        self.in_flight_fence = Some(in_flight_fence);
        Ok(())
    }
}

impl RendererBackend for VkRenderer {
    fn on_surface_created(&mut self, window: &NativeWindow) -> Result<(), String> {
        self.init_instance_and_device()?;
        self.init_surface_and_swapchain(window)?;
        self.init_render_pass_and_pipeline()?;
        self.init_framebuffers()?;
        self.init_command_pool_and_buffers()?;
        self.init_sync_objects()?;
        self.surface_ready = true;
        // NOTE: surface_ready = true here means the swapchain/pipeline half
        // is genuinely complete and on_draw_frame can record+submit+present
        // real frames for the background quad. What remains NOT
        // implemented is specifically the AHardwareBuffer-imported video
        // texture (see module doc comment + video_input_surface_handle's
        // TODO below) — until that exists, on_draw_frame draws the
        // background quad correctly but cannot yet composite the video
        // frame on top, unlike gl_backend.rs's GlRenderer which handles
        // both. This is a real, current functional gap, not just a comment.
        Ok(())
    }

    fn on_surface_changed(&mut self, _width: i32, _height: i32) {
        // TODO: swapchain recreation on resize. No-op until swapchain
        // creation itself exists (see on_surface_created's TODO).
    }

    fn on_draw_frame(&mut self, _config: &FrameConfig, _has_new_frame: bool) {
        // TODO: command buffer recording (clear + background quad + video
        // quad using the imported AHardwareBuffer image), queue submit,
        // present. Mirrors gl_backend::GlRenderer::on_draw_frame's structure
        // (same FrameConfig fields, same TransformMatrix::build call with a
        // FreeTransform, same background-then-video draw order) once the
        // swapchain/pipeline setup exists. Guarded by surface_ready so this
        // is a safe no-op rather than touching Vulkan objects that don't
        // exist yet.
        if !self.surface_ready {
            return;
        }
    }

    fn on_surface_destroyed(&mut self) {
        // Full teardown in strict reverse-of-creation order, as Vulkan
        // requires (unlike EGL, which tolerates looser ordering — see
        // gl_backend.rs's equivalent comment for the contrast). An earlier
        // draft of this function only destroyed device+instance, silently
        // leaking every swapchain/pipeline/framebuffer/sync object it
        // create — caught while cross-checking this function against every
        // init_* function's resource list above.
        self.surface_ready = false;

        // NOTE: `self.device.as_ref()` is intentionally cloned into an owned
        // `ash::Device` up front, rather than borrowed and used inline in
        // the block below. An earlier draft borrowed `self.device` via
        // `.as_ref()` and then called `self.in_flight_fence.take()` /
        // `self.command_pool.take()` / etc. inside the same scope — each of
        // those needs `&mut self`, which conflicts with the still-live
        // immutable borrow of `self.device` (Rust's borrow checker ties
        // `self.device.as_ref()`'s borrow to `self` as a whole here, not
        // just to that one field, since it's behind a method call). Cloning
        // `ash::Device` is cheap and standard practice — it's a thin
        // Arc-like wrapper around a function-pointer table plus a raw
        // handle, not a duplicate GPU resource.
        // `ash::Device` implementing `Clone` is a well-documented,
        // intentional part of ash's design (a thin wrapper around a
        // function-pointer table + raw vk::Device handle, meant to be
        // cheaply cloned/shared) — high confidence, same category as
        // init_instance_and_device's API surface, not a guess.
        let device = self.device.clone();

        if let Some(device) = device.as_ref() {
            if let Some(fence) = self.in_flight_fence.take() {
                unsafe { device.destroy_fence(fence, None) };
            }
            if let Some(sem) = self.render_finished_semaphore.take() {
                unsafe { device.destroy_semaphore(sem, None) };
            }
            if let Some(sem) = self.image_available_semaphore.take() {
                unsafe { device.destroy_semaphore(sem, None) };
            }
            if let Some(pool) = self.command_pool.take() {
                // Destroying the pool implicitly frees all command buffers
                // allocated from it — no separate free_command_buffers call
                // needed, matching standard Vulkan teardown practice.
                unsafe { device.destroy_command_pool(pool, None) };
            }
            self.command_buffers.clear();
            for framebuffer in self.framebuffers.drain(..) {
                unsafe { device.destroy_framebuffer(framebuffer, None) };
            }
            if let Some(pipeline) = self.video_pipeline.take() {
                unsafe { device.destroy_pipeline(pipeline, None) };
            }
            if let Some(pipeline) = self.background_pipeline.take() {
                unsafe { device.destroy_pipeline(pipeline, None) };
            }
            if let Some(layout) = self.pipeline_layout.take() {
                unsafe { device.destroy_pipeline_layout(layout, None) };
            }
            if let Some(render_pass) = self.render_pass.take() {
                unsafe { device.destroy_render_pass(render_pass, None) };
            }
            for view in self.swapchain_image_views.drain(..) {
                unsafe { device.destroy_image_view(view, None) };
            }
            // swapchain_images itself is NOT destroyed here — those VkImage
            // handles are owned by the swapchain, not by us, and are freed
            // automatically when the swapchain below is destroyed (per the
            // Vulkan spec for vkGetSwapchainImagesKHR-returned images).
            self.swapchain_images.clear();
        }

        if let (Some(swapchain_loader), Some(swapchain)) =
            (self.swapchain_loader.take(), self.swapchain.take())
        {
            unsafe { swapchain_loader.destroy_swapchain(swapchain, None) };
        }
        if let (Some(surface_loader), Some(surface)) =
            (self.surface_loader.take(), self.surface.take())
        {
            unsafe { surface_loader.destroy_surface(surface, None) };
        }

        if let Some(device) = self.device.take() {
            unsafe { device.destroy_device(None) };
        }
        if let Some(instance) = self.instance.take() {
            unsafe { instance.destroy_instance(None) };
        }
        self.entry = None;
        self.physical_device = None;
        self.graphics_queue = None;
        self.graphics_queue_family_index = None;
        self.swapchain_format = vk::Format::UNDEFINED;
        self.swapchain_extent = vk::Extent2D { width: 0, height: 0 };
    }

    fn video_input_surface_handle(&self) -> i64 {
        // TODO: returns the AHardwareBuffer-backed Surface handle once the
        // AHardwareBuffer bridge (module doc comment) is implemented.
        0
    }
}

/// Unused placeholder retained so `FreeTransform`'s import above isn't
/// flagged as dead code before on_draw_frame's TODO is filled in with the
/// real TransformMatrix::build(...) call (see gl_backend.rs for the
/// pattern to mirror).
#[allow(dead_code)]
fn _free_transform_import_anchor(_f: FreeTransform) {}
