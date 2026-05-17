const std = @import("std");

export fn apex_extension_init() u32 {
    return 0;
}

export fn apex_extension_process(data: [*]const u8, len: usize) u32 {
    _ = data;
    _ = len;
    return 0;
}

export fn apex_extension_memory_alloc(size: usize) [*]u8 {
    const slice = std.heap.wasm_allocator.alloc(u8, size) catch return @ptrFromInt(0);
    return slice.ptr;
}

export fn apex_extension_memory_free(ptr: [*]u8, size: usize) void {
    const slice = ptr[0..size];
    std.heap.wasm_allocator.free(slice);
}
