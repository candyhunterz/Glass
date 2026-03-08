//! RectRenderer: instanced wgpu pipeline for colored rectangles.
//!
//! Used for cell backgrounds, cursor shape, and selection highlight quads.
//! Each rectangle is a `RectInstance` with pixel position and RGBA color.

use bytemuck::{Pod, Zeroable};

/// A single colored rectangle instance for GPU rendering.
///
/// Position and dimensions are in pixels; color is RGBA normalized [0..1].
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// [x, y, width, height] in pixels
    pub pos: [f32; 4],
    /// [r, g, b, a] normalized color
    pub color: [f32; 4],
}

/// Instanced wgpu render pipeline for drawing colored rectangles.
///
/// Used for cell backgrounds, cursor, and selection highlight. Each instance
/// is a single rectangle positioned in pixel coordinates and converted to NDC
/// via a viewport uniform.
pub struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_buffer_capacity: usize,
}

impl RectRenderer {
    /// Create the rect rendering pipeline.
    ///
    /// - `device`: wgpu device for resource creation
    /// - `surface_format`: texture format of the render target
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        // WGSL shader for instanced rectangle rendering
        let shader_src = r#"
struct Viewport {
    width: f32,
    height: f32,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

struct RectInstance {
    @location(0) pos: vec4<f32>,   // x, y, w, h in pixels
    @location(1) color: vec4<f32>, // RGBA normalized
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: RectInstance,
) -> VertexOutput {
    // Generate quad corners from vertex_index (0-5 for 2 triangles)
    // Triangle 1: 0,1,2  Triangle 2: 2,1,3
    // Map to corners: (0,0), (1,0), (0,1), (1,1)
    let corner_index = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let ci = corner_index[vertex_index];
    let cx = f32(ci & 1u);
    let cy = f32((ci >> 1u) & 1u);

    // Instance position in pixels
    let px = instance.pos.x + cx * instance.pos.z;
    let py = instance.pos.y + cy * instance.pos.w;

    // Convert pixels to NDC: x: [0, width] -> [-1, 1], y: [0, height] -> [1, -1]
    let ndc_x = (px / viewport.width) * 2.0 - 1.0;
    let ndc_y = 1.0 - (py / viewport.height) * 2.0;

    var out: VertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        // Uniform buffer for viewport resolution (2 x f32)
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect_viewport_uniform"),
            size: 8, // 2 x f32
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group layout + bind group
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        // Instance buffer layout: two vec4<f32> at instance rate
        let instance_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // pos: vec4<f32> at location 0
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                },
                // color: vec4<f32> at location 1
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 16,
                    shader_location: 1,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[instance_buffer_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Initial instance buffer (will grow as needed)
        let initial_capacity = 1024;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect_instance_buffer"),
            size: (initial_capacity * std::mem::size_of::<RectInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            instance_buffer,
            instance_buffer_capacity: initial_capacity,
        }
    }

    /// Upload instance data and viewport resolution to GPU buffers.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[RectInstance],
        width: u32,
        height: u32,
    ) {
        // Update viewport uniform
        let viewport_data = [width as f32, height as f32];
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&viewport_data),
        );

        if instances.is_empty() {
            return;
        }

        // Grow instance buffer if needed
        if instances.len() > self.instance_buffer_capacity {
            let new_capacity = instances.len().next_power_of_two();
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rect_instance_buffer"),
                size: (new_capacity * std::mem::size_of::<RectInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_buffer_capacity = new_capacity;
        }

        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
    }

    /// Draw instanced rectangles into the given render pass.
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>, instance_count: u32) {
        if instance_count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        render_pass.draw(0..6, 0..instance_count);
    }

    /// Draw a range of instanced rectangles into the given render pass.
    pub fn render_range<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        start: u32,
        end: u32,
    ) {
        if start >= end {
            return;
        }
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        render_pass.draw(0..6, start..end);
    }
}
