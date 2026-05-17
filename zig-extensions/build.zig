const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const wasm = b.addSharedLibrary(.{
        .name = "apex_ext",
        .root_source_file = b.path("src/main.zig"),
        .target = b.resolveTargetQuery(.{
            .cpu_arch = .wasm32,
            .os_tag = .freestanding,
        }),
        .optimize = optimize,
    });
    wasm.rdynamic = true;

    b.installArtifact(wasm);

    const native = b.addExecutable(.{
        .name = "apex_zig_bridge",
        .root_source_file = b.path("src/wasm_bridge.zig"),
        .target = target,
        .optimize = optimize,
    });
    b.installArtifact(native);
}
