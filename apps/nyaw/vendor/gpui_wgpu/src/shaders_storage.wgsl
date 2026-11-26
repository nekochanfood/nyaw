// Storage-buffer instance transport. Concatenated after `shaders.wgsl` on backends
// with storage buffer support; `shaders_webgl.wgsl` is the WebGL2 counterpart that
// decodes the same records from a texture instead.
//
// NYAW: the buffer is read as raw 32-bit words and decoded by `shaders_records.wgsl`,
// because the packed scene records of vendor/gpui do not match WGSL struct layout.
// The binding starts at the batch's first instance, so word indices start at 0.

@group(1) @binding(0) var<storage, read> b_words: array<u32>;

struct InstanceCursor {
    word_index: u32,
}

fn instance_cursor(word_index: u32) -> InstanceCursor {
    return InstanceCursor(word_index);
}

fn read_word(cursor: ptr<function, InstanceCursor>) -> u32 {
    let word_index = (*cursor).word_index;
    (*cursor).word_index = word_index + 1u;
    return b_words[word_index];
}
