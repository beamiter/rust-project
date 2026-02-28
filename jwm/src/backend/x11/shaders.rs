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
in vec2 v_uv;
out vec4 frag_color;

void main() {
    frag_color = texture(u_texture, v_uv);
}
"#;
