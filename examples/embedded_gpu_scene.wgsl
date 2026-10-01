struct Frame { angle: vec4<f32> }
@group(0) @binding(0) var<uniform> frame: Frame;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32> }
const corners = array<vec3<f32>, 8>(
    vec3(-1., -1., -1.), vec3(1., -1., -1.), vec3(1., 1., -1.), vec3(-1., 1., -1.),
    vec3(-1., -1., 1.), vec3(1., -1., 1.), vec3(1., 1., 1.), vec3(-1., 1., 1.));
// Faces deliberately include hidden geometry: the depth buffer, not painter order,
// resolves which face is visible while the cube rotates.
const indices = array<u32, 36>(0,1,2, 0,2,3, 4,6,5, 4,7,6, 0,4,5, 0,5,1,
    3,2,6, 3,6,7, 1,5,6, 1,6,2, 0,3,7, 0,7,4);
const colors = array<vec3<f32>, 6>(vec3(.85,.25,.2), vec3(.2,.7,.85), vec3(.65,.35,.85),
    vec3(.3,.8,.5), vec3(.9,.65,.2), vec3(.3,.45,.85));
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> Vertex {
    let p = corners[indices[i]];
    let a = frame.angle.x;
    let yaw = vec3(cos(a)*p.x + sin(a)*p.z, p.y, -sin(a)*p.x + cos(a)*p.z);
    let pitch = vec3(yaw.x, cos(.55)*yaw.y - sin(.55)*yaw.z, sin(.55)*yaw.y + cos(.55)*yaw.z);
    let z = pitch.z + 5.;
    var out: Vertex;
    // Perspective projection, wgpu depth interval [0,1], near=.1, far=20.
    out.position = vec4(pitch.x * 2.5, pitch.y * 2.5, 20./19.9*z - 2./19.9, z);
    out.color = colors[i / 6u];
    return out;
}
@fragment fn fs_main(in: Vertex) -> @location(0) vec4<f32> { return vec4(in.color, 1.); }
