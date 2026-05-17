const std = @import("std");

pub fn main() !void {
    const allocator = std.heap.page_allocator;
    _ = allocator;

    std.log.info("Apex Zig Bridge initialized", .{});
}
