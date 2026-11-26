// NYAW: record decoders shared by both instance transports (`shaders_storage.wgsl` and `shaders_webgl.wgsl`).
// The scene primitives of vendor/gpui are tightly packed 32-bit words (`repr(C)`, 4-byte aligned), which WGSL
// struct layout (vec2 / mat2x2 aligned to 8 bytes) cannot express, so every record is read word by word in
// declaration order. Each transport provides `InstanceCursor`, `instance_cursor` and `read_word`.
// The word counts are checked against `size_of` in `wgpu_renderer.rs` (`record_sizes_match_shader_word_strides`).

const QUAD_WORDS: u32 = 56u;
const SHADOW_WORDS: u32 = 43u;
const PATH_VERTEX_WORDS: u32 = 26u;
const PATH_SPRITE_WORDS: u32 = 4u;
const UNDERLINE_WORDS: u32 = 25u;
const MONO_SPRITE_WORDS: u32 = 37u;
const POLY_SPRITE_WORDS: u32 = 33u;
// Word offset of `content_mask` in a monochrome (and subpixel) sprite.
const MONO_SPRITE_MASK_OFFSET: u32 = 6u;

fn read_f32(cursor: ptr<function, InstanceCursor>) -> f32 {
    return bitcast<f32>(read_word(cursor));
}

fn read_i32(cursor: ptr<function, InstanceCursor>) -> i32 {
    return bitcast<i32>(read_word(cursor));
}

fn read_vec2_f32(cursor: ptr<function, InstanceCursor>) -> vec2<f32> {
    let x = read_f32(cursor);
    let y = read_f32(cursor);
    return vec2<f32>(x, y);
}

fn read_vec2_i32(cursor: ptr<function, InstanceCursor>) -> vec2<i32> {
    let x = read_i32(cursor);
    let y = read_i32(cursor);
    return vec2<i32>(x, y);
}

fn read_hsla(cursor: ptr<function, InstanceCursor>) -> Hsla {
    let h = read_f32(cursor);
    let s = read_f32(cursor);
    let l = read_f32(cursor);
    let a = read_f32(cursor);
    return Hsla(h, s, l, a);
}

fn read_bounds(cursor: ptr<function, InstanceCursor>) -> Bounds {
    let origin = read_vec2_f32(cursor);
    let size = read_vec2_f32(cursor);
    return Bounds(origin, size);
}

fn read_corners(cursor: ptr<function, InstanceCursor>) -> Corners {
    let top_left = read_f32(cursor);
    let top_right = read_f32(cursor);
    let bottom_right = read_f32(cursor);
    let bottom_left = read_f32(cursor);
    return Corners(top_left, top_right, bottom_right, bottom_left);
}

fn read_edges(cursor: ptr<function, InstanceCursor>) -> Edges {
    let top = read_f32(cursor);
    let right = read_f32(cursor);
    let bottom = read_f32(cursor);
    let left = read_f32(cursor);
    return Edges(top, right, bottom, left);
}

fn read_content_mask(cursor: ptr<function, InstanceCursor>) -> ContentMask {
    let bounds = read_bounds(cursor);
    let round = read_bounds(cursor);
    let radii = read_corners(cursor);
    let exponent = read_f32(cursor);
    return ContentMask(bounds, round, radii, exponent);
}

fn read_color_stop(cursor: ptr<function, InstanceCursor>) -> LinearColorStop {
    let color = read_hsla(cursor);
    let percentage = read_f32(cursor);
    return LinearColorStop(color, percentage);
}

fn read_background(cursor: ptr<function, InstanceCursor>) -> Background {
    let tag = read_word(cursor);
    let color_space = read_word(cursor);
    let solid = read_hsla(cursor);
    let gradient_angle_or_pattern_height = read_f32(cursor);
    let color0 = read_color_stop(cursor);
    let color1 = read_color_stop(cursor);
    let pad = read_word(cursor);
    return Background(
        tag,
        color_space,
        solid,
        gradient_angle_or_pattern_height,
        array<LinearColorStop, 2>(color0, color1),
        pad,
    );
}

fn read_atlas_tile(cursor: ptr<function, InstanceCursor>) -> AtlasTile {
    let index = read_word(cursor);
    let kind = read_word(cursor);
    let tile_id = read_word(cursor);
    let padding = read_word(cursor);
    let origin = read_vec2_i32(cursor);
    let size = read_vec2_i32(cursor);
    return AtlasTile(AtlasTextureId(index, kind), tile_id, padding, AtlasBounds(origin, size));
}

fn read_transformation(cursor: ptr<function, InstanceCursor>) -> TransformationMatrix {
    let row0 = read_vec2_f32(cursor);
    let row1 = read_vec2_f32(cursor);
    let translation = read_vec2_f32(cursor);
    return TransformationMatrix(mat2x2<f32>(row0, row1), translation);
}

fn load_quad(instance_id: u32) -> Quad {
    var cursor = instance_cursor(instance_id * QUAD_WORDS);
    let order = read_word(&cursor);
    let border_style = read_word(&cursor);
    let bounds = read_bounds(&cursor);
    let content_mask = read_content_mask(&cursor);
    let background = read_background(&cursor);
    let border_color = read_hsla(&cursor);
    let corner_radii = read_corners(&cursor);
    let border_widths = read_edges(&cursor);
    let corner_exponent = read_f32(&cursor);
    let transformation = read_transformation(&cursor);
    return Quad(order, border_style, bounds, content_mask, background, border_color, corner_radii,
        border_widths, corner_exponent, transformation);
}

fn load_shadow(instance_id: u32) -> Shadow {
    var cursor = instance_cursor(instance_id * SHADOW_WORDS);
    let order = read_word(&cursor);
    let blur_radius = read_f32(&cursor);
    let bounds = read_bounds(&cursor);
    let corner_radii = read_corners(&cursor);
    let content_mask = read_content_mask(&cursor);
    let color = read_hsla(&cursor);
    let element_bounds = read_bounds(&cursor);
    let element_corner_radii = read_corners(&cursor);
    let inset = read_word(&cursor);
    let corner_exponent = read_f32(&cursor);
    let transformation = read_transformation(&cursor);
    return Shadow(order, blur_radius, bounds, corner_radii, content_mask, color, element_bounds,
        element_corner_radii, inset, corner_exponent, transformation);
}

fn load_path_vertex(vertex_id: u32) -> PathRasterizationVertex {
    var cursor = instance_cursor(vertex_id * PATH_VERTEX_WORDS);
    let xy_position = read_vec2_f32(&cursor);
    let st_position = read_vec2_f32(&cursor);
    let color = read_background(&cursor);
    let bounds = read_bounds(&cursor);
    return PathRasterizationVertex(xy_position, st_position, color, bounds);
}

fn load_path_sprite(instance_id: u32) -> PathSprite {
    var cursor = instance_cursor(instance_id * PATH_SPRITE_WORDS);
    return PathSprite(read_bounds(&cursor));
}

fn load_underline(instance_id: u32) -> Underline {
    var cursor = instance_cursor(instance_id * UNDERLINE_WORDS);
    let order = read_word(&cursor);
    let pad = read_word(&cursor);
    let bounds = read_bounds(&cursor);
    let content_mask = read_content_mask(&cursor);
    let color = read_hsla(&cursor);
    let thickness = read_f32(&cursor);
    let wavy = read_word(&cursor);
    return Underline(order, pad, bounds, content_mask, color, thickness, wavy);
}

fn load_mono_sprite(instance_id: u32) -> MonochromeSprite {
    var cursor = instance_cursor(instance_id * MONO_SPRITE_WORDS);
    let order = read_word(&cursor);
    let pad = read_word(&cursor);
    let bounds = read_bounds(&cursor);
    let content_mask = read_content_mask(&cursor);
    let color = read_hsla(&cursor);
    let tile = read_atlas_tile(&cursor);
    let transformation = read_transformation(&cursor);
    return MonochromeSprite(order, pad, bounds, content_mask, color, tile, transformation);
}

// Only the content mask of a monochrome (or subpixel, same layout) sprite, for the fragment shader.
fn load_mono_sprite_mask(instance_id: u32) -> ContentMask {
    var cursor = instance_cursor(instance_id * MONO_SPRITE_WORDS + MONO_SPRITE_MASK_OFFSET);
    return read_content_mask(&cursor);
}

fn load_poly_sprite(instance_id: u32) -> PolychromeSprite {
    var cursor = instance_cursor(instance_id * POLY_SPRITE_WORDS);
    let order = read_word(&cursor);
    let pad = read_word(&cursor);
    let grayscale = read_word(&cursor);
    let opacity = read_f32(&cursor);
    let bounds = read_bounds(&cursor);
    let content_mask = read_content_mask(&cursor);
    let corner_radii = read_corners(&cursor);
    let tile = read_atlas_tile(&cursor);
    return PolychromeSprite(order, pad, grayscale, opacity, bounds, content_mask, corner_radii, tile);
}
