// Gaussian Blur - Horizontal Pass
// First pass of two-pass separable Gaussian blur
// Applies horizontal blur to the input texture

// Uniform data
struct Uniforms {
    // Blur radius in pixels (sigma)
    sigma: f32,
    // Image dimensions
    image_width: f32,
    image_height: f32,
    // Padding for alignment
    _padding: f32,
}

// Bindings
@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> uniforms: Uniforms;

// Calculate Gaussian weight
fn gaussian_weight(x: f32, sigma: f32) -> f32 {
    let pi = 3.14159265359;
    let coefficient = 1.0 / (sqrt(2.0 * pi) * sigma);
    let exponent = -(x * x) / (2.0 * sigma * sigma);
    return coefficient * exp(exponent);
}

// Compute shader - horizontal blur
@compute @workgroup_size(16, 16, 1)
fn cs_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pixel_coords = vec2<i32>(global_id.xy);
    let image_size = vec2<i32>(i32(uniforms.image_width), i32(uniforms.image_height));

    // Check bounds
    if (pixel_coords.x >= image_size.x || pixel_coords.y >= image_size.y) {
        return;
    }

    // Calculate blur kernel radius (3 sigma covers ~99.7% of Gaussian)
    let kernel_radius = i32(ceil(uniforms.sigma * 3.0));

    var sum = vec4<f32>(0.0);
    var weight_sum = 0.0;

    // Apply horizontal blur
    for (var offset = -kernel_radius; offset <= kernel_radius; offset++) {
        let sample_x = pixel_coords.x + offset;

        // Clamp to texture bounds
        if (sample_x < 0 || sample_x >= image_size.x) {
            continue;
        }

        let sample_coords = vec2<i32>(sample_x, pixel_coords.y);
        let tex_coords = vec2<f32>(sample_coords) / vec2<f32>(image_size);

        // Sample texture
        let sample_color = textureSample(input_texture, input_sampler, tex_coords);

        // Calculate Gaussian weight
        let weight = gaussian_weight(f32(offset), uniforms.sigma);

        sum += sample_color * weight;
        weight_sum += weight;
    }

    // Normalize by total weight
    let blurred_color = sum / weight_sum;

    // Write to output texture
    textureStore(output_texture, pixel_coords, blurred_color);
}
