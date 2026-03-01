pub const VERTEX_SHADER: &str = r#"#version 330 core

uniform vec4 u_rect;       // x, y, w, h in pixels
uniform mat4 u_projection; // orthographic projection

out vec2 v_uv;

void main() {
    // Generate a fullscreen quad from gl_VertexID (0..3)
    vec2 pos = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    v_uv = vec2(pos.x, 1.0 - pos.y); // flip Y for texture
    vec2 pixel = u_rect.xy + pos * u_rect.zw;
    gl_Position = u_projection * vec4(pixel, 0.0, 1.0);
}
"#;

pub const FRAGMENT_SHADER: &str = r#"#version 330 core

uniform sampler2D u_texture;
uniform float u_opacity; // 1.0 for RGB windows (force opaque), negative to use texture alpha
in vec2 v_uv;
out vec4 frag_color;

void main() {
    vec4 texel = texture(u_texture, v_uv);
    // For RGB-only windows (24-bit depth) the alpha channel from TFP is
    // undefined (often 0), which makes the window invisible with
    // pre-multiplied alpha blending.  u_opacity >= 0 forces that value;
    // u_opacity < 0 means use the texture's own alpha (RGBA windows).
    float a = u_opacity >= 0.0 ? u_opacity : texel.a;
    frag_color = vec4(texel.rgb, a);
}
"#;
